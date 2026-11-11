#!/usr/bin/env python3
"""Isolated macOS M0 smoke: verified artifacts, Laravel, PostgreSQL, PHP and Vite.

Uses temporary data/configuration and free loopback ports. Does not modify hosts,
system certificate trust, launch services, global PATH, Herd, or existing databases.
Intel tests on an ARM Mac are explicitly reported as Rosetta, not native Intel.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
from contextlib import contextmanager
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import secrets
import shutil
import signal
import socket
import ssl
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request
from urllib.parse import quote
import zipfile

from m0_runtime_audit import digest, safe_member


def probe_hmr(port, client_source, fixture):
    """Verify Vite's direct WebSocket and a file-change notification, not browser rendering."""
    match = re.search(r'const\s+wsToken\s*=\s*["\']([^"\']+)["\']', client_source)
    if not match:
        raise RuntimeError("Could not identify the served Vite WebSocket token")
    key = base64.b64encode(secrets.token_bytes(16)).decode()
    request = ("GET /?token=" + quote(match.group(1)) + " HTTP/1.1\r\nHost: 127.0.0.1:" + str(port) +
               "\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: " + key +
               "\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: vite-hmr\r\nOrigin: http://127.0.0.1:" + str(port) + "\r\n\r\n")
    with socket.create_connection(("127.0.0.1", port), timeout=10) as connection:
        connection.sendall(request.encode())
        received = b""
        while b"\r\n\r\n" not in received:
            chunk = connection.recv(4096)
            if not chunk or len(received) > 32768:
                raise RuntimeError("Invalid Vite WebSocket handshake")
            received += chunk
        headers, remainder = received.split(b"\r\n\r\n", 1)
        lines = headers.decode("iso-8859-1").split("\r\n")
        values = {name.strip().lower(): value.strip() for name, value in
                  (line.split(":", 1) for line in lines[1:] if ":" in line)}
        expected = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        if " 101 " not in lines[0] or not hmac.compare_digest(values.get("sec-websocket-accept", ""), expected):
            raise RuntimeError("Vite WebSocket upgrade was rejected")
        pending = bytearray(remainder)

        def take(count):
            while len(pending) < count:
                chunk = connection.recv(max(4096, count - len(pending)))
                if not chunk:
                    raise RuntimeError("Vite closed the WebSocket")
                pending.extend(chunk)
            data = bytes(pending[:count])
            del pending[:count]
            return data

        entry = next((fixture / name for name in ("resources/js/app.js", "resources/js/app.ts") if (fixture / name).is_file()), None)
        if entry is None:
            raise RuntimeError("Laravel Vite entrypoint was not found")
        with urllib.request.urlopen("http://127.0.0.1:" + str(port) + "/" + entry.relative_to(fixture).as_posix(), timeout=10) as response:
            response.read()
        original = entry.read_bytes()
        try:
            entry.write_bytes(original + b"\n// DevForge M0 HMR probe\n")
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                connection.settimeout(max(0.1, deadline - time.monotonic()))
                head = take(2)
                opcode, length = head[0] & 15, head[1] & 127
                if head[1] & 128:
                    raise RuntimeError("Unexpected masked server frame")
                if length == 126:
                    length = int.from_bytes(take(2), "big")
                elif length == 127:
                    length = int.from_bytes(take(8), "big")
                if length > 1024 * 1024:
                    raise RuntimeError("Unexpectedly large HMR frame")
                payload = take(length)
                if opcode == 1:
                    event = json.loads(payload)
                    if event.get("type") in ("update", "full-reload"):
                        return {"websocket_upgrade": 101, "file_change_notification": event["type"],
                                "scope": "direct Vite transport; browser application and HTTPS reverse-proxy HMR remain unverified"}
            raise TimeoutError("No HMR file-change notification received")
        finally:
            entry.write_bytes(original)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def extract_tar(archive, destination):
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as source:
        for member in source.getmembers():
            if not safe_member(member.name):
                raise ValueError("Unsafe archive path")
            if not (member.isdir() or member.isfile() or member.issym() or member.islnk()):
                raise ValueError("Unsupported archive member")
            if member.issym() or member.islnk():
                base = (destination / member.name).parent if member.issym() else destination
                if not (base / member.linkname).resolve().is_relative_to(destination.resolve()):
                    raise ValueError("Archive link escapes destination")
        source.extractall(destination)


