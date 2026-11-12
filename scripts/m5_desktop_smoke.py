#!/usr/bin/env python3
"""Native WKWebView/WebView2 acceptance via debug-only embedded W3C WebDriver.
No invoke mocking. Only lifecycle observation/control uses explicit native IPC.
"""
import argparse,base64,json,os,pathlib,signal,socket,subprocess,time,urllib.request,urllib.error,http.client
p=argparse.ArgumentParser();p.add_argument('fixture',type=pathlib.Path);p.add_argument('application',type=pathlib.Path);p.add_argument('launcher',type=pathlib.Path);p.add_argument('--output',type=pathlib.Path,default=pathlib.Path('output/playwright/m5'));args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
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
 ident=wait(lambda:element(css));wait(lambda:call('GET',f'/element/{ident}/enabled'));call('POST',f'/element/{ident}/click',{})
def fill(css,text):
 ident=element(css);call('POST',f'/element/{ident}/clear',{});call('POST',f'/element/{ident}/value',{'text':text})
def select(css,value):script("const el=document.querySelector(arguments[0]);el.value=arguments[1];el.dispatchEvent(new Event('change',{bubbles:true}));",css,value)
def snap():return invoke('workspace_snapshot')
def settled():return wait(lambda:not snap()['busy'])
def row(id):return next(r for r in snap()['snapshot']['projects'] if r['project']['id']==id)
def project(id):click(f'a.project-row[href$="{id}"]')
def button(label,scope='.project-detail'):
 wait(lambda:script("return [...document.querySelectorAll(arguments[0]+' button')].some(b=>b.textContent.trim()===arguments[1]&&!b.disabled)",scope,label))
 css=script("const nodes=[...document.querySelectorAll(arguments[0]+' button')];const index=nodes.findIndex(b=>b.textContent.trim()===arguments[1]);if(index<0)throw new Error('Button missing '+arguments[1]);nodes[index].dataset.smoke='target';return '[data-smoke=target]';",scope,label);click(css);script("document.querySelector('[data-smoke=target]')?.removeAttribute('data-smoke');")
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
 actual=script('return {width:innerWidth,height:innerHeight}');viewport_attempts.append({'requested':{'width':width,'height':height},'actual':actual,'matched':False});print('M5_VIEWPORT_LIMIT:'+json.dumps(viewport_attempts[-1]),flush=True);assert actual['width']>=820 and actual['height']>=588;assert script('return document.documentElement.scrollWidth<=innerWidth+1')
def screenshot(name):
 value=call('GET','/screenshot');(args.output/name).write_bytes(base64.b64decode(value));captures.append({'file':name,'viewport':script('return {width:innerWidth,height:innerHeight,scale:devicePixelRatio,language:document.documentElement.lang,theme:document.documentElement.dataset.theme}')})
def audit(name):
 axe=pathlib.Path(__file__).resolve().parent.parent/'node_modules/axe-core/axe.min.js'
 script(axe.read_text())
 results=call('POST','/execute/async',{'script':"const done=arguments[arguments.length-1];window.axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(r=>done(r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))),e=>done({error:String(e)}));",'args':[]})
 accessibility.append({'case':name,'violations':results});assert results==[],results
def launch():
 global process,session,port,base,opener
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';session='';opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 print('M5_UI_PHASE: launch '+str(port),flush=True)
 ready=args.fixture/'data/e2e-page-ready';ready.unlink(missing_ok=True)
 process=subprocess.Popen([str(args.launcher.resolve()),'launch',str(args.application.resolve()),str(args.fixture.resolve()),str(port)])
 wait(lambda:ready.exists(),60);wait(lambda:request('GET','/status',timeout=3),60);session=request('POST','/session',{'capabilities':{'alwaysMatch':{}}})['sessionId'];call('POST','/timeouts',{'script':120000,'implicit':0});wait(lambda:script("return !!document.querySelector('h1')"),60);settled()
def quit_app():
 try:click('.quit-button')
 except Exception:pass
 process.wait(timeout=60);assert process.returncode==0,process.returncode
