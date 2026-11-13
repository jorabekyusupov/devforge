#!/usr/bin/env python3
"""Native WKWebView/WebView2 acceptance via debug-only embedded W3C WebDriver.
No invoke mocking. Only lifecycle observation/control uses explicit native IPC.
"""
import argparse,base64,json,os,pathlib,signal,socket,subprocess,time,urllib.request,urllib.error,http.client,uuid
p=argparse.ArgumentParser();p.add_argument('fixture',type=pathlib.Path);p.add_argument('application',type=pathlib.Path);p.add_argument('launcher',type=pathlib.Path);p.add_argument('--output',type=pathlib.Path,default=pathlib.Path('output/playwright/m12'));args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
base=f'http://127.0.0.1:{port}';session='';process=None;accessibility=[];captures=[];viewport_attempts=[];performance=[]
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def request(method,path,data=None,timeout=120):
 req=urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json'},method=method)
 try:
  with opener.open(req,timeout=timeout) as response:result=json.load(response)
 except urllib.error.HTTPError as e:raise RuntimeError(e.read().decode()) from e
 value=result.get('value');
 if isinstance(value,dict) and 'error'in value:raise RuntimeError(json.dumps(value))
 return value
def call(method,path,data=None):return request(method,'/session/'+session+path,data)
def script(source,*values):return call('POST','/execute/sync',{'script':source,'args':list(values)})
def begin_frames():
 script("window.dfFrames={count:0,total:0,max:0,started:performance.now(),last:performance.now(),active:true};const tick=t=>{const p=window.dfFrames;if(!p.active)return;const gap=t-p.last;if(gap>=0){p.count++;p.total+=gap;p.max=Math.max(p.max,gap)}p.last=t;requestAnimationFrame(tick)};requestAnimationFrame(tick)")
def end_frames(workload):
 value=script("const p=window.dfFrames;p.active=false;return {frames:p.count,mean_frame_gap_ms:p.total/Math.max(1,p.count),max_frame_gap_ms:p.max,duration_ms:performance.now()-p.started}");value.update({'workload':workload,'build':'debug_e2e','scope':'native frontend frame intervals; not input-to-paint latency'});performance.append(value)
def invoke(name,params=None):
 result=call('POST','/execute/async',{'script':"const done=arguments[arguments.length-1];window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(done,e=>done({error:e}));",'args':[name,params or {}]})
 if isinstance(result,dict) and 'error'in result:raise RuntimeError(str(result))
 return result
def wait(fn,seconds=120):
 until=time.monotonic()+seconds;last=None
 while time.monotonic()<until:
  try:
   value=fn()
   if value:return value
  except Exception as e:last=e
  time.sleep(.2)
 raise AssertionError(f'Condition timed out: {last}')
def element(css):return call('POST','/element',{'using':'css selector','value':css})['element-6066-11e4-a52e-4f735466cecf']
def click(css):
 ident=wait(lambda:element(css));wait(lambda:script("const e=document.querySelector(arguments[0]);return e&&!e.matches(':disabled')",css));wait(lambda:call('GET',f'/element/{ident}/enabled'));call('POST',f'/element/{ident}/click',{})
def fill(css,text):
 ident=element(css);call('POST',f'/element/{ident}/clear',{});call('POST',f'/element/{ident}/value',{'text':text})
def select(css,value):
 wait(lambda:script("return !!document.querySelector(arguments[0])",css));script("const el=document.querySelector(arguments[0]);el.value=arguments[1];el.dispatchEvent(new Event('change',{bubbles:true}));",css,value)
def snap():return invoke('workspace_snapshot')
def settled():return wait(lambda:not snap()['busy'])
def row(id):return next(r for r in snap()['snapshot']['projects'] if r['project']['id']==id)
def project(id):click(f'a.project-row[href$="{id}"]')
def button(label,scope='.project-detail'):
 wait(lambda:script("return [...document.querySelectorAll(arguments[0]+' button')].some(b=>(b.textContent.trim()===arguments[1]||b.getAttribute('aria-label')===arguments[1])&&!b.disabled)",scope,label))
 css=script("const nodes=[...document.querySelectorAll(arguments[0]+' button')];const index=nodes.findIndex(b=>b.textContent.trim()===arguments[1]||b.getAttribute('aria-label')===arguments[1]);if(index<0)throw new Error('Button missing '+arguments[1]);nodes[index].dataset.smoke='target';return '[data-smoke=target]';",scope,label);click(css);script("document.querySelector('[data-smoke=target]')?.removeAttribute('data-smoke');")