@contextmanager
def running(command, env, cwd, log):
    with log.open("wb") as output:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=output,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            yield process
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGINT)
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)


def wait_port(port, process):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("Fixture process exited before readiness")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.3):
                return
        except OSError:
            time.sleep(0.1)
    raise TimeoutError("Fixture port was not ready")


def run_smoke(args, result, work):
    entries = json.loads(args.catalog.read_text())
    prefix = ["/usr/bin/arch", "-x86_64"] if args.architecture == "x64" else []
    target = "macos-" + args.architecture
    result["execution_mode"] = "rosetta" if prefix and platform.machine() == "arm64" else "native"
    result["artifact_ids"] = []

    def artifact(package, version=None, sapi=None):
        entry = next(e for e in entries if e["package"] == package
                     and (e["target"] in (target, "macos-universal", "php-portable"))
                     and (version is None or e["version"] == version)
                     and (sapi is None or e.get("sapi") == sapi))
        name = Path(entry["url"].split("?", 1)[0]).name
        path = args.cache / (entry["id"] + "-" + name)
        if not entry.get("sha256") or digest(path) != entry["sha256"]:
            raise ValueError("Artifact must match publisher SHA-256 before execution: " + entry["id"])
        result["artifact_ids"].append(entry["id"])
        return path

    def binary(root, name):
        return next(p for p in root.rglob(name) if p.is_file())

    base_env = {key: os.environ[key] for key in ("HOME", "TMPDIR", "LANG", "LC_CTYPE", "USER", "LOGNAME") if key in os.environ}
    base_env.update({"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "COMPOSER_HOME": str(work / "composer-home"),
                     "COMPOSER_CACHE_DIR": str(args.cache.parent / "composer-cache"),
                     "npm_config_cache": str(args.cache.parent / ("npm-cache-" + args.architecture)),
                     "XDG_DATA_HOME": str(work / "xdg-data"), "XDG_CONFIG_HOME": str(work / "xdg-config")})

    def command(argv, cwd=None, env=None, timeout=180, translated=True):
        cmd = (prefix if translated else []) + [str(a) for a in argv]
        completed = subprocess.run(cmd, cwd=cwd or work, env=env or base_env,
                                   capture_output=True, text=True, timeout=timeout)
        if completed.returncode:
            raise RuntimeError("Command failed: {}\n{}".format(
                Path(str(argv[0])).name, (completed.stdout + completed.stderr)[-5000:]))
        return completed.stdout.strip()

    php_versions = sorted({e["version"] for e in entries if e["package"] == "php" and e["target"] == target})
    nodes = sorted({e["version"] for e in entries if e["package"] == "node" and e["target"] == target})
    php = {}
    for version in php_versions:
        roots = {}
        for sapi in ("cli", "fpm"):
            root = work / ("relocated PHP Русский " + version) / sapi
            extract_tar(artifact("php", version, sapi), root)
            roots[sapi] = binary(root, "php" if sapi == "cli" else "php-fpm")
        php[version] = roots
        info = json.loads(command([roots["cli"], "-n", "-r",
            "echo json_encode(['version'=>PHP_VERSION,'sapi'=>PHP_SAPI,'extensions'=>get_loaded_extensions(),'pdo'=>PDO::getAvailableDrivers()],JSON_THROW_ON_ERROR);"]))
        required = {"curl", "dom", "fileinfo", "mbstring", "openssl", "PDO", "pdo_pgsql", "session", "tokenizer", "xml"}
        missing = sorted(required - set(info["extensions"]))
        if missing:
            raise RuntimeError("Missing PHP extensions: " + ", ".join(missing))
        result.setdefault("php", {})[version] = {
            "info": info, "relocated_unicode_path": True,
            "dynamic_dependencies": command(["/usr/bin/otool", "-L", roots["cli"]], translated=False),
            "minimum_macos_metadata": re.findall(r"\bminos\s+(\S+)", command(["/usr/bin/otool", "-l", roots["cli"]], translated=False)),
        }
        print("PHP CLI/extension/relocation checks passed:", version, flush=True)

    node_bins = {}
    for version in nodes:
        root = work / ("node-" + version)
        extract_tar(artifact("node", version), root)
        node = binary(root, "node")
        node_bins[version] = node
        info = json.loads(command([node, "-p", "JSON.stringify({version:process.version,arch:process.arch,platform:process.platform})"]))
        result.setdefault("node", {})[version] = info

    root = work / "caddy"
    extract_tar(artifact("caddy"), root)
    caddy = binary(root, "caddy")
    result["caddy_version"] = command([caddy, "version"])
    print("Caddy executable check passed", flush=True)
    composer = artifact("composer")
    primary_php = php[php_versions[0]]["cli"]
    node = node_bins[nodes[-1]]
    env = {**base_env, "PATH": str(node.parent) + ":" + str(primary_php.parent) + ":" + base_env["PATH"]}
    php_args = [primary_php, "-n", "-d", "openssl.cafile=/etc/ssl/cert.pem", "-d", "curl.cainfo=/etc/ssl/cert.pem"]
    result["composer_version"] = command([*php_args, composer, "--version", "--no-ansi"], env=env)
    fixture = work / "laravel-fixture"
    print("Installing isolated Laravel fixture (no project scripts/plugins)", flush=True)
    command([*php_args, composer, "create-project", "laravel/laravel", fixture, "v13.11.0",
             "--no-dev", "--no-scripts", "--no-plugins", "--no-interaction", "--prefer-dist"], env=env, timeout=600)
    result["laravel_skeleton"] = "v13.11.0"
    result["laravel_framework"] = command([*php_args, fixture / "artisan", "--version", "--no-ansi"], cwd=fixture, env=env)
    print("Laravel fixture installed", result["laravel_framework"], flush=True)

    pg_root = work / "relocated-postgresql"
    # Prefer the checksummed small test bundle. A JAR is just a ZIP container here;
    # Java is not installed or used to run PostgreSQL.
    test_bundle = next((e for e in entries if e["package"] == "postgresql" and e["target"] == target
                        and "Zonky" in e.get("provider", "")), None)
    if test_bundle:
        package = artifact("postgresql", test_bundle["version"])
        nested_path = work / "postgres-payload.txz"
        with zipfile.ZipFile(package) as archive:
            member = next(m for m in archive.infolist() if m.filename.endswith((".txz", ".tar.gz", ".tgz")))
            with archive.open(member) as source, nested_path.open("wb") as output:
                shutil.copyfileobj(source, output)
        extract_tar(nested_path, pg_root)
        result["postgresql_provider"] = test_bundle["provider"]
    else:
        # Mount a verified provider image read-only; never run its app or installer.
        dmg = artifact("postgresql")
        mounted = plistlib.loads(subprocess.check_output(["hdiutil", "attach", "-readonly", "-nobrowse", "-plist", str(dmg)]))
        mount = next(Path(e["mount-point"]) for e in mounted["system-entities"] if "mount-point" in e)
        try:
            pg_version = next(mount.glob("*.app/Contents/Versions/18"))
            shutil.copytree(pg_version, pg_root, symlinks=True)
        finally:
            subprocess.run(["hdiutil", "detach", str(mount)], check=True, capture_output=True)
        result["postgresql_provider"] = "Postgres.app"
    pg_bin = binary(pg_root, "postgres").parent
    result["postgresql_version"] = command([pg_bin / "postgres", "--version"])
    print("PostgreSQL executable check passed", result["postgresql_version"], flush=True)
    result["postgresql_dependencies"] = command(["/usr/bin/otool", "-L", pg_bin / "postgres"], translated=False)

    pg_port = free_port()
    password = secrets.token_urlsafe(24)
    result["_redact_values"] = [password]
    pwfile = work / "pg-password"
    pwfile.write_text(password + "\n")
    pwfile.chmod(0o600)
    pgdata = work / "pgdata"
    command([pg_bin / "initdb", "-D", pgdata, "--encoding=UTF8", "--no-locale", "--username=devforge_m0",
             "--pwfile=" + str(pwfile), "--auth-host=scram-sha-256", "--auth-local=scram-sha-256"])
    sockets = work / "pg-sockets"
    sockets.mkdir()
    pg_env = {**env, "PGPASSWORD": password}
    with running(prefix + [str(pg_bin / "postgres"), "-D", str(pgdata), "-h", "127.0.0.1", "-p", str(pg_port),
                           "-k", str(sockets)], pg_env, work, work / "postgres.log") as pg:
        wait_port(pg_port, pg)
        if (pg_bin / "createdb").is_file():
            command([pg_bin / "createdb", "-h", "127.0.0.1", "-p", str(pg_port), "-U", "devforge_m0", "devforge_m0"], env=pg_env)
        else:
            bootstrap_env = {**env, "M0_PG_PORT": str(pg_port), "M0_PG_PASSWORD": password}
            command([*php_args, "-r", "$db=new PDO('pgsql:host=127.0.0.1;port='.getenv('M0_PG_PORT').';dbname=postgres','devforge_m0',getenv('M0_PG_PASSWORD'));$db->exec('CREATE DATABASE devforge_m0');echo 'created';"], env=bootstrap_env)
        (fixture / ".env").write_text(
            "APP_NAME=DevForge-M0\nAPP_ENV=testing\nAPP_DEBUG=false\nAPP_KEY=base64:" + base64.b64encode(secrets.token_bytes(32)).decode() +
            "\nDB_CONNECTION=pgsql\nDB_HOST=127.0.0.1\nDB_PORT=" + str(pg_port) +
            "\nDB_DATABASE=devforge_m0\nDB_USERNAME=devforge_m0\nDB_PASSWORD=" + password +
            "\nCACHE_STORE=array\nSESSION_DRIVER=array\nQUEUE_CONNECTION=sync\nLOG_CHANNEL=stderr\n", encoding="utf-8")
        with (fixture / "routes/web.php").open("a", encoding="utf-8") as routes:
            routes.write("\n\\Illuminate\\Support\\Facades\\Route::get('/m0', fn () => response()->json(["
                         "'php'=>PHP_VERSION,'sapi'=>PHP_SAPI,'database'=>\\Illuminate\\Support\\Facades\\DB::selectOne('select current_database() as name, version() as version'),"
                         "'samples'=>['ru'=>'Готово','uz'=>'Tayyor','en'=>'Ready']],200,[],JSON_UNESCAPED_UNICODE));\n")
        for version, binaries in php.items():
            fpm_port, http_port, https_port = free_port(), free_port(), free_port()
            fpm_config = work / ("fpm-" + version + ".conf")
            fpm_config.write_text("[global]\ndaemonize=no\nerror_log=" + str(work / ("fpm-" + version + ".log")) +
                                  "\n[fixture]\nlisten=127.0.0.1:" + str(fpm_port) +
                                  "\npm=static\npm.max_children=1\ncatch_workers_output=yes\nclear_env=no\n")
            caddy_config = work / ("Caddyfile-" + version)
            sites = ""
            for scheme, port in (("http", http_port), ("https", https_port)):
                sites += scheme + "://127.0.0.1:" + str(port) + " {\n bind 127.0.0.1\n"
                if scheme == "https":
                    sites += " tls internal\n"
                sites += ' root * "' + str(fixture / "public") + '"\n php_fastcgi 127.0.0.1:' + str(fpm_port) + "\n file_server\n}\n"
            caddy_config.write_text("{\n admin off\n auto_https disable_redirects\n skip_install_trust\n}\n" + sites)
            with running(prefix + [str(binaries["fpm"]), "-n", "-y", str(fpm_config)], env, work, work / ("fpm-process-" + version + ".log")) as fpm:
                wait_port(fpm_port, fpm)
                with running(prefix + [str(caddy), "run", "--config", str(caddy_config), "--adapter", "caddyfile"], env, work, work / ("caddy-" + version + ".log")) as server:
                    wait_port(http_port, server)
                    with urllib.request.urlopen("http://127.0.0.1:" + str(http_port) + "/m0", timeout=20) as response:
                        data = json.load(response)
                    if data["php"] != version or data["database"]["name"] != "devforge_m0":
                        raise RuntimeError("Wrong PHP or database binding")
                    root_ca = work / "xdg-data/caddy/pki/authorities/local/root.crt"
                    deadline = time.monotonic() + 15
                    while not root_ca.exists() and time.monotonic() < deadline:
                        time.sleep(0.1)
                    context = ssl.create_default_context(cafile=str(root_ca))
                    with urllib.request.urlopen("https://127.0.0.1:" + str(https_port) + "/m0", context=context, timeout=20) as response:
                        tls_data = json.load(response)
                    result["php"][version]["laravel_http"] = data
                    result["php"][version]["laravel_https_explicit_ca"] = tls_data["php"] == version
                    result["php"][version]["system_ca_trust_changed"] = False
                    print("Laravel HTTP/HTTPS/PDO checks passed:", version, flush=True)
        npm_cli = node.parent.parent / "lib/node_modules/npm/bin/npm-cli.js"
        command([node, npm_cli, "install", "--no-audit", "--no-fund"], cwd=fixture, env=env, timeout=600)
        result["vite_version"] = json.loads((fixture / "node_modules/vite/package.json").read_text())["version"]
        for version, node_binary in node_bins.items():
            node_env = {**env, "PATH": str(node_binary.parent) + ":" + str(primary_php.parent) + ":" + base_env["PATH"]}
            cli = node_binary.parent.parent / "lib/node_modules/npm/bin/npm-cli.js"
            command([node_binary, cli, "run", "build"], cwd=fixture, env=node_env, timeout=180)
            result["node"][version]["vite_build"] = True
            print("Vite asset build passed:", version, flush=True)
        vite_port = free_port()
        with running(prefix + [str(node), str(npm_cli), "run", "dev", "--", "--host", "127.0.0.1", "--port", str(vite_port), "--strictPort"],
                     env, fixture, work / "vite.log") as vite:
            wait_port(vite_port, vite)
            with urllib.request.urlopen("http://127.0.0.1:" + str(vite_port) + "/@vite/client", timeout=15) as response:
                result["vite_client_http_status"] = response.status
                client_source = response.read().decode("utf-8")
            result["vite_hmr"] = probe_hmr(vite_port, client_source, fixture)
            print("Direct Vite WebSocket/file-change check passed", flush=True)
        result["postgresql_query"] = "Laravel PDO query passed"
    result["fixture_lockfiles"] = {name: digest(fixture / name) for name in ("composer.lock", "package-lock.json")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", required=True, type=Path)
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--architecture", required=True, choices=("arm64", "x64"))
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This runner tests macOS only; Windows needs a real Windows runner.")
    result = {"recorded_at": datetime.now(timezone.utc).isoformat(), "host_os": platform.mac_ver()[0],
              "host_architecture": platform.machine(), "requested_architecture": args.architecture,
              "system_configuration_changed": False, "stages": "runtime smoke, not desktop UI/i18n validation"}
    with tempfile.TemporaryDirectory(prefix="devforge-m0-") as directory:
        work = Path(directory)
        try:
            run_smoke(args, result, work)
            result["status"] = "passed"
        except Exception as exc:
            result["status"] = "failed"
            result["error"] = str(exc).replace(str(work), "<temporary-fixture>")
            result["diagnostics"] = {p.name: p.read_text(errors="replace")[-4000:].replace(str(work), "<temporary-fixture>")
                                     for p in work.glob("*.log")}
        # Strip task-local paths; never record the fixture .env or passwords.
        sensitive = result.pop("_redact_values", [])
        serialized = json.dumps(result).replace(str(work), "<temporary-fixture>")
        for value in sensitive:
            serialized = serialized.replace(value, "<redacted>")
        result = json.loads(serialized)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(result, indent=2, ensure_ascii=True) + "\n")
    print(json.dumps({"status": result["status"], "architecture": args.architecture, "error": result.get("error")}, ensure_ascii=True))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
