#!/usr/bin/env python3
"""Native WKWebView/WebView2 acceptance via debug-only embedded W3C WebDriver.
No invoke mocking. Only lifecycle observation/control uses explicit native IPC.
"""
import argparse, base64, json, os, pathlib, signal, socket, subprocess, time, urllib.request, urllib.error, http.client, uuid

p = argparse.ArgumentParser()
p.add_argument("fixture", type=pathlib.Path)
p.add_argument("application", type=pathlib.Path)
p.add_argument("launcher", type=pathlib.Path)
p.add_argument(
    "--output", type=pathlib.Path, default=pathlib.Path("output/playwright/m7-xdebug")
)
args = p.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
base = f"http://127.0.0.1:{port}"
session = ""
process = None
accessibility = []
captures = []
viewport_attempts = []
performance = []
paint_waits = []
operation_timings = []
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(method, path, data=None, timeout=120):
    req = urllib.request.Request(
        base + path,
        data=None if data is None else json.dumps(data).encode(),
        headers={"Content-Type": "application/json"},
        method=method,
    )
    try:
        with opener.open(req, timeout=timeout) as response:
            result = json.load(response)
    except urllib.error.HTTPError as e:
        raise RuntimeError(e.read().decode()) from e
    value = result.get("value")
    if isinstance(value, dict) and "error" in value:
        raise RuntimeError(json.dumps(value))
    return value


def call(method, path, data=None):
    value = request(method, "/session/" + session + path, data)
    if (
        path == "/execute/async"
        and data
        and "animation-frame" in data.get("script", "")
    ):
        paint_waits.append(value)
    return value


def script(source, *values):
    return call("POST", "/execute/sync", {"script": source, "args": list(values)})


def begin_frames():
    script(
        "window.dfFrames={count:0,total:0,max:0,started:performance.now(),last:performance.now(),active:true};const tick=t=>{const p=window.dfFrames;if(!p.active)return;const gap=t-p.last;if(gap>=0){p.count++;p.total+=gap;p.max=Math.max(p.max,gap)}p.last=t;requestAnimationFrame(tick)};requestAnimationFrame(tick)"
    )


def end_frames(workload):
    value = script(
        "const p=window.dfFrames;p.active=false;return {frames:p.count,mean_frame_gap_ms:p.total/Math.max(1,p.count),max_frame_gap_ms:p.max,duration_ms:performance.now()-p.started}"
    )
    value.update(
        {
            "workload": workload,
            "build": "debug_e2e",
            "scope": "native frontend frame intervals; not input-to-paint latency",
        }
    )
    performance.append(value)


def invoke(name, params=None):
    result = call(
        "POST",
        "/execute/async",
        {
            "script": "const done=arguments[arguments.length-1];window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(done,e=>done({error:e}));",
            "args": [name, params or {}],
        },
    )
    if isinstance(result, dict) and "error" in result:
        raise RuntimeError(str(result))
    return result


def wait(fn, seconds=120):
    until = time.monotonic() + seconds
    last = None
    while time.monotonic() < until:
        try:
            value = fn()
            if value:
                return value
        except Exception as e:
            last = e
        time.sleep(0.2)
    raise AssertionError(f"Condition timed out: {last}")


def element(css):
    return call("POST", "/element", {"using": "css selector", "value": css})[
        "element-6066-11e4-a52e-4f735466cecf"
    ]


def click(css):
    ident = wait(lambda: element(css))
    wait(
        lambda: script(
            "const e=document.querySelector(arguments[0]);return e&&!e.matches(':disabled')",
            css,
        )
    )
    wait(lambda: call("GET", f"/element/{ident}/enabled"))
    call("POST", f"/element/{ident}/click", {})


def fill(css, text):
    ident = element(css)
    call("POST", f"/element/{ident}/clear", {})
    call("POST", f"/element/{ident}/value", {"text": text})


def select(css, value):
    wait(lambda: script("return !!document.querySelector(arguments[0])", css))
    script(
        "const el=document.querySelector(arguments[0]);el.value=arguments[1];el.dispatchEvent(new Event('change',{bubbles:true}));",
        css,
        value,
    )


def snap():
    return invoke("workspace_snapshot")


def settled(seconds=120):
    return wait(lambda: not snap()["busy"], seconds)


def start_project():
    started = time.monotonic()
    debugger_button("Start project")
    settled(300)
    timing = {
        "operation": "start_project",
        "elapsed_seconds": time.monotonic() - started,
        "build": "debug_e2e",
        "scope": "includes verified SDK hashing and fixture services; not release latency",
    }
    operation_timings.append(timing)
    print("M7_XDEBUG_START_TIMING:" + json.dumps(timing), flush=True)


