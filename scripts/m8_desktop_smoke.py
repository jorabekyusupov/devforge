#!/usr/bin/env python3
"""Native WKWebView/WebView2 acceptance via debug-only embedded W3C WebDriver.
No invoke mocking. Only lifecycle observation/control uses explicit native IPC.
"""
import argparse,base64,json,os,pathlib,signal,socket,subprocess,time,urllib.request,urllib.error,http.client
p=argparse.ArgumentParser();p.add_argument('fixture',type=pathlib.Path);p.add_argument('application',type=pathlib.Path);p.add_argument('launcher',type=pathlib.Path);p.add_argument('--output',type=pathlib.Path,default=pathlib.Path('output/playwright/m8'));args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
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
 actual=script('return {width:innerWidth,height:innerHeight}');viewport_attempts.append({'requested':{'width':width,'height':height},'actual':actual,'matched':False});print('M8_VIEWPORT_LIMIT:'+json.dumps(viewport_attempts[-1]),flush=True);assert actual['width']>=820 and actual['height']>=588;assert script('return document.documentElement.scrollWidth<=innerWidth+1')
def screenshot(name):
 call('POST','/window/rect',{'x':0,'y':0})
 script("window.scrollTo(0,0);for(const el of document.querySelectorAll('main,.sidebar,dialog'))el.scrollTo(0,0)");call('POST','/execute/async',{'script':'const done=arguments[arguments.length-1];requestAnimationFrame(()=>requestAnimationFrame(()=>done(true)));','args':[]});time.sleep(.8)
 value=call('GET','/screenshot');(args.output/name).write_bytes(base64.b64decode(value));captures.append({'file':name,'viewport':script('return {width:innerWidth,height:innerHeight,scale:devicePixelRatio,language:document.documentElement.lang,theme:document.documentElement.dataset.theme}')})
def audit(name):
 axe=pathlib.Path(__file__).resolve().parent.parent/'node_modules/axe-core/axe.min.js'
 script(axe.read_text())
 results=call('POST','/execute/async',{'script':"const done=arguments[arguments.length-1];window.axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(r=>done(r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))),e=>done({error:String(e)}));",'args':[]})
 accessibility.append({'case':name,'violations':results});assert results==[],results
 print('M8_UI_PHASE: accessibility passed '+name,flush=True)
def launch():
 global process,session,port,base,opener
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';session='';opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 print('M8_UI_PHASE: launch '+str(port),flush=True)
 ready=args.fixture/'data/e2e-page-ready';ready.unlink(missing_ok=True)
 process=subprocess.Popen([str(args.launcher.resolve()),'launch',str(args.application.resolve()),str(args.fixture.resolve()),str(port)])
 wait(lambda:ready.exists(),60);wait(lambda:request('GET','/status',timeout=3),60);session=request('POST','/session',{'capabilities':{'alwaysMatch':{}}})['sessionId'];call('POST','/timeouts',{'script':120000,'implicit':0});wait(lambda:script("return !!document.querySelector('h1')"),60);settled()
def quit_app():
 try:click('.quit-button')
 except Exception:pass
 process.wait(timeout=60);assert process.returncode==0,process.returncode
# Every project and Git source below belongs to the isolated native fixture.
import hashlib
native=json.loads((args.fixture/'m8-native-result.json').read_text())
relative=pathlib.PurePath(native['source_relative']);assert not relative.is_absolute() and '..' not in relative.parts
source=str((args.fixture/relative).resolve());parent=args.fixture/'UI workspace';parent.mkdir(exist_ok=True)
checks={}
def rows():return snap()['snapshot']['projects']
def named(name):return next(r for r in rows() if r['project']['name']==name)
def field(name):return f'dialog [name="{name}"]'
def choose_source(kind):click(field('setup-source')+f'[value="{kind}"]')
def wizard_start(kind='laravel',name='M8 Preview'):
 click('a[href="#projects"]');settled();button('New project','.workspace-toolbar');wait(lambda:script("return !!document.querySelector('dialog[open]')"));choose_source(kind)
 if kind=='git':fill(field('repository'),source);fill(field('branch'),'main')
 button('Continue','dialog');wait(lambda:script("return !!document.querySelector('[name=setup-name]')"));fill(field('setup-name'),name);fill(field('setup-parent'),str(parent.resolve()))
 fresh=named('M8 Fresh')['project'];select(field('setup-php'),fresh['php']);select(field('setup-node'),fresh['node'])
def review_submit():
 button('Continue','dialog');wait(lambda:script("return !!document.querySelector('[name=trusted]')"));assert script("return [...document.querySelectorAll('dialog button')].find(b=>b.textContent.trim()==='Create project').disabled");click(field('trusted'));button('Create project','dialog')
