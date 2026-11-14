#!/usr/bin/env python3
"""Scoped native agent-connections acceptance, using real Tauri IPC and public MCP.

Build preparation is deliberately external: this harness never builds or changes
manager-artifact.json. Run only with the integrator's frozen debug e2e CLI/GUI pair.
All writable state is a new marked /tmp fixture. No services/runtimes are installed.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import socket
import stat
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
ELEMENT = "element-6066-11e4-a52e-4f735466cecf"
AGENTS = "//*[contains(concat(' ',normalize-space(@class),' '),' agent-connections ')]"


def wait(function, seconds=35):
    until, last = time.monotonic() + seconds, None
    while time.monotonic() < until:
        try:
            value = function()
            if value:
                return value
        except Exception as error:
            last = type(error).__name__
        time.sleep(0.1)
    raise AssertionError(f"bounded native condition timed out ({last})")


def literal(text):
    if "'" not in text:
        return "'" + text + "'"
    if '"' not in text:
        return '"' + text + '"'
    return "concat(" + ',"\'",'.join("'" + part + "'" for part in text.split("'")) + ")"


class MCP:
    def __init__(self, config, stderr):
        entry, = config["mcpServers"].values()
        self.child = subprocess.Popen([entry["command"], *entry["args"]],
                                      env={**os.environ, **entry.get("env", {})},
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=stderr, text=True, encoding="utf-8")
        self.lines, self.serial = queue.Queue(maxsize=16), 0
        def reader():
            for line in iter(lambda: self.child.stdout.readline(1_048_577), ""):
                try:
                    if len(line) > 1_048_576:
                        raise ValueError("bounded MCP frame exceeded")
                    value = json.loads(line)
                except (ValueError, TypeError):
                    self.lines.put({"invalid_frame": True})
                    return
                self.lines.put(value)
        threading.Thread(target=reader, daemon=True).start()
        result = self.call("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                           "clientInfo": {"name": "native-ui-acceptance", "version": "1"}})
        assert "result" in result
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, value):
        self.child.stdin.write(json.dumps(value, ensure_ascii=False) + "\n")
        self.child.stdin.flush()

    def call(self, method, params):
        self.serial += 1
        self.send({"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params})
        result = self.lines.get(timeout=20)
        assert result.get("id") == self.serial and not result.get("invalid_frame")
        return result

    def close(self):
        self.child.stdin.close()
        self.child.wait(timeout=15)
        assert self.child.returncode == 0


class Harness:
    def __init__(self, cli, application, output, resume=None, remaining_only=False):
        self.cli, self.application, self.output = cli.resolve(), application.resolve(), output.resolve()
        assert self.cli.is_file() and self.application.is_file()
        assert self.cli.parent == self.application.parent, "use the frozen adjacent e2e pair"
        manifest = json.loads((ROOT / "distribution/generated/manager-artifact.json").read_text())
        assert manifest["filename"] == self.cli.name
        digest = hashlib.sha256()
        with self.cli.open("rb") as executable:
            while chunk := executable.read(1024 * 1024):
                digest.update(chunk)
        assert digest.hexdigest() == manifest["sha256"]
        gui_digest = hashlib.sha256()
        with self.application.open("rb") as executable:
            while chunk := executable.read(1024 * 1024):
                gui_digest.update(chunk)
        self.artifact = {"cli_sha256": digest.hexdigest(), "gui_sha256": gui_digest.hexdigest(),
                         "target": manifest["target"], "profile": "debug_e2e_custom_protocol",
                         "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                         "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())}
        self.resume_existing = resume is not None
        self.remaining_only = remaining_only
        self.parent = resume.resolve() if resume else Path(tempfile.mkdtemp(prefix="df-agent-ui-", dir="/tmp")).resolve()
        if resume:
            metadata = self.parent.lstat()
            assert self.parent.parent == Path("/tmp").resolve() and self.parent.name.startswith("df-agent-ui-")
            assert stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.geteuid() and metadata.st_mode & 0o077 == 0
            assert (self.parent / "data/.devforge-disposable-e2e.json").is_file()
        self.data, self.session, self.process, self.manager_pid = self.parent / "data", "", None, None
        self.environment = {**os.environ, "DEVFORGE_DATA_DIR": str(self.data)}
        self.output.mkdir(parents=True, exist_ok=True)
        self.checks, self.captures, self.audits, self.focus = [], [], [], []
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.label_catalogs = {locale: json.loads((ROOT / f"shared/locales/{locale}.json").read_text())
                               for locale in ["en", "ru", "uz-Latn"]}
        self.locale = "en"
        self.mcp, self.mcp_log, self.gui_log = None, None, None

    def label(self, key):
        value = self.label_catalogs[self.locale]
        for part in key.split("."):
            value = value[part]
        return value

    def request(self, method, path, value=None, timeout=30):
        request = urllib.request.Request(self.base + path, data=None if value is None else json.dumps(value).encode(),
                                         method=method, headers={"Content-Type": "application/json"})
        try:
            with self.opener.open(request, timeout=timeout) as response:
                result = json.load(response).get("value")
        except urllib.error.HTTPError as error:
            # Do not emit arbitrary browser/IPC payloads into an acceptance report.
            raise RuntimeError(f"native WebDriver HTTP {error.code}") from None
        if isinstance(result, dict) and "error" in result:
            raise RuntimeError("native WebDriver command failed: " + str(result["error"]))
        return result

    def call(self, method, path, value=None):
        return self.request(method, "/session/" + self.session + path, value)

    def script(self, source, *values):
        return self.call("POST", "/execute/sync", {"script": source, "args": list(values)})

    def invoke(self, name, value=None):
        result = self.call("POST", "/execute/async", {
            "script": "const done=arguments[arguments.length-1];window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(data=>done({ok:true,data}),()=>done({ok:false}));",
            "args": [name, value or {}]})
        assert result["ok"], "real native IPC refused fixture action"
        return result.get("data")

    def element(self, using, value):
        return self.call("POST", "/element", {"using": using, "value": value})[ELEMENT]

    def click(self, css=None, xpath=None):
        element = wait(lambda: self.element("xpath" if xpath else "css selector", xpath or css))
        wait(lambda: self.call("GET", f"/element/{element}/enabled"))
        self.call("POST", f"/element/{element}/click", {})

    def button(self, key, scope=AGENTS):
        self.click(xpath=f"({scope})//button[normalize-space(.)={literal(self.label('agentConnections.' + key))}]")

    def fill(self, css, value):
        wait(lambda: self.script("const e=document.querySelector(arguments[0]);return e&&!e.disabled&&!e.readOnly&&e.getClientRects().length>0", css))
        self.click(css)
        element = self.element("css selector", css)
        self.call("POST", f"/element/{element}/clear", {})
        element = self.element("css selector", css)
        self.call("POST", f"/element/{element}/value", {"text": value})

    def select(self, css, value):
        self.script("const e=document.querySelector(arguments[0]);e.value=arguments[1];e.dispatchEvent(new Event('change',{bubbles:true}));", css, value)

    def key(self, value):
        self.call("POST", "/actions", {"actions": [{"type": "key", "id": "keyboard", "actions": [
            {"type": "keyDown", "value": value}, {"type": "keyUp", "value": value}]}]})

    def settled(self):
        wait(lambda: self.script("return document.querySelector('.agent-connections')?.getAttribute('aria-busy')==='false'"))

    def cli_api(self, arguments, token, successful=True):
        result = subprocess.run([str(self.cli), "api", *arguments, "--credential-stdin"],
                                env=self.environment, input=token + "\n", capture_output=True,
                                text=True, encoding="utf-8", timeout=30)
        value = json.loads(result.stdout)
        assert bool(value["ok"]) is successful
        assert (result.returncode == 0) is successful
        return value

    def health(self):
        reply = subprocess.run([str(self.cli), "manager", "health"], env=self.environment,
                               capture_output=True, text=True, timeout=8)
        if reply.returncode:
            return None
        value = json.loads(reply.stdout)
        return value.get("data") if value.get("ok") else None

    def prepare(self):
        initialized = subprocess.run([str(self.cli), "manager", "__fixture-root-init", "--internal-fixture-store"],
                                     env=self.environment, capture_output=True, text=True, timeout=15)
        assert initialized.returncode == 0 and json.loads(initialized.stdout) == {"initialized": True}
        folder = self.data / "discovery-empty"
        folder.mkdir(mode=0o700)

    def launch(self):
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.base = f"http://127.0.0.1:{port}"
        self.gui_log = (self.parent / "gui.log").open("wb")
        self.process = subprocess.Popen([str(self.application)], env={**self.environment, "TAURI_WEBDRIVER_PORT": str(port)},
                                        stdout=self.gui_log, stderr=self.gui_log)
        wait(lambda: self.request("GET", "/status", timeout=2), 60)
        self.session = self.request("POST", "/session", {"capabilities": {"alwaysMatch": {}}})["sessionId"]
        self.call("POST", "/timeouts", {"script": 120000, "implicit": 0})
        wait(lambda: self.script("return !!document.querySelector('h1')"))
        self.frontend_origin = self.script("return location.origin")
        assert "127.0.0.1:1420" not in self.frontend_origin and "localhost:1420" not in self.frontend_origin
        wait(lambda: self.invoke("workspace_snapshot")["snapshot"] is not None)
        saved = self.invoke("save_settings", {"settings": {"project_folder": str(self.data / "discovery-empty")}})
        assert saved["settings"]["project_folder"] == str(self.data / "discovery-empty")
        owner = wait(self.health)
        self.manager_pid = owner["owner_pid"]
        assert self.manager_pid != self.process.pid
        self.checks.append("native_webview_real_ipc_single_manager")

    def import_projects(self):
        snapshot = self.invoke("workspace_snapshot")["snapshot"]
        php = next(p["id"] for p in snapshot["runtimes"] if p["kind"] == "php" and p["variant"] == "cli")
        node = next(p["id"] for p in snapshot["runtimes"] if p["kind"] == "node")
        projects = []
        for name in ["Alpha acceptance", "Bravo isolation"]:
            path = self.parent / name
            (path / "public").mkdir(parents=True)
            (path / "public/index.php").write_text("<?php /* metadata fixture only */")
            (path / "composer.json").write_text('{"require":{"php":"^8.3","laravel/framework":"^13"}}')
            value = self.invoke("workspace_command", {"command": {"action": "import", "path": str(path), "name": name,
                "php": php, "node": node, "domain": name.split()[0].lower() + ".devforge.test", "https": False, "vite": False, "queue": False}})
            projects.append(value["project_id"])
        self.projects = projects
        self.click('a[href="#settings"]')
        self.click('input[name="language"][value="en"]')
        wait(lambda: self.script("return document.documentElement.lang==='en'"))
        self.click('input[name="appearance"][value="light"]')
        wait(lambda: self.script("return document.documentElement.dataset.theme==='light'"))
        self.settled()
        self.select('select[name="agent-project"]', projects[0])

    def issue_and_read(self):
        self.click('input[name="agent-project_manage"]')
        self.click('.agent-connections > form.agent-form button[type="submit"]')
        self.settled()
        snippet = wait(lambda: self.script("return document.querySelector('.agent-review textarea')?.value"))
        config = json.loads(snippet)
        entry, = config["mcpServers"].values()
        token_path = Path(entry["args"][entry["args"].index("--credential-file") + 1])
        assert token_path.resolve().is_relative_to(self.data)
        metadata = token_path.lstat()
        assert stat.S_ISREG(metadata.st_mode) and not token_path.is_symlink() and metadata.st_mode & 0o077 == 0
        credential = json.loads(token_path.read_text())
        assert credential["project_id"] == self.projects[0] and len(credential["credential"]) == 64
        self.token, self.config, self.grant = credential["credential"], config, token_path.parent.name.removeprefix("agent-connection-")
        assert self.token not in self.script("return document.body.innerText+[...document.querySelectorAll('input,textarea')].map(e=>e.value).join(' ')")
        self.mcp_log = (self.parent / "mcp.log").open("wb")
        self.mcp = MCP(config, self.mcp_log)
        listed = self.mcp.call("tools/call", {"name": "projects.list", "arguments": {}})
        assert listed["result"]["isError"] is False
        text = json.dumps(listed)
        assert self.projects[0] in text and self.projects[1] not in text and "Bravo isolation" not in text
        resources = self.mcp.call("resources/list", {})["result"]["resources"]
        assert [r["uri"] for r in resources] == [f"devforge://project/{self.projects[0]}/summary"]
        denied = self.mcp.call("resources/read", {"uri": f"devforge://project/{self.projects[1]}/summary"})
        assert "error" in denied or denied.get("result", {}).get("isError") is True
        self.cli_api(["project", "inspect", self.projects[1]], self.token, successful=False)
        self.checks += ["dom_grant_issue_private_config_without_bearer_in_dom", "actual_config_mcp_project_a_only_and_project_b_denied"]

    def approve_and_reconcile(self):
        project = self.projects[0]
        row = next(r for r in self.invoke("workspace_snapshot")["snapshot"]["projects"] if r["project"]["id"] == project)
        self.operation_key = "native-ui-stop-acceptance"
        flags = [project, "--revision", str(row["project"]["revision"]), "--idempotency-key", self.operation_key]
        plan = self.cli_api(["service", "plan", "stop", *flags], self.token)["data"]
        self.button("refresh")
        self.settled()
        self.viewport(1536, 1024)
        self.button("review")
        wait(lambda: self.script("return document.querySelector('dialog')?.open"))
        assert self.script("return document.activeElement===document.querySelector('dialog input[data-initial-focus]')")
        self.key("\ue004")
        assert self.script("return document.querySelector('dialog').contains(document.activeElement)")
        self.key("\ue00c")
        wait(lambda: not self.script("return !!document.querySelector('dialog[open]')"))
        assert self.script("return document.activeElement?.textContent.trim()===arguments[0]", self.label("agentConnections.review"))
        self.button("review")
        wait(lambda: self.script("return document.querySelector('dialog')?.open"))
        self.capture("review-dialog-en-light", audit=True)
        self.click('dialog input[type="checkbox"]')
        self.button("approve", "//dialog")
        self.settled()
        self.operation = self.cli_api(["service", "stop", *flags, "--review", plan["digest"]], self.token)["data"]["operation_id"]
        wait(lambda: self.cli_api(["operation", "lookup", self.operation_key, "--project", project], self.token)["data"]["state"] == "succeeded")
        self.button("refresh")
        self.settled()
        operation_scope = f"{AGENTS}//li[contains(@class,'agent-entry')][.//code[normalize-space(.)={literal(self.operation)}]]"
        self.button("checkStatus", operation_scope)
        self.settled()
        cancel = self.element("xpath", f"({operation_scope})//button[normalize-space(.)={literal(self.label('agentConnections.cancel'))}]")
        assert self.call("GET", f"/element/{cancel}/enabled") is False
        self.click(xpath=f"{AGENTS}//summary[normalize-space(.)={literal(self.label('agentConnections.reconcile'))}]")
        self.select('select[name="lookup-grant"]', self.grant)
        self.fill('input[name="lookup-idempotency"]', self.operation_key)
        self.click('.agent-connections details form button[type="submit"]')
        self.settled()
        assert self.operation in self.script("return document.querySelector('.agent-connections details').innerText")
        self.checks += ["actual_dom_review_approval_and_noop_stop", "review_dialog_keyboard_escape_restores_focus", "operation_status_reconcile_and_terminal_cancel_disabled"]

    def viewport(self, width, height):
        for _ in range(5):
            actual = self.script("return {width:innerWidth,height:innerHeight}")
            if actual == {"width": width, "height": height}:
                return actual
            rect = self.call("GET", "/window/rect")
            self.call("POST", "/window/rect", {"width": rect["width"] + width - actual["width"],
                                                "height": rect["height"] + height - actual["height"]})
        raise AssertionError("native content viewport did not match requested dimensions")

    def capture(self, name, audit=True):
        image = base64.b64decode(self.call("GET", "/screenshot"))
        (self.output / f"{name}.png").write_bytes(image)
        self.captures.append({"case": name, "path": f"{name}.png", "viewport": self.script("return {width:innerWidth,height:innerHeight,dpr:devicePixelRatio}"), "window_rect": self.call("GET", "/window/rect")})
        if audit:
            self.script((ROOT / "node_modules/axe-core/axe.min.js").read_text())
            violations = self.call("POST", "/execute/async", {"script": "const done=arguments[arguments.length-1];window.axe.run(document.querySelector('dialog[open]')||document.querySelector('.agent-connections'),{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(r=>done(r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))),()=>done({audit_error:true}));", "args": []})
            self.audits.append({"case": name, "violations": violations})
            assert violations == [], f"scoped native axe failed for {name}"

    def matrix(self):
        for locale, theme, width, height in [("en", "light", 1536, 1024), ("ru", "dark", 820, 620), ("uz-Latn", "light", 820, 620)]:
            self.click(f'input[name="language"][value="{locale}"]')
            wait(lambda: self.script("return document.documentElement.lang===arguments[0]", locale))
            self.locale = locale
            self.click(f'input[name="appearance"][value="{theme}"]')
            wait(lambda: self.script("return document.documentElement.dataset.theme===arguments[0]", theme))
            self.settled()
            self.viewport(width, height)
            self.script("document.querySelector('.agent-connections').scrollIntoView({block:'start'});document.querySelector('.agent-toolbar button:not(:disabled)').focus({preventScroll:true});")
            assert self.script("return document.documentElement.scrollWidth<=innerWidth+1")
            self.key("\ue004")
            focused = self.script("const e=document.activeElement,s=e&&getComputedStyle(e);return {tag:e?.tagName,body:e===document.body,contained:document.querySelector('.agent-connections').contains(e),visible:!!s&&(s.outlineStyle!=='none'||s.boxShadow!=='none')}")
            print("AGENT_UI_FOCUS: " + json.dumps({"locale": locale, "theme": theme, **focused}), flush=True)
            assert not focused["body"] and focused["contained"]
            focused["scope"] = "synthetic WebDriver focus observation; native OS focus-visible not asserted"
            name = f"agents-{locale}-{theme}-{width}x{height}"
            self.focus.append({"case": name, **focused})
            self.capture(name)
        self.checks.append("three_representative_native_locale_theme_size_axe_focus_cases")

    def resume_remaining(self):
        self.launch()
        rows = self.invoke("workspace_snapshot")["snapshot"]["projects"]
        self.projects = [next(row["project"]["id"] for row in rows if row["project"]["name"] == name) for name in ["Alpha acceptance", "Bravo isolation"]]
        assert len(rows) == 2
        self.click('a[href="#settings"]')
        self.settled()
        self.button("viewConfig")
        self.settled()
        config = json.loads(wait(lambda: self.script("return document.querySelector('.agent-review textarea')?.value")))
        entry, = config["mcpServers"].values()
        path = Path(entry["args"][entry["args"].index("--credential-file")+1])
        assert path.resolve().is_relative_to(self.data)
        credential = json.loads(path.read_text())
        assert credential["project_id"] == self.projects[0]
        self.token, self.grant = credential["credential"], path.parent.name.removeprefix("agent-connection-")
        self.operation_key = "native-ui-stop-acceptance"
        self.operation = self.cli_api(["operation", "lookup", self.operation_key, "--project", self.projects[0]], self.token)["data"]["operation_id"]
        self.mcp_log = (self.parent / "mcp-resume.log").open("wb")
        self.mcp = MCP(config, self.mcp_log)
        if self.remaining_only:
            self.checks += ["reused_prior_same_fixture_dom_issue_mcp_scope_review_successful_stop_reconcile_status_and_modal_keyboard"]
            return
        self.click(xpath=f"{AGENTS}//summary[normalize-space(.)={literal(self.label('agentConnections.reconcile'))}]")
        wait(lambda: self.script("return document.querySelector('input[name=lookup-idempotency]').closest('details').open"))
        self.select('select[name="lookup-grant"]', self.grant)
        self.fill('input[name="lookup-idempotency"]', self.operation_key)
        self.click('.agent-connections details form button[type="submit"]')
        self.settled()
        assert self.operation in self.script("return document.querySelector('input[name=lookup-idempotency]').closest('details').innerText")
        operation_scope = f"{AGENTS}//li[contains(@class,'agent-entry')][.//code[normalize-space(.)={literal(self.operation)}]]"
        self.button("checkStatus", operation_scope)
        self.settled()
        cancel = self.element("xpath", f"({operation_scope})//button[normalize-space(.)={literal(self.label('agentConnections.cancel'))}]")
        assert self.call("GET", f"/element/{cancel}/enabled") is False
        self.checks += ["reused_prior_same_fixture_dom_issue_mcp_scope_review_and_successful_stop", "same_key_dom_reconciliation_status_and_disabled_terminal_cancel"]

    def revoke(self):
        self.locale = "en"
        self.click('input[name="language"][value="en"]')
        wait(lambda: self.script("return document.documentElement.lang==='en'"))
        self.button("revoke")
        wait(lambda: self.script("return document.querySelector('dialog')?.open"))
        self.button("revoke", "//dialog")
        self.settled()
        refused = self.mcp.call("tools/call", {"name": "projects.get", "arguments": {}})
        assert "error" in refused or refused.get("result", {}).get("isError") is True
        self.mcp.close()
        self.mcp = None
        assert self.health()["owner_pid"] == self.manager_pid
        assert all(row["state"] == "stopped" for row in self.invoke("workspace_snapshot")["snapshot"]["projects"])
        packages = self.data / "runtimes/packages"
        assert not packages.exists() or not any(packages.iterdir())
        self.checks += ["dom_revoke_invalidates_actual_mcp_grant", "no_services_or_runtime_downloads"]

    def cleanup(self):
        if self.mcp:
            self.mcp.close()
            self.mcp = None
        if self.process and self.process.poll() is None:
            try:
                self.invoke("quit_application")
            except Exception:
                pass
            try:
                self.process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=10)
        current = self.health()
        if current:
            owner = current["owner_pid"]
            if self.manager_pid is not None:
                assert owner == self.manager_pid, "fixture owner changed; do not signal an unknown process"
            command = subprocess.check_output(["ps", "-p", str(owner), "-o", "command="], text=True).strip()
            assert command.startswith(str(self.cli) + " manager serve ") and "--internal-fixture-store" in command
            # Even a failed page load/quit can leave the dedicated fixture owner.
            # Resolve only this marked root and validate the frozen e2e sidecar.
            self.manager_pid = owner
            os.kill(owner, 2)
            wait(lambda: self.health() is None, 20)
        if self.gui_log:
            self.gui_log.close()
        if self.mcp_log:
            self.mcp_log.close()

    def run(self):
        try:
            steps = [("resume same owned fixture and reconcile", self.resume_remaining)] if self.resume_existing else [
                ("fresh marked root", self.prepare), ("native launch", self.launch),
                ("metadata project import", self.import_projects), ("DOM grant and actual MCP", self.issue_and_read),
                ("DOM approval and reconciliation", self.approve_and_reconcile)]
            steps += [("representative native locale/theme/size cases", self.matrix), ("DOM revoke", self.revoke)]
            for name, step in steps:
                print("AGENT_UI_PHASE: " + name, flush=True)
                step()
        finally:
            self.cleanup()
        assert self.health() is None
        lifecycle = json.loads((self.data / "control/quit-inhibited.json").read_text())
        assert lifecycle["inhibited"] is True and lifecycle["shutdown"]["outcome"] == "succeeded"
        report = {"passed": True, "native_webview": True, "ipc_mocked": False,
                  "checks": self.checks, "captures": self.captures, "axe": self.audits,
                  "keyboard_focus": self.focus, "fixture_parent": str(self.parent),
                  "manager_pid": self.manager_pid, "native_store_provider_used": False,
                  "artifact": self.artifact, "frontend_origin": self.frontend_origin,
                  "os_security_permissions_changed": False,
                  "services_started": False, "runtime_downloads": False,
                  "visual_review": "requires integrator inspection of captured PNGs"}
        (self.output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print("AGENT_CONNECTIONS_NATIVE_RESULT:" + json.dumps({"passed": True, "checks": len(self.checks),
                                                              "captures": len(self.captures), "scoped_axe_cases": len(self.audits)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--application", required=True, type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "output/native/agent-connections")
    parser.add_argument("--resume-owned-parent", type=Path)
    parser.add_argument("--remaining-only", action="store_true")
    arguments = parser.parse_args()
    if arguments.remaining_only and not arguments.resume_owned_parent:
        parser.error("--remaining-only requires the already owned fixture")
    Harness(arguments.cli, arguments.application, arguments.output, arguments.resume_owned_parent, arguments.remaining_only).run()


if __name__ == "__main__":
    main()