def row(id):
    return next(r for r in snap()["snapshot"]["projects"] if r["project"]["id"] == id)


def project(id):
    click(f'a.project-row[href$="{id}"]')


def button(label, scope=".project-detail"):
    wait(
        lambda: script(
            "return [...document.querySelectorAll(arguments[0]+' button')].some(b=>(b.textContent.trim()===arguments[1]||b.getAttribute('aria-label')===arguments[1])&&!b.disabled)",
            scope,
            label,
        )
    )
    css = script(
        "const nodes=[...document.querySelectorAll(arguments[0]+' button')];const index=nodes.findIndex(b=>b.textContent.trim()===arguments[1]||b.getAttribute('aria-label')===arguments[1]);if(index<0)throw new Error('Button missing '+arguments[1]);nodes[index].dataset.smoke='target';return '[data-smoke=target]';",
        scope,
        label,
    )
    click(css)
    script(
        "document.querySelector('[data-smoke=target]')?.removeAttribute('data-smoke');"
    )


def served(url):
    from urllib.parse import urlparse

    u = urlparse(url)
    conn = http.client.HTTPConnection("127.0.0.1", u.port or 80, timeout=4)
    conn.request("GET", u.path or "/", headers={"Host": u.netloc})
    r = conn.getresponse()
    data = r.read()
    conn.close()
    assert r.status == 200, (r.status, data[:200])
    return json.loads(data)


def content_viewport(width, height):
    for _ in range(3):
        metrics = script("return {width:innerWidth,height:innerHeight}")
        rect = call("GET", "/window/rect")
        if metrics == {"width": width, "height": height}:
            viewport_attempts.append(
                {
                    "requested": {"width": width, "height": height},
                    "actual": metrics,
                    "matched": True,
                }
            )
            return
        call(
            "POST",
            "/window/rect",
            {
                "width": rect["width"] + width - metrics["width"],
                "height": rect["height"] + height - metrics["height"],
            },
        )
        call(
            "POST",
            "/execute/async",
            {
                "script": "const callback=arguments[arguments.length-1];let finished=false;const done=source=>{if(finished)return;finished=true;callback({source,visibility:document.visibilityState,focused:document.hasFocus()})};setTimeout(()=>done('timer'),700);requestAnimationFrame(()=>requestAnimationFrame(()=>done('animation-frame')));",
                "args": [],
            },
        )
    actual = script("return {width:innerWidth,height:innerHeight}")
    viewport_attempts.append(
        {
            "requested": {"width": width, "height": height},
            "actual": actual,
            "matched": False,
        }
    )
    print("M7_XDEBUG_VIEWPORT_LIMIT:" + json.dumps(viewport_attempts[-1]), flush=True)
    assert actual["width"] >= 820 and actual["height"] >= 588
    assert script("return document.documentElement.scrollWidth<=innerWidth+1")


def screenshot(name):
    call("POST", "/window/rect", {"x": 0, "y": 0})
    script(
        "window.scrollTo(0,0);for(const el of document.querySelectorAll('main,.sidebar,dialog'))el.scrollTo(0,0)"
    )
    call(
        "POST",
        "/execute/async",
        {
            "script": "const callback=arguments[arguments.length-1];let finished=false;const done=source=>{if(finished)return;finished=true;callback({source,visibility:document.visibilityState,focused:document.hasFocus()})};setTimeout(()=>done('timer'),700);requestAnimationFrame(()=>requestAnimationFrame(()=>done('animation-frame')));",
            "args": [],
        },
    )
    time.sleep(0.8)
    value = call("GET", "/screenshot")
    (args.output / name).write_bytes(base64.b64decode(value))
    captures.append(
        {
            "file": name,
            "viewport": script(
                "return {width:innerWidth,height:innerHeight,scale:devicePixelRatio,language:document.documentElement.lang,theme:document.documentElement.dataset.theme}"
            ),
        }
    )


def audit(name):
    axe = (
        pathlib.Path(__file__).resolve().parent.parent
        / "node_modules/axe-core/axe.min.js"
    )
    script(axe.read_text())
    results = call(
        "POST",
        "/execute/async",
        {
            "script": "const done=arguments[arguments.length-1];window.axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(r=>done(r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))),e=>done({error:String(e)}));",
            "args": [],
        },
    )
    accessibility.append({"case": name, "violations": results})
    assert results == [], results
    print("M7_XDEBUG_UI_PHASE: accessibility passed " + name, flush=True)