def open_project(name):
 click('a[href="#projects"]');settled();project(named(name)['project']['id']);settled()
try:
 launch();click('a[href="#settings"]');wait(lambda:script("return !!document.querySelector('input[value=en]')"));click('input[value=en]');wait(lambda:script("return document.documentElement.lang==='en'"));click('input[value=light]');wait(lambda:script("return document.documentElement.dataset.theme==='light'"));click('a[href="#projects"]');settled()
 atlas=named('Atlas')['project']['id'];project(atlas);button('Start');wait(lambda:row(atlas)['state']=='running');atlas_pids=[(n,s['pid']) for n,s in row(atlas)['processes']]
 before=len(rows());wizard_start();content_viewport(1523,1032);screenshot('setup-environment-light.png');audit('wizard-environment');button('Continue','dialog');wait(lambda:script("return !!document.querySelector('[name=trusted]')"));screenshot('setup-review.png');audit('wizard-review');button('Close dialog','dialog');assert len(rows())==before;checks['review_cancel_creates_nothing']=True
 wizard_start('laravel','M8 UI Cancel');review_submit();wait(lambda:snap()['busy']);button('Cancel operation','.setup-progress');settled();wait(lambda:script("return document.querySelector('dialog [role=alert]')?.textContent.includes('cancelled')"));screenshot('setup-cancelled.png');button('Close dialog','dialog');checks['actual_ui_cancel']=True
 wizard_start('git','M8 UI Clone');assert script("return !document.querySelector('[name=run_scripts]').checked && !document.querySelector('[name=initialize_sqlite]')");screenshot('setup-git-environment.png');review_submit();wait(lambda:not script("return !!document.querySelector('dialog[open]')"),600);settled();clone=named('M8 UI Clone');clone_id=clone['project']['id'];clone_path=pathlib.Path(clone['project']['path']);assert (clone_path/'vendor/autoload.php').is_file();assert (clone_path/'.env').is_file();assert not (clone_path/'.m8-script-ran').exists();env_hash=hashlib.sha256((clone_path/'.env').read_bytes()).hexdigest();assert [(n,s['pid']) for n,s in row(atlas)['processes']]==atlas_pids;checks['real_git_clone_composer_key_and_other_project_pids']=True
 wizard_start('git','M8 UI Clone');fill(field('setup-domain'),'m8-conflict-attempt.devforge.test');review_submit();settled();wait(lambda:script("return document.querySelector('dialog [role=alert]')?.textContent.includes('destination already exists')"));assert script("return document.activeElement===document.querySelector('dialog [role=alert]')");screenshot('setup-destination-conflict.png');audit('destination-conflict');assert hashlib.sha256((clone_path/'.env').read_bytes()).hexdigest()==env_hash;button('Close dialog','dialog');checks['destination_error_preserves_files_and_review']=True
 open_project('M8 UI Clone');button('Prepare project','.setup-tools');wait(lambda:script("return !!document.querySelector('dialog[open]')"));audit('prepare-confirmation');click(field('trusted'));button('Run setup','dialog');wait(lambda:not script("return !!document.querySelector('dialog[open]')"),600);settled();assert hashlib.sha256((clone_path/'.env').read_bytes()).hexdigest()==env_hash;assert not (clone_path/'.m8-script-ran').exists();checks['prepare_existing_preserves_key_and_script_policy']=True
 favorite=script("const b=document.querySelector(arguments[0]);return b?.getAttribute('aria-pressed')",f'a[href$="{clone_id}"]+button');assert favorite=='false';click(f'a[href$="{clone_id}"]+button');settled();button('Favorites','.project-filters');wait(lambda:script("return !!document.querySelector(arguments[0])",f'.project-list a[href$="{clone_id}"]'));button('Recent','.project-filters');wait(lambda:script("return !!document.querySelector(arguments[0])",f'.project-list a[href$="{clone_id}"]'));button('All','.project-filters');checks['favorite_and_recent_controls']=True
 open_project('M8 UI Clone');button('Start');wait(lambda:row(clone_id)['state']=='running')
 # Health endpoint proves installed Laravel boots without migrating an existing Git project's database.
 from urllib.parse import urlparse
 u=urlparse(row(clone_id)['serving']['preferred_url']);connection=http.client.HTTPConnection('127.0.0.1',u.port or 80,timeout=10);connection.request('GET','/up',headers={'Host':u.netloc});response=connection.getresponse();response.read();assert response.status==200;connection.close();button('Stop');wait(lambda:row(clone_id)['state']=='stopped')
 open_project('M8 Fresh');fresh_id=named('M8 Fresh')['project']['id'];button('Start');wait(lambda:row(fresh_id)['state']=='running');fresh_pids=[(n,s['pid']) for n,s in row(fresh_id)['processes']];button('PHP settings','.setup-tools');wait(lambda:script("return !!document.querySelector('dialog[open]')"));settled();wait(lambda:script("return !!document.querySelector('.php-diagnostics')"));assert script("return document.querySelector('.php-diagnostics').textContent.includes('pdo_sqlite')");screenshot('setup-php-diagnostics.png');audit('php-diagnostics');fill(field('memory-limit'),'768M');fill(field('upload-mb'),'32');fill(field('post-mb'),'40');fill(field('php-timezone'),'Europe/Moscow');button('Save changes','dialog');wait(lambda:not script("return !!document.querySelector('dialog[open]')"));settled();wait(lambda:row(fresh_id)['project']['php_settings']['memory_limit']=='768M');wait(lambda:script('return document.activeElement?.id===arguments[0]',f'project-{fresh_id}-php-settings'));assert [(n,s['pid']) for n,s in row(atlas)['processes']]==atlas_pids;assert [(n,s['pid']) for n,s in row(fresh_id)['processes']]!=fresh_pids;web=served(row(fresh_id)['serving']['preferred_url'].rstrip('/')+'/m8-ini');assert web['memory']=='768M' and web['timezone']=='Europe/Moscow';checks['actual_php_diagnostics_settings_restart_and_isolation']=True
 button('HTTPS setup','.setup-tools');wait(lambda:script("return !!document.querySelector('dialog[open]')"));settled();wait(lambda:script("return !!document.querySelector('.certificate-fingerprint')"));screenshot('setup-https-review.png');audit('https-review');assert script("return [...document.querySelectorAll('dialog button')].find(b=>b.textContent.trim()==='Apply system setup').disabled");host_path=pathlib.Path(script("return document.querySelector('dialog .setup-review dd').textContent"));hosts_before=hashlib.sha256(host_path.read_bytes()).hexdigest();click('dialog input[type=checkbox]');assert script("return ![...document.querySelectorAll('dialog button')].find(b=>b.textContent.trim()==='Apply system setup').disabled");button('Refresh review','dialog');settled();wait(lambda:script("return !document.querySelector('dialog input[type=checkbox]').checked"));button('Close dialog','dialog');assert hashlib.sha256(host_path.read_bytes()).hexdigest()==hosts_before;checks['https_review_consent_refresh_no_system_changes']=True
 for language in ['ru','uz-Latn','en']:
  click('a[href="#settings"]');click(f'input[value="{language}"]');wait(lambda:script('return document.documentElement.lang===arguments[0]',language));click('a[href="#projects"]');settled();click('.workspace-toolbar button.primary');wait(lambda:script("return !!document.querySelector('dialog[open]')"));call('POST','/window/rect',{'width':820,'height':620});screenshot('setup-source-small-'+language+'.png');audit('source-small-'+language);call('POST','/execute/async',{'script':"const done=arguments[arguments.length-1];const button=document.querySelector('dialog button[type=submit]');button.scrollIntoView({block:'center'});requestAnimationFrame(()=>{button.click();done(true)});",'args':[]});wait(lambda:script("return !!document.querySelector('[name=setup-name]')"));assert script('return document.querySelector("dialog").scrollWidth<=document.querySelector("dialog").clientWidth+1');screenshot('setup-environment-small-'+language+'.png');audit('environment-small-'+language);call('POST','/execute/sync',{'script':"document.querySelector('dialog .modal-title button').click();return true;",'args':[]})
 checks['ru_uz_en_minimum_source_environment']=True
 click('a[href="#settings"]');click('input[value=dark]');wait(lambda:script("return document.documentElement.dataset.theme==='dark'"));click('a[href="#projects"]');settled();wizard_start();screenshot('setup-environment-dark.png');audit('wizard-dark');button('Close dialog','dialog')
 quit_app();launch();click('a[href="#projects"]');settled();retained=named('M8 UI Clone');assert retained['navigation']['favorite'] and retained['navigation']['last_opened_ms']>0;assert named('M8 Fresh')['project']['php_settings']['memory_limit']=='768M';quit_app();checks['favorites_recent_php_persist_on_native_reopen']=True
 report={'passed':True,'native_webview':True,'ipc_mocked':False,'checks':checks,'native_system_authorization_exercised':False,'system_hosts_unchanged':True,'accessibility':accessibility,'captures':captures,'viewport_attempts':viewport_attempts};(args.fixture/'m8-desktop-result.json').write_text(json.dumps(report,indent=2));print('M8_DESKTOP_REPORT:'+json.dumps(report))
except Exception:
 try:screenshot('failure.png');(args.output/'failure.html').write_text(call('GET','/source'))
 except Exception:pass
 raise
finally:
 if process and process.poll() is None:
  try:invoke('quit_application');process.wait(timeout=60)
  except Exception:process.kill();process.wait()