def served(url):
 from urllib.parse import urlparse
 u=urlparse(url);conn=http.client.HTTPConnection('127.0.0.1',u.port or 80,timeout=4);conn.request('GET',u.path or '/',headers={'Host':u.netloc});r=conn.getresponse();data=r.read();conn.close();assert r.status==200,(r.status,data[:200]);return json.loads(data)
def content_viewport(width,height):
 for _ in range(3):
  metrics=script('return {width:innerWidth,height:innerHeight}');rect=call('GET','/window/rect')
  if metrics=={'width':width,'height':height}:
   viewport_attempts.append({'requested':{'width':width,'height':height},'actual':metrics,'matched':True});return
  call('POST','/window/rect',{'width':rect['width']+width-metrics['width'],'height':rect['height']+height-metrics['height']})
  call('POST','/execute/async',{'script':'const done=arguments[arguments.length-1];requestAnimationFrame(()=>requestAnimationFrame(()=>done(true)));','args':[]})
 actual=script('return {width:innerWidth,height:innerHeight}');viewport_attempts.append({'requested':{'width':width,'height':height},'actual':actual,'matched':False});print('M12_VIEWPORT_LIMIT:'+json.dumps(viewport_attempts[-1]),flush=True);assert actual['width']>=820 and actual['height']>=588;assert script('return document.documentElement.scrollWidth<=innerWidth+1')
def screenshot(name):
 call('POST','/window/rect',{'x':0,'y':0})
 script("window.scrollTo(0,0);for(const el of document.querySelectorAll('main,.sidebar,dialog'))el.scrollTo(0,0)");call('POST','/execute/async',{'script':'const done=arguments[arguments.length-1];requestAnimationFrame(()=>requestAnimationFrame(()=>done(true)));','args':[]});time.sleep(.8)
 value=call('GET','/screenshot');(args.output/name).write_bytes(base64.b64decode(value));captures.append({'file':name,'viewport':script('return {width:innerWidth,height:innerHeight,scale:devicePixelRatio,language:document.documentElement.lang,theme:document.documentElement.dataset.theme}')})
def audit(name):
 axe=pathlib.Path(__file__).resolve().parent.parent/'node_modules/axe-core/axe.min.js'
 script(axe.read_text())
 results=call('POST','/execute/async',{'script':"const done=arguments[arguments.length-1];window.axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(r=>done(r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))),e=>done({error:String(e)}));",'args':[]})
 accessibility.append({'case':name,'violations':results});assert results==[],results
 print('M12_UI_PHASE: accessibility passed '+name,flush=True)
def launch():
 global process,session,port,base,opener
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';session='';opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 print('M12_UI_PHASE: launch '+str(port),flush=True)
 ready=args.fixture/'data/e2e-page-ready';ready.unlink(missing_ok=True)
 process=subprocess.Popen([str(args.launcher.resolve()),'launch',str(args.application.resolve()),str(args.fixture.resolve()),str(port)])
 wait(lambda:ready.exists(),60);wait(lambda:request('GET','/status',timeout=3),60);session=request('POST','/session',{'capabilities':{'alwaysMatch':{}}})['sessionId'];call('POST','/timeouts',{'script':120000,'implicit':0});wait(lambda:script("return !!document.querySelector('h1')"),60);settled()
def quit_app():
 try:click('.quit-button')
 except Exception:pass
 process.wait(timeout=60);assert process.returncode==0,process.returncode


