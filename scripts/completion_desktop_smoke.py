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
    "--output", type=pathlib.Path, default=pathlib.Path("output/playwright/completion")
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
    native=json.loads((args.fixture/'completion-native-result.json').read_text())
    target=native['ui_project']; workspace=native['ui_workspace']
    launch()
    click('a[href="#settings"]');click('input[value=en]');click('input[value=light]')
    click('a[href="#configuration"]');settled()
    select('[name=configuration-project]',target)
    wait(lambda:script("return !!document.querySelector('[name=configuration-yaml]')"))
    current=script("return document.querySelector('[name=configuration-yaml]').value")
    fill('[name=configuration-yaml]',current+'\n# Native GUI edited\n')
    button('Save configuration','.configuration-content')
    audit('configuration-save-review')
    assert script("return document.querySelector('dialog .primary').disabled")
    click('dialog input[type=checkbox]');button('Save configuration','dialog')
    wait(lambda:script("return !document.querySelector('dialog')"));settled()
    button('Review saved configuration','.configuration-content');settled()
    wait(lambda:script("return [...document.querySelectorAll('.configuration-review button')].some(b=>b.textContent==='Apply reviewed plan')"))
    script("document.querySelector('[name=configuration-yaml]').scrollTop=0")
    screenshot('configuration-en-light.png');audit('configuration-editor-review')
    checks['native_yaml_edit_review_save_and_plan']=True
    # Store a real local secret using the GUI; the value is absent from snapshots/reports.
    script("const d=[...document.querySelectorAll('.configuration-review details')][0];d.open=true")
    fill('[name=secret-reference]','ui-completion-secret');fill('[name=secret-value]','COMPLETION_UI_PRIVATE_SENTINEL')
    button('Store locally','.configuration-review');settled()
    wait(lambda:script("return document.querySelector('[name=secret-value]').value===''") )
    assert 'COMPLETION_UI_PRIVATE_SENTINEL' not in json.dumps(snap())
    checks['native_secret_store_value_cleared_and_not_in_snapshot']=True
    for locale in ['ru','uz-Latn','en']:
        click('a[href="#settings"]');click('input[value='+locale+']');wait(lambda:script('return document.documentElement.lang')==locale)
        click('a[href="#configuration"]');settled();content_viewport(820,620);audit('configuration-'+locale+'-minimum');screenshot('configuration-'+locale+'-minimum.png')
    click('a[href="#settings"]');click('input[value=dark]');click('a[href="#configuration"]');content_viewport(1536,1024);settled();audit('configuration-dark');screenshot('configuration-dark.png')
    # Saving portable YAML is an intentional repository edit. Commit fixture edits before archive.
    path=row(target)['project']['path']
    subprocess.run(['git','-C',path,'add','devforge.yaml'],check=True)
    subprocess.run(['git','-C',path,'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','-m','Native configuration editor fixture'],check=True)
    # Return to the exact managed workspace and inspect archive and remote-copy controls.
    click('a[href="#workspaces"]');settled();click('[data-workspace="'+workspace+'"]')
    button('Copy external database','.fork-detail');audit('external-copy-form');screenshot('external-copy-form.png');button('Cancel','dialog') if script("return [...document.querySelectorAll('dialog button')].some(b=>b.textContent==='Cancel')") else click('dialog .modal-title button')
    checks['native_external_database_review_form']=True
    button('Review cleanup','.fork-detail')
    click('dialog input[name=consent]');button('Archive workspace','dialog');settled()
    wait(lambda:next(v for v in snap()['snapshot']['worktrees'] if v['record']['id']==workspace)['record']['stage']=='removed')
    button('Reactivate archive','.fork-detail');audit('archive-reactivation-review');click('dialog input[name=archive-consent]');button('Reactivate archive','dialog');settled()
    wait(lambda:next(v for v in snap()['snapshot']['worktrees'] if v['record']['id']==workspace)['record']['stage']=='ready')
    checks['native_archive_restore_same_project']=True
    button('Review cleanup','.fork-detail');click('dialog input[name=consent]');button('Archive workspace','dialog');settled()
    wait(lambda:next(v for v in snap()['snapshot']['worktrees'] if v['record']['id']==workspace)['record']['stage']=='removed')
    button('Delete archived files','.fork-detail');audit('archive-purge-review')
    assert script("return document.querySelector('dialog .modal-actions .danger').disabled")
    click('dialog input[name=archive-consent]');button('Delete archived files','dialog');settled()
    wait(lambda:next(v for v in snap()['snapshot']['worktrees'] if v['record']['id']==workspace)['record']['stage']=='purged')
    assert script("return [...document.querySelectorAll('.fork-detail button')].find(b=>b.textContent.includes('Open folder')).disabled")
    checks['native_purge_explicit_review_files_only']=True

    quit_app()
    report={'native':'real WKWebView/WebView2','checks':checks,'accessibility':accessibility,'captures':captures,'viewport_attempts':viewport_attempts}
    (args.fixture/'completion-desktop-result.json').write_text(json.dumps(report,indent=2));print('COMPLETION_UI_REPORT:'+json.dumps(report),flush=True)
except Exception:
    try:screenshot('failure.png')
    except Exception:pass
    raise
finally:
    if process and process.poll() is None:
        try:quit_app()
        except Exception:process.terminate();process.wait(timeout=30)