try:
 launch();click('a[href="#settings"]');wait(lambda:script("return !!document.querySelector('input[type=radio][value=en]')"));click('input[type=radio][value=en]');wait(lambda:script("return document.documentElement.lang==='en'"));click('input[type=radio][value=light]');wait(lambda:script("return document.documentElement.dataset.theme==='light'"));click('a[href="#projects"]');settled();wait(lambda:len(snap()['snapshot']['projects'])>=2);initial=snap()['snapshot'];a=next(r for r in initial['projects'] if r['project']['name']=='Atlas')['project']['id'];b=next(r for r in initial['projects'] if r['project']['name']=='Studio')['project']['id']
 # A prior partial run may have left this test's own Notes registration.
 existing=next((r for r in initial['projects'] if r['project']['name']=='Notes'),None)
 if existing:
  project(existing['project']['id']);click('details.project-meta summary');button('Forget project');wait(lambda:script("return document.querySelector('dialog')?.open"));click('dialog button.danger');wait(lambda:not script("return !!document.querySelector('dialog')"));settled()
 # Actual project import and inline error recovery through the dialog.
 click('.workspace-toolbar button.primary');wait(lambda:script("return document.querySelector('dialog')?.open"));assert script("return document.activeElement?.name==='path'");fill('dialog input[name=path]','/missing/devforge-m5');fill('dialog input[name=name]','Notes');fill('dialog input[name=domain]','notes.devforge.test');click('dialog input[type=checkbox]');click('dialog button[type=submit]');wait(lambda:script("return !!document.querySelector('dialog [role=alert]')"));assert script("return document.querySelector('dialog input[name=name]').value==='Notes'");audit('import-error-dialog')
 notes=args.fixture/'Notes fixture';notes.mkdir(exist_ok=True);(notes/'public').mkdir(exist_ok=True);(notes/'composer.json').write_text('{"require":{"php":"^8.3","laravel/framework":"^13"}}');(notes/'public/index.php').write_text("<?php header('Content-Type: application/json');echo json_encode(['php'=>PHP_VERSION]);")
 fill('dialog input[name=path]',str(notes.resolve()));click('dialog button[type=submit]');wait(lambda:not script("return !!document.querySelector('dialog')"));settled();assert len(snap()['snapshot']['projects'])==3
 print('M5_UI_PHASE: import passed',flush=True)
 # Install/remove through the actual Runtimes view, then assign a newly created DB to Notes.
 click('a[href="#runtimes"]');fill('.workspace-toolbar input[type=search]','composer');wait(lambda:script("return document.querySelectorAll('.runtime-table tbody tr').length===1"));click('.runtime-table tbody button');wait(lambda:script("return document.querySelector('dialog')?.open"));click('dialog button.danger');wait(lambda:not script("return !!document.querySelector('dialog')"));settled();wait(lambda:script("return document.querySelector('.runtime-table tbody button')?.textContent.trim()==='Install'"));begin_frames();click('.runtime-table tbody button');settled();wait(lambda:all(r['installed'] for r in snap()['snapshot']['runtimes'] if r['kind']=='composer'));end_frames('real_composer_download_verify_install')
 screenshot('runtimes-light.png');audit('runtimes-light')
 click('a[href="#services"]');before_databases={d['instance']['id'] for d in snap()['snapshot']['databases']};click('.work-panel .section-toolbar button.primary');settled();new_database=wait(lambda:next((d['instance']['id'] for d in snap()['snapshot']['databases'] if d['instance']['id'] not in before_databases),None));click('a[href="#projects"]');notes_id=next(r['project']['id'] for r in snap()['snapshot']['projects'] if r['project']['name']=='Notes');project(notes_id);select('.database-controls select',new_database);button('Assign database');settled();wait(lambda:row(notes_id)['database'] and row(notes_id)['database']['instance_id']==new_database);button('Test connection');settled()
 click('a[href="#services"]');screenshot('services-light.png');audit('services-light');click('a[href="#projects"]')
 print('M5_UI_PHASE: runtime install and DB assignment passed',flush=True)
 # Real sessions, database test and changing A while B's worker keeps its PID.
 project(a);button('Start');wait(lambda:row(a)['state']=='running');project(b);button('Start');wait(lambda:row(b)['state']=='running');b_url=row(b)['serving']['preferred_url'];before=served(b_url);b_pids=[(name,status['pid']) for name,status in row(b)['processes']];project(a);button('Test connection');settled();wait(lambda:script("return document.body.textContent.includes('Database connection succeeded.')"));php=next(r['id'] for r in snap()['snapshot']['runtimes'] if r['kind']=='php' and r['id']!=row(a)['project']['php'] and r['variant']!='fpm');select('.project-detail select[name=php]',php);button('Save changes');wait(lambda:row(a)['project']['php']==php and row(a)['state']=='running');assert [(name,status['pid']) for name,status in row(b)['processes']]==b_pids;project(a)
 print('M5_UI_PHASE: parallel sessions and version switch passed',flush=True)
 # Native close-to-tray must preserve services. Restore using the same main-window adapter.
 invoke('main_window_action',{'action':'close'});wait(lambda:invoke('main_window_action',{'action':'status'}) is False);assert row(a)['state']=='running' and [(name,status['pid']) for name,status in row(b)['processes']]==b_pids;invoke('main_window_action',{'action':'show'});wait(lambda:invoke('main_window_action',{'action':'status'}) is True)
 # Source-matched visual evidence and text expansion at the minimum supported size.
 content_viewport(1487,1058);project(a);screenshot('projects-light.png');audit('projects-light')
 for language in ['ru','uz-Latn','en']:
  click('a[href="#settings"]');click(f'input[type=radio][value="{language}"]');wait(lambda:script('return document.documentElement.lang===arguments[0]',language));click('a[href="#projects"]');project(a);call('POST','/window/rect',{'width':820,'height':620});assert script('return document.documentElement.scrollWidth<=innerWidth+1');screenshot(f'projects-small-{language}.png');audit(f'projects-small-{language}')
 click('a[href="#settings"]');click('input[type=radio][value=dark]');wait(lambda:script("return document.documentElement.dataset.theme==='dark'"));click('a[href="#projects"]');project(a);screenshot('projects-dark.png');audit('projects-dark');button('Stop');wait(lambda:row(a)['state']=='stopped');assert [(name,status['pid']) for name,status in row(b)['processes']]==b_pids;assert next(d for d in snap()['snapshot']['databases'] if d['instance']['id']==row(b)['database']['instance_id'])['state']=='ready';assert any(l['process']=='queue' for l in snap()['snapshot']['logs'])
 # Log filters are owned text controls, not translated raw output.
 click('a[href="#services"]');fill('.section-toolbar input[type=search]','DEVFORGE_QUEUE_READY');wait(lambda:script("return document.querySelector('.log-viewer').textContent.includes('DEVFORGE_QUEUE_READY')"));assert script("return !document.querySelector('.log-viewer').textContent.includes('0123456789abcdef0123456789abcdef')")
 # Concurrent real Laravel HTTP load while the bounded native log viewer refreshes.
 from concurrent.futures import ThreadPoolExecutor
 begin_frames()
 with ThreadPoolExecutor(max_workers=4) as pool:
  list(pool.map(lambda _:served(b_url),range(100)))
 time.sleep(3);end_frames('100_laravel_requests_with_services_log_viewer')
 screenshot('services-dark.png');audit('services-dark')
 running_ports=[d['instance']['port'] for d in snap()['snapshot']['databases']]
 from urllib.parse import urlparse
 running_ports.append(urlparse(b_url).port or 80)
 def ports_closed(values):
  for p in values:
   with socket.socket() as sock:
    sock.settimeout(.1)
    if sock.connect_ex(('127.0.0.1',p))==0:return False
  return True
 quit_app();wait(lambda:ports_closed(running_ports),15)
 print('M5_UI_PHASE: initial UI/lifecycle passed',flush=True)
 # Reopen keeps registrations and locale/theme, but does not auto-start projects.
 launch();wait(lambda:script("return document.documentElement.dataset.theme==='dark'"));assert len(snap()['snapshot']['projects'])==3;assert all(r['state']=='stopped' for r in snap()['snapshot']['projects']);project(a);button('Start');wait(lambda:row(a)['state']=='running');ports=[d['instance']['port'] for d in snap()['snapshot']['databases']];url=row(a)['serving']['preferred_url'];app_pid=snap()['host_pid'];os.kill(app_pid,signal.SIGTERM if os.name=='nt' else signal.SIGKILL);process.wait(timeout=30)
 from urllib.parse import urlparse
 ports.append(urlparse(url).port or 80)
 def closed():
  for p in ports:
   with socket.socket() as s:
    s.settimeout(.1)
    if s.connect_ex(('127.0.0.1',p))==0:return False
  return True
 wait(closed,15);launch();project(a);button('Start');wait(lambda:row(a)['state']=='running');assert row(a)['project']['php']==php;quit_app()
 result={'performance':performance,'host':os.name,'accessibility':accessibility,'captures':captures,'viewport_attempts':viewport_attempts,'real_native_webview':True,'ipc_mocked':False,'secret_store':'private filesystem E2E adapter; production uses OS credentials','import_error_recovery':True,'parallel_projects_and_database':True,'runtime_remove_install_via_ui':True,'database_create_assign_via_ui':True,'version_switch_preserves_B_pid':True,'close_to_tray_preserves_services':True,'ru_uz_en_and_dark_persistence':True,'minimum_window_no_horizontal_overflow':True,'logs_filtering_redacted':True,'quit_stops_owned_services':True,'forced_app_crash_cleanup_and_recovery':True,'passed':True};(args.fixture/'m5-desktop-result.json').write_text(json.dumps(result,indent=2));print('M5_DESKTOP_REPORT:'+json.dumps(result))
except Exception:
 try:
  screenshot('failure.png');(args.output/'failure.html').write_text(call('GET','/source'))
 except Exception:pass
 raise
finally:
 if process and process.poll() is None:
  try:invoke('quit_application');process.wait(timeout=60)
  except Exception:process.kill();process.wait()