checks={}
try:
    native=json.loads((args.fixture/'m12-native-result.json').read_text())
    branch_name='feature/m12-ui-'+uuid.uuid4().hex[:8]
    launch()
    click('a[href="#settings"]');click('input[value=en]');click('input[value=light]')
    wait(lambda:script("return document.documentElement.lang==='en'"))
    click('a[href="#workspaces"]');settled();content_viewport(1536,1024)
    wait(lambda:script("return !!document.querySelector('.fork-detail')"))
    screenshot('workspaces-en-light.png');audit('workspace-list')
    button('New workspace','.worktrees-page')
    wait(lambda:script("return !!document.querySelector('dialog[open]')"))
    select('dialog [name=source_id]',native['ui_create_source_id'])
    fill('dialog [name=name]','Native UI workspace')
    fill('dialog [name=branch]',branch_name)
    if script("return document.querySelector('dialog [name=dependencies]').checked"):
        click('dialog [name=dependencies]')
    audit('create-form');screenshot('workspace-create.png')
    button('Review workspace','dialog')
    wait(lambda:script("return !!document.querySelector('dialog [name=trusted]')"))
    assert script("return document.querySelector('dialog .modal-actions button.primary').disabled")
    audit('create-review')
    click('dialog [name=trusted]');button('Create workspace','dialog')
    wait(lambda:not script("return !!document.querySelector('dialog[open]')"),240)
    settled()
    state=invoke('workspace_snapshot')['snapshot']
    created=next(v for v in state['worktrees'] if v['record']['input']['branch']==branch_name)
    assert created['record']['stage']=='ready'
    project=next(p for p in state['projects'] if p['project']['id']==created['record']['project_id'])
    source=next(p for p in state['projects'] if p['project']['id']==native['ui_create_source_id'])
    source_env_file=pathlib.Path(source['project']['path']).joinpath('.env')
    source_env=source_env_file.read_bytes() if source_env_file.exists() else None
    child_env=pathlib.Path(created['path']).joinpath('.env').read_bytes()
    assert child_env!=source_env
    checks['real_ui_creation_and_independent_project']=True
    audit('created-workspace')
    button('Review cleanup','.fork-detail')
    wait(lambda:script("return !!document.querySelector('dialog [name=consent]')"))
    assert script("return document.querySelector('dialog .modal-actions button.danger').disabled")
    audit('cleanup-review');screenshot('workspace-cleanup-review.png')
    button('Cancel','dialog');assert pathlib.Path(created['path']).is_dir()
    checks['cleanup_review_cancel_preserves_worktree']=True
    user_file=pathlib.Path(created['path'])/'untracked-native-user.txt';user_file.write_text('USER_WORK_RETAIN')
    button('Review cleanup','.fork-detail')
    wait(lambda:script("return !!document.querySelector('dialog .notice.error')"))
    assert script("return document.querySelector('dialog .modal-actions button.danger').disabled")
    audit('dirty-cleanup-refusal');button('Cancel','dialog');assert user_file.read_text()=='USER_WORK_RETAIN';user_file.unlink()
    checks['dirty_worktree_cleanup_refused']=True
    button('Review cleanup','.fork-detail')
    wait(lambda:script("return !!document.querySelector('dialog [name=consent]')"))
    click('dialog [name=consent]');button('Archive workspace','dialog')
    wait(lambda:not script("return !!document.querySelector('dialog[open]')"),120);settled()
    assert not pathlib.Path(created['path']).exists()
    assert pathlib.Path(created['retained_path']).joinpath('.env').read_bytes()==child_env
    state=invoke('workspace_snapshot')['snapshot']
    assert not any(p['project']['id']==created['record']['project_id'] for p in state['projects'])
    assert next(v for v in state['worktrees'] if v['record']['id']==created['record']['id'])['record']['stage']=='removed'
    checks['real_archive_and_project_retirement']=True
    audit('archived-workspace');screenshot('workspace-archived.png')
    for locale in ['ru','uz-Latn','en']:
        click('a[href="#settings"]');click('input[value="'+locale+'"]')
        wait(lambda:script("return document.documentElement.lang===arguments[0]",locale))
        click('a[href="#workspaces"]');settled();content_viewport(1536,1024)
        screenshot('workspaces-'+locale+'.png');audit('workspaces-'+locale)
        assert not script("return [...document.querySelectorAll('main *')].some(e=>e.childElementCount===0 && /^(forks|workspaces|work|workspace|project|common|nav)\.[a-z]/.test(e.textContent.trim()))")
        content_viewport(820,620);audit('workspaces-'+locale+'-minimum')
        assert script("return document.documentElement.scrollWidth<=innerWidth+1")
    click('a[href="#settings"]');click('input[value=dark]');click('a[href="#workspaces"]')
    content_viewport(1536,1024);screenshot('workspaces-dark.png');audit('workspaces-dark')
    quit_app();launch()
    assert script("return document.documentElement.lang==='en' && document.documentElement.dataset.theme==='dark'")
    click('a[href="#workspaces"]');settled();audit('preferences-relaunch')
    checks['languages_appearance_and_relaunch']=True
    quit_app()
    assert (source_env_file.read_bytes() if source_env_file.exists() else None)==source_env
    checks['source_environment_preserved']=True
    report={'native':'real WKWebView/WebView2 via debug-only W3C endpoint','checks':checks,'accessibility':accessibility,'captures':captures,'viewport_attempts':viewport_attempts,'external_folder_launch':'not automated; ownership URL boundary inspected'}
    (args.fixture/'m12-desktop-result.json').write_text(json.dumps(report,indent=2))
    print('M12_DESKTOP_REPORT:'+json.dumps(report),flush=True)
except Exception:
    try:screenshot('failure.png')
    except Exception:pass
    raise
finally:
    if process and process.poll() is None:
        try:quit_app()
        except Exception:process.terminate();process.wait(timeout=30)