def launch():
    global process, session, port, base, opener
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    session = ""
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    print("M7_XDEBUG_UI_PHASE: launch " + str(port), flush=True)
    ready = args.fixture / "data/e2e-page-ready"
    ready.unlink(missing_ok=True)
    process = subprocess.Popen(
        [
            str(args.launcher.resolve()),
            "launch",
            str(args.application.resolve()),
            str(args.fixture.resolve()),
            str(port),
        ]
    )
    wait(lambda: ready.exists(), 60)
    wait(lambda: request("GET", "/status", timeout=3), 60)
    session = request("POST", "/session", {"capabilities": {"alwaysMatch": {}}})[
        "sessionId"
    ]
    call("POST", "/timeouts", {"script": 120000, "implicit": 0})
    wait(lambda: script("return !!document.querySelector('h1')"), 60)
    settled()


def quit_app():
    try:
        click(".quit-button")
    except Exception:
        pass
    process.wait(timeout=60)
    assert process.returncode == 0, process.returncode


checks = {}


def view(id):
    return invoke("debug_read", {"request": {"kind": "view", "id": id}})


def debugger_button(label):
    button(label, ".debugger-content")


def configure(mode):
    print("M7_XDEBUG_UI_PHASE: configure " + mode, flush=True)
    debugger_button("Configure Xdebug")
    wait(lambda: script("return !!document.querySelector('dialog [name=debug-mode]')"))
    select("[name=debug-mode]", mode)
    if mode != "off":
        select("[name=debug-runtime]", native["runtime_id"])
    click("[name=debug-consent]")
    button("Save changes", "dialog")
    wait(lambda: not script("return !!document.querySelector('dialog[open]')"))
    settled()


def at_line(number):
    return view(atlas)["session"]["state"] == "paused" and any(
        f["file"] == "app/M7DebugFixture.php" and f["line"] == number
        for f in view(atlas)["session"]["frames"]
    )


