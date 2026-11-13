#!/usr/bin/env python3
"""Private-prefix PHP/Xdebug build. Never invokes brew, doctor, PECL or system installers."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, tarfile, tempfile, zipfile

p = argparse.ArgumentParser()
p.add_argument("recipe", type=pathlib.Path)
p.add_argument("version")
p.add_argument("target")
p.add_argument("output", type=pathlib.Path)
p.add_argument("--cache", type=pathlib.Path)
p.add_argument("--work-dir", type=pathlib.Path)
args = p.parse_args()
args.output = args.output.resolve()
args.output.parent.mkdir(parents=True, exist_ok=True)
recipe = json.loads(args.recipe.read_text())
assert args.version in recipe["php"]
assert args.target in recipe["spc"]
cache_path = args.cache or args.output.parent / "debug-build-inputs"
if cache_path.is_symlink():
    raise RuntimeError("cache directory must not be linked")
cache = cache_path.resolve()
cache.mkdir(parents=True, exist_ok=True)
# Compiler/autotools prefixes must stay short and free of spaces even when app data
# is in "Library/Application Support". The application owns this TempDir in Rust,
# so cancellation still cleans it after the managed process tree has stopped.
if args.work_dir:
    if (
        args.work_dir.is_symlink()
        or not args.work_dir.is_dir()
        or any(args.work_dir.iterdir())
    ):
        raise RuntimeError("work directory must be an empty owned directory")
    work = args.work_dir.resolve()
else:
    work = pathlib.Path(
        tempfile.mkdtemp(prefix="devforge-xdebug-", dir="/private/tmp")
    ).resolve()
if " " in str(work):
    raise RuntimeError("compiler prefix must not contain spaces")
os.chmod(work, 0o700)
prefix = work / "tools"
sources = work / "sources"
prefix.mkdir()
sources.mkdir()
log = args.output.with_suffix(".build.log")
progress = args.output.with_suffix(".progress.json")
token = os.environ.get("GITHUB_TOKEN", "")
env = {
    k: v
    for k, v in os.environ.items()
    if k
    in [
        "HOME",
        "USERPROFILE",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SystemRoot",
        "WINDIR",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "GITHUB_TOKEN",
    ]
}
env["PATH"] = str(prefix / "bin") + ":/usr/bin:/bin:/usr/sbin:/sbin"
env["SPC_CONCURRENCY"] = "4"
env["MACOSX_DEPLOYMENT_TARGET"] = "15.0"
env["HOMEBREW_NO_AUTO_UPDATE"] = "1"


def stage(value):
    progress.write_text(json.dumps({"stage": value, "build_directory": str(work)}))
    print("DEBUG_BUILD_STAGE:" + value, flush=True)


def hash(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def download(meta):
    path = cache / meta["sha256"]
    if path.is_symlink():
        raise RuntimeError("cache input must not be linked")
    if path.exists() and hash(path) == meta["sha256"]:
        return path
    temporary = path.with_suffix(".part")
    if temporary.is_symlink():
        raise RuntimeError("download staging must not be linked")
    # Apple CLT Python may use an older TLS library; the native curl uses system trust.
    # --disable is first so user curl configuration cannot alter this pinned transfer.
    run(
        [
            "/usr/bin/curl",
            "--disable",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "20",
            "--max-time",
            "180",
            "--retry",
            "2",
            "--max-filesize",
            str(1024 * 1024 * 1024),
            "--output",
            str(temporary),
            meta["url"],
        ],
        work,
    )
    if hash(temporary) != meta["sha256"]:
        raise RuntimeError("source checksum mismatch")
    temporary.replace(path)
    return path


def unpack(path, directory):
    directory.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path) as archive:
        members = archive.getmembers()
        if (
            len(members) > 100000
            or sum(m.size for m in members) > 2 * 1024 * 1024 * 1024
        ):
            raise RuntimeError("archive limit")
        for m in members:
            name = pathlib.PurePosixPath(m.name)
            if name.is_absolute() or ".." in name.parts:
                raise RuntimeError("unsafe archive")
            if m.issym() or m.islnk():
                target = pathlib.PurePosixPath(m.linkname)
                resolved = pathlib.PurePosixPath(
                    os.path.normpath(str(name.parent / target if m.issym() else target))
                )
                if target.is_absolute() or ".." in resolved.parts:
                    raise RuntimeError("unsafe link")
            elif not (m.isfile() or m.isdir()):
                raise RuntimeError("unsupported archive type")
        archive.extractall(directory)
    return list(directory.iterdir())


def run(argv, directory):
    with log.open("a") as f:
        child = subprocess.Popen(
            [str(a) for a in argv],
            cwd=directory,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            for line in child.stdout:
                f.write(line.replace(token, "[REDACTED]") if token else line)
                f.flush()
                if f.tell() > 32 * 1024 * 1024:
                    raise RuntimeError("build log limit")
            code = child.wait()
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
        if code:
            raise RuntimeError("build command failed: " + str(argv[0]))


try:
    stage("tools")
    for meta in recipe["tools"]:
        entries = unpack(download(meta), sources / meta["name"])
        source = entries[0]
        if meta["name"] == "cmake":
            (prefix / "bin").mkdir(exist_ok=True)
            for name in ["cmake", "ctest", "cpack"]:
                (prefix / "bin" / name).symlink_to(
                    source / "CMake.app/Contents/bin" / name
                )
        else:
            run(["./configure", "--prefix=" + str(prefix), "--disable-nls"], source)
            run(["/usr/bin/make", "-j4"], source)
            run(["/usr/bin/make", "install"], source)
    stage("sources")
    spcdir = work / "spc"
    unpack(download(recipe["spc"][args.target]), spcdir)
    spc = spcdir / "spc"
    spc.chmod(0o700)
    build = work / "build"
    build.mkdir()
    run(
        [
            spc,
            "install-pkg",
            "pkg-config-"
            + ("aarch64" if args.target == "macos-arm64" else "x86_64")
            + "-macos",
            "--no-interaction",
        ],
        build,
    )
    inputs_dir = work / "inputs"
    inputs_dir.mkdir()
    php = inputs_dir / ("php-" + args.version + ".tar.xz")
    xdebug = inputs_dir / ("xdebug-" + recipe["xdebug"]["version"] + ".tgz")
    shutil.copy2(download(recipe["php"][args.version]), php)
    shutil.copy2(download(recipe["xdebug"]), xdebug)
    run(
        [
            spc,
            "download",
            "--with-php=" + ".".join(args.version.split(".")[:2]),
            "--for-extensions=" + recipe["extensions"] + ",xdebug",
            "--prefer-pre-built",
            "--retry=3",
            "--no-interaction",
            "--custom-url=php-src:" + php.as_uri(),
            "--custom-url=xdebug:" + xdebug.as_uri(),
        ],
        build,
    )
    # Source archives come from reviewed checksums. All resolved dependency blobs are recorded below.
    stage("compile")
    run(
        [
            spc,
            "build",
            recipe["extensions"],
            "--build-cli",
            "--build-fpm",
            "--build-shared=xdebug",
            "--with-config-file-path=/__devforge_no_global_ini__",
            "--with-config-file-scan-dir=/__devforge_no_global_scan__",
            "--no-interaction",
        ],
        build,
    )
    stage("probe")
    tree = build / "buildroot"
    module = tree / "modules/xdebug.so"
    command = [
        str(tree / "bin/php"),
        "-n",
        "-d",
        "zend_extension=" + str(module),
        "-r",
        "echo json_encode(['php'=>PHP_VERSION,'xdebug'=>phpversion('xdebug'),'pdo'=>PDO::getAvailableDrivers()]);",
    ]
    verified = json.loads(subprocess.check_output(command, env=env, text=True))
    assert (
        verified["php"] == args.version
        and verified["xdebug"] == recipe["xdebug"]["version"]
    )
    assert all(x in verified["pdo"] for x in ["mysql", "pgsql", "sqlite"])
    inputs = {
        f.name: hash(f)
        for f in (build / "downloads").iterdir()
        if f.is_file() and not f.name.startswith(".")
    }
    provenance = {
        "schema": 1,
        "version": args.version,
        "target": args.target,
        "recipe_sha256": hash(args.recipe),
        "builder_sha256": hash(pathlib.Path(__file__)),
        "source_lock": json.loads((build / "downloads/.lock.json").read_text()),
        "source_build": True,
        "probe": verified,
        "input_sha256": inputs,
        "compiler": subprocess.check_output(
            ["/usr/bin/clang", "--version"], text=True
        ).splitlines()[0],
    }
    stage("package")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.output, "w", zipfile.ZIP_DEFLATED) as z:
        for src, name in [
            (tree / "bin/php", "php"),
            (tree / "bin/php-fpm", "php-fpm"),
            (module, "xdebug.so"),
        ]:
            z.write(src, name)
        for file in (tree / "license").rglob("*"):
            if file.is_file():
                z.write(file, "licenses/" + str(file.relative_to(tree / "license")))
        z.writestr("provenance.json", json.dumps(provenance, indent=2))
    stage("ready")
    print(
        json.dumps({"archive_sha256": hash(args.output), "probe": verified}), flush=True
    )
except Exception:
    stage("failed")
    raise
finally:
    # Build logs/progress remain for diagnosis; source and tool prefixes are disposable.
    shutil.rmtree(work)