try:
    native = json.loads((args.fixture / "m7-xdebug-native-result.json").read_text())
    atlas = native["project_id"]
    other = native["other_project_id"]
    launch()
    click('a[href="#settings"]')
    click("input[value=en]")
    click("input[value=light]")
    click('a[href="#debugger"]')
    settled()
    select("[name=debug-project]", atlas)
    wait(lambda: script("return !!document.querySelector('.debug-runtime-row')"))
    debugger_button("Prepare debug runtime")
    audit("runtime-install-review")
    assert script(
        "return document.querySelector('dialog .modal-actions .primary').disabled"
    )
    button("Cancel", "dialog")
    configure("debug")
    start_project()
    fill("[name=debug-request-path]", "/m7-debug")
    debugger_button("Run request")
    wait(lambda: view(atlas)["session"]["state"] == "paused")
    debugger_button("Continue")
    wait(lambda: at_line(8))
    wait(
        lambda: script(
            "return document.querySelector('.debug-code .current')?.textContent.includes('multiply')"
        )
    )
    assert script(
        "return document.querySelector('.debug-variables').textContent.includes('M7_VISIBLE_LOCAL') && document.querySelector('.debug-variables').textContent.includes('REDACTED') && !document.querySelector('.debugger-content').textContent.includes('M7_PRIVATE_SENTINEL')"
    )
    content_viewport(1536, 1024)
    screenshot("debugger-en-light.png")
    audit("paused-breakpoint-source-locals")
    checks["real_breakpoint_source_and_redacted_locals"] = True
    target=script("const li=[...document.querySelectorAll('.debug-variables li')].find(e=>e.querySelector('span')?.textContent==='label');li.querySelector('button').dataset.smoke='edit-local';return '[data-smoke=edit-local]'")
    click(target)
    fill('[name=debug-value]','M7_UI_EDITED_LOCAL')
    audit('primitive-local-edit-dialog')
    button('Apply to paused frame','dialog')
    wait(lambda: script("return !document.querySelector('dialog') && document.querySelector('.debug-variables').textContent.includes('M7_UI_EDITED_LOCAL')"))
    checks['ui_property_set_real_paused_frame']=True

    fill("[name=breakpoint-file]", "app/M7DebugFixture.php")
    fill("[name=breakpoint-line]", "13")
    debugger_button("Add")
    wait(lambda: len(view(atlas)["settings"]["breakpoints"]) == 2)
    target = wait(
        lambda: script(
            "const li=[...document.querySelectorAll('.debug-breakpoints li')].find(e=>e.querySelector('code').textContent.endsWith(':13'));if(!li)return null;li.querySelector('button').dataset.smoke='remove-added';return '[data-smoke=remove-added]'"
        )
    )
    click(target)
    wait(lambda: len(view(atlas)["settings"]["breakpoints"]) == 1)
    checks["live_breakpoint_add_remove"] = True

    debugger_button("Step into")
    wait(lambda: at_line(12))
    debugger_button("Step over")
    wait(lambda: at_line(13))
    debugger_button("Step out")
    wait(lambda: at_line(9))
    debugger_button("Continue")
    wait(lambda: view(atlas)["request"]["state"] == "completed")
    checks["step_into_over_out_and_continue"] = True
    debugger_button("Stop project")
    settled()
    configure("profile")
    start_project()
    debugger_button("Run request")
    wait(
        lambda: view(atlas)["request"]["state"] == "completed"
        and len(view(atlas)["profiles"]) > 0
    )
    debugger_button("Function profiles")
    wait(
        lambda: script(
            "return !!document.querySelector('.debug-profiles article:first-child li button')"
        )
    )
    click(".debug-profiles article:first-child li button")
    wait(
        lambda: script(
            "return !!document.querySelector('.debug-function-table tbody tr')"
        )
    )
    fill("[name=profile-search]", "M7DebugFixture")
    wait(
        lambda: script(
            "return document.querySelector('.debug-function-table tbody').textContent.includes('M7DebugFixture')"
        )
    )
    screenshot("function-profile.png")
    audit("function-profile")
    checks["actual_function_profile_filter"] = True
    debugger_button("Delete profile")
    audit("profile-delete-confirmation")
    button("Cancel", "dialog")
    assert script("return !!document.querySelector('.debug-function-table tbody tr')")
    checks["delete_review_cancel_preserves_profile"] = True
    for locale in ["ru", "uz-Latn", "en"]:
        click('a[href="#settings"]')
        click('input[value="' + locale + '"]')
        wait(
            lambda: script(
                "return document.documentElement.lang===arguments[0]", locale
            )
        )
        click('a[href="#debugger"]')
        settled()
        select("[name=debug-project]", atlas)
        content_viewport(1536, 1024)
        screenshot("debugger-" + locale + ".png")
        audit("debugger-" + locale)
        assert not script(
            "return [...document.querySelectorAll('main *')].some(e=>e.childElementCount===0 && /^(debugger|work|workspace|project|common|nav)\\.[a-z]/.test(e.textContent.trim()))"
        )
        content_viewport(820, 620)
        audit("debugger-" + locale + "-minimum")
        assert script("return document.documentElement.scrollWidth<=innerWidth+1")
    budget = (
        args.fixture
        / "data/projects/debugging"
        / atlas
        / "profiles/cachegrind.budget.out"
    )
    with budget.open("wb") as f:
        f.truncate(33 * 1024 * 1024)
    wait(lambda: row(atlas)["state"] == "failed")
    wait(
        lambda: script(
            "return [...document.querySelectorAll('.debugger-content button')].some(b=>b.textContent.trim()==='Stop project'&&!b.disabled)"
        )
    )
    audit("failed-runtime-recovery")
    debugger_button("Stop project")
    settled()
    budget.unlink(missing_ok=True)
    checks["failed_owned_runtime_can_be_stopped"] = True
    configure("off")
    select("[name=debug-project]", other)
    wait(lambda: view(other)["settings"]["mode"] == "off")
    checks["other_project_remains_off"] = True
    click('a[href="#settings"]')
    click("input[value=dark]")
    click('a[href="#debugger"]')
    content_viewport(1536, 1024)
    screenshot("debugger-dark.png")
    audit("debugger-dark")
    quit_app()
    launch()
    assert script(
        "return document.documentElement.lang==='en' && document.documentElement.dataset.theme==='dark'"
    )
    checks["preferences_relaunch"] = True
    quit_app()
    report = {
        "native": "real WKWebView/WebView2 via debug-only W3C endpoint",
        "checks": checks,
        "accessibility": accessibility,
        "captures": captures,
        "viewport_attempts": viewport_attempts,
        "paint_waits": paint_waits,
        "operation_timings": operation_timings,
        "export_native_dialog": "not automated; actual bounded export verified in backend fixture",
    }
    (args.fixture / "m7-xdebug-desktop-result.json").write_text(
        json.dumps(report, indent=2)
    )
    print("M7_XDEBUG_DESKTOP_REPORT:" + json.dumps(report), flush=True)
except Exception:
    try:
        screenshot("failure.png")
    except Exception:
        pass
    raise
finally:
    if process and process.poll() is None:
        try:
            quit_app()
        except Exception:
            process.terminate()
            process.wait(timeout=30)
