#!/usr/bin/env python3
"""Native WKWebView/WebView2 acceptance via debug-only embedded W3C WebDriver.
No invoke mocking. Only lifecycle observation/control uses explicit native IPC.
"""
import argparse,base64,json,os,pathlib,socket,sqlite3,subprocess,time,urllib.request,urllib.error,uuid
p=argparse.ArgumentParser();p.add_argument('--picker-handoff',action='store_true');p.add_argument('fixture',type=pathlib.Path);p.add_argument('application',type=pathlib.Path);p.add_argument('launcher',type=pathlib.Path);p.add_argument('--output',type=pathlib.Path,default=pathlib.Path('output/playwright/discovery'));args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
base=f'http://127.0.0.1:{port}';session='';process=None;accessibility=[];captures=[];viewport_attempts=[]
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
 ident=element(css);wait(lambda:call('GET',f'/element/{ident}/enabled'));call('POST',f'/element/{ident}/clear',{});call('POST',f'/element/{ident}/value',{'text':text})
def snap():return invoke('workspace_snapshot')
def settled():return wait(lambda:not snap()['busy'])
def row(id):return next(r for r in snap()['snapshot']['projects'] if r['project']['id']==id)
def project(id):click(f'a.project-row[href$="{id}"]')
def button(label,scope='.project-detail'):
 wait(lambda:script("return [...document.querySelectorAll(arguments[0]+' button')].some(b=>(b.textContent.trim()===arguments[1]||b.getAttribute('aria-label')===arguments[1])&&!b.disabled)",scope,label))
 css=script("const nodes=[...document.querySelectorAll(arguments[0]+' button')];const index=nodes.findIndex(b=>b.textContent.trim()===arguments[1]||b.getAttribute('aria-label')===arguments[1]);if(index<0)throw new Error('Button missing '+arguments[1]);nodes[index].dataset.smoke='target';return '[data-smoke=target]';",scope,label);click(css);script("document.querySelector('[data-smoke=target]')?.removeAttribute('data-smoke');")
def content_viewport(width,height):
 for _ in range(3):
  metrics=script('return {width:innerWidth,height:innerHeight}');rect=call('GET','/window/rect')
  if metrics=={'width':width,'height':height}:
   viewport_attempts.append({'requested':{'width':width,'height':height},'actual':metrics,'matched':True});return
  call('POST','/window/rect',{'width':rect['width']+width-metrics['width'],'height':rect['height']+height-metrics['height']})
  call('POST','/execute/async',{'script':'const done=arguments[arguments.length-1];requestAnimationFrame(()=>requestAnimationFrame(()=>done(true)));','args':[]})
 actual=script('return {width:innerWidth,height:innerHeight}');viewport_attempts.append({'requested':{'width':width,'height':height},'actual':actual,'matched':False});print('DISCOVERY_VIEWPORT_LIMIT:'+json.dumps(viewport_attempts[-1]),flush=True);assert actual['width']>=820 and actual['height']>=588;assert script('return document.documentElement.scrollWidth<=innerWidth+1')
def screenshot(name):
 call('POST','/window/rect',{'x':0,'y':0})
 script("window.scrollTo(0,0);for(const el of document.querySelectorAll('main,.sidebar,dialog'))el.scrollTo(0,0)");call('POST','/execute/async',{'script':'const done=arguments[arguments.length-1];requestAnimationFrame(()=>requestAnimationFrame(()=>done(true)));','args':[]});time.sleep(.8)

 if name.startswith('folder'):script("document.querySelector('.discovery-settings')?.scrollIntoView({block:'center'})")
 value=call('GET','/screenshot');(args.output/name).write_bytes(base64.b64decode(value));captures.append({'file':name,'viewport':script('return {width:innerWidth,height:innerHeight,scale:devicePixelRatio,language:document.documentElement.lang,theme:document.documentElement.dataset.theme}')})
def audit(name):
 axe=pathlib.Path(__file__).resolve().parent.parent/'node_modules/axe-core/axe.min.js'
 script(axe.read_text())
 results=call('POST','/execute/async',{'script':"const done=arguments[arguments.length-1];window.axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(r=>done(r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))),e=>done({error:String(e)}));",'args':[]})
 accessibility.append({'case':name,'violations':results});assert results==[],results
 print('DISCOVERY_UI_PHASE: accessibility passed '+name,flush=True)
def launch():
 global process,session,port,base,opener
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';session='';opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 print('DISCOVERY_UI_PHASE: launch '+str(port),flush=True)
 ready=args.fixture/'data/e2e-page-ready';ready.unlink(missing_ok=True)
 process=subprocess.Popen([str(args.launcher.resolve()),'launch',str(args.application.resolve()),str(args.fixture.resolve()),str(port)])
 wait(lambda:ready.exists(),60);wait(lambda:request('GET','/status',timeout=3),60);session=request('POST','/session',{'capabilities':{'alwaysMatch':{}}})['sessionId'];call('POST','/timeouts',{'script':120000,'implicit':0});wait(lambda:script("return !!document.querySelector('h1')"),60);settled()
def quit_app():
 try:click('.quit-button')
 except Exception:pass
 process.wait(timeout=60);assert process.returncode==0,process.returncode


checks={}
try:
    native=json.loads((args.fixture/'discovery-native-result.json').read_text())
    database=sqlite3.connect(args.fixture/'data/preferences.sqlite3')
    preferences=json.loads(database.execute('SELECT value FROM preferences WHERE id=1').fetchone()[0]);preferences.update(project_folder=native['ui_root'],language='en',theme='light')
    database.execute('UPDATE preferences SET value=? WHERE id=1',(json.dumps(preferences),));database.commit();database.close()
    launch()
    click('a[href="#settings"]');wait(lambda:script("return document.documentElement.lang==='en'"))
    content_viewport(1536,1024)
    saved=invoke('bootstrap')['settings']['project_folder']
    wait(lambda:script("return document.querySelector('#project-folder')?.value===arguments[0]",saved))
    fill('#project-folder','relative/path');button('Save folder','.discovery-settings')
    wait(lambda:script("return !!document.querySelector('#project-folder-error')"));assert invoke('bootstrap')['settings']['project_folder']==saved
    checks['invalid_path_keeps_saved_root']=True;audit('invalid-folder');screenshot('folder-invalid.png')
    button('Use ~/Devforge','.discovery-settings');button('Create folder','.discovery-settings')
    wait(lambda:script("return !document.querySelector('.discovery-settings').getAttribute('aria-busy')||document.querySelector('.discovery-settings').getAttribute('aria-busy')==='false'"))
    button('Save folder','.discovery-settings')
    wait(lambda:invoke('bootstrap')['settings']['project_folder'].endswith('Devforge'))
    default_root=invoke('bootstrap')['settings']['project_folder'];assert pathlib.Path(default_root).is_dir()
    checks['explicit_default_create_save']=True
    custom=args.fixture.resolve()/('UI custom Ўзбек '+uuid.uuid4().hex[:6]);custom.mkdir(exist_ok=True)
    fill('#project-folder',str(custom));button('Save folder','.discovery-settings')
    wait(lambda:pathlib.Path(invoke('bootstrap')['settings']['project_folder']).resolve()==custom.resolve())
    checks['custom_unicode_path_saved']=True
    wait(lambda:script("return document.querySelector('.discovery-settings').getAttribute('aria-busy')==='false' && document.querySelector('#project-folder').value===arguments[0]",invoke('bootstrap')['settings']['project_folder']))
    click('a[href="#projects"]');wait(lambda:script("return !!document.querySelector('.discovery-projects')"))
    def is_auto(project):
        try:return os.path.samefile(project['project']['path'],auto)
        except OSError:return False
    # Real native event, not Scan now or an IPC-injected snapshot.
    name='UI Auto '+uuid.uuid4().hex[:6];auto=custom/name;auto.mkdir(exist_ok=True);(auto/'public').mkdir(exist_ok=True)
    (auto/'composer.json').write_text('{"require":{"laravel/framework":"^13","php":"^8.4"}}')
    (auto/'artisan').write_text('<?php');(auto/'public/index.php').write_text('<?php echo "UI discovery";')
    started=time.monotonic()
    wait(lambda:any(is_auto(r) for r in snap()['snapshot']['projects']),10)
    project_row=next(r for r in snap()['snapshot']['projects'] if is_auto(r))
    auto_id=project_row['project']['id']
    wait(lambda:script("return !!document.querySelector(arguments[0])",'a.project-row[href$="'+auto_id+'"]'),10)
    assert time.monotonic()-started<5
    checks['native_event_to_ui_under_5s']=True
    project(auto_id)
    wait(lambda:script("return document.querySelector('.project-detail h2')?.textContent===arguments[0]",name));audit('discovered-project');screenshot('projects-discovered.png')
    button('Forget project');wait(lambda:script("return !!document.querySelector('dialog[open]')"));button('Remove','dialog');wait(lambda:not any(r['project']['id']==auto_id for r in snap()['snapshot']['projects']))
    entry=next(e for e in snap()['snapshot']['discovery']['entries'] if e['project_id']==auto_id);scope='.discovery-projects li[data-discovery-identity="'+entry['identity']+'"]';button('Restore',scope)
    wait(lambda:any(is_auto(r) for r in snap()['snapshot']['projects']))
    checks['native_ui_forget_restore']=True
    button('New project','.workspace-toolbar');wait(lambda:script("return !!document.querySelector('dialog[open]')"));button('Continue','dialog')
    wait(lambda:script("return !!document.querySelector('#setup-parent-input')"))
    assert os.path.samefile(script("return document.querySelector('#setup-parent-input').value"),custom)
    audit('new-project-default-folder');button('Close dialog','dialog');wait(lambda:not script("return !!document.querySelector('dialog[open]')"))
    checks['new_project_uses_configured_parent']=True
    click('a[href="#settings"]');fill('#project-folder',native['ui_root']);button('Save folder','.discovery-settings');wait(lambda:invoke('bootstrap')['settings']['project_folder']==native['ui_root'])
    # Native window remount must restore saved settings and monitor without Projects open.
    quit_app();launch();click('a[href="#settings"]');wait(lambda:script("return document.querySelector('#project-folder')?.value===arguments[0]",native['ui_root']))
    checks['saved_folder_survives_native_relaunch']=True
    for locale in ['en','ru','uz-Latn']:
        click('input[value="'+locale+'"]');wait(lambda:script('return document.documentElement.lang===arguments[0]',locale))
        for theme in ['light','dark']:
            click('input[value="'+theme+'"]');wait(lambda:script('return document.documentElement.dataset.theme===arguments[0]',theme))
            script("document.querySelector('.discovery-settings').scrollIntoView({block:'center'})")
            audit('folder-'+locale+'-'+theme);screenshot('folder-'+locale+'-'+theme+'.png')
    checks['three_locales_light_dark']=True
    click('input[value=en]');click('input[value=light]');content_viewport(820,620)
    script("document.querySelector('.discovery-settings').scrollIntoView({block:'center'})");assert script('return document.documentElement.scrollWidth<=innerWidth+1');audit('folder-small');screenshot('folder-small.png')
    checks['small_window_no_horizontal_overflow']=True
    content_viewport(1536,1024)
    if os.name=='nt':
        picker_results=[]
        selected=args.fixture.resolve()/'no-runtime-root'
        for action in ['select','cancel']:
            script("[...document.querySelectorAll('.discovery-settings button')].find(b=>b.textContent.trim()==='Choose folder').click()")
            picker_env=os.environ.copy();picker_env.update(DEVFORGE_PICKER_PID=str(snap()['host_pid']),DEVFORGE_PICKER_ACTION=action,DEVFORGE_PICKER_PATH=str(selected))
            result=subprocess.run(['powershell','-NoProfile','-File',str(pathlib.Path(__file__).with_name('discovery_native_picker.ps1'))],env=picker_env,text=True,capture_output=True,timeout=40)
            assert result.returncode==0,(result.stdout,result.stderr)
            picker_results.append(json.loads(result.stdout))
            wait(lambda:script("return document.querySelector('.discovery-settings').getAttribute('aria-busy')==='false'"))
            assert pathlib.Path(script("return document.querySelector('#project-folder').value")).resolve()==selected.resolve()
            if action=='select':wait(lambda:script("return document.activeElement?.id==='project-folder'"))
            else:wait(lambda:script("return document.activeElement?.textContent.trim()==='Choose folder'"))
        fill('#project-folder',native['ui_root'])
        checks['native_system_folder_picker']={'select':True,'cancel':True,'focus_return':True,'method':'actual Windows dialog via UIAutomation, app PID restricted','actions':picker_results}
    elif not args.picker_handoff:
        selected=str((args.fixture.resolve()/'no-runtime-root').resolve())
        native_actions=[]
        for action in ['select','cancel']:
            script("[...document.querySelectorAll('.discovery-settings button')].find(b=>b.textContent.trim()==='Choose folder').click()")
            wait(lambda:invoke('native_picker_test',{'action':'inspect','path':selected}))
            if action=='select':
                native_actions.append(invoke('native_picker_test',{'action':'navigate','path':selected}))
                wait(lambda:(lambda state:pathlib.Path(state['directory']).resolve()==pathlib.Path(selected).resolve())(invoke('native_picker_test',{'action':'inspect','path':selected})))
            native_result=invoke('native_picker_test',{'action':action,'path':selected});native_actions.append(native_result)
            if action=='select':
                assert len(native_result['selected_urls'])==1,native_result
                chosen=native_result['selected_urls'][0]
            wait(lambda:script("return document.querySelector('.discovery-settings').getAttribute('aria-busy')==='false'"))
            print('DISCOVERY_UI_PHASE: native picker '+action+' '+json.dumps(script("return {value:document.querySelector('#project-folder').value,focus:document.activeElement?.id,busy:document.querySelector('.discovery-settings').getAttribute('aria-busy')}")),flush=True)
            assert pathlib.Path(script("return document.querySelector('#project-folder').value")).resolve()==pathlib.Path(chosen).resolve()
            wait(lambda:script("return document.activeElement?.id==='project-folder'" if action=='select' else "return document.activeElement?.textContent.trim()==='Choose folder'"))
        fill('#project-folder',native['ui_root'])
        checks['native_system_folder_picker']={'select':True,'cancel':True,'focus_return':True,'method':'actual visible NSOpenPanel operated by debug-only AppKit fixture; no mocked picker return','actions':native_actions}
    elif args.picker_handoff:
        (args.fixture/'picker-handoff-ready').write_text('ready')
        print('DISCOVERY_UI_PHASE: native picker ready for computer-use select/cancel',flush=True)
        wait(lambda:(args.fixture/'picker-handoff-result.json').exists(),3600)
        checks['native_system_folder_picker']=json.loads((args.fixture/'picker-handoff-result.json').read_text())
    else:
        checks['native_system_folder_picker']='NOT VERIFIED: native system-dialog interaction requires dedicated host automation'
    report={'native':'real WKWebView/WebView2 via debug-only W3C endpoint','checks':checks,'accessibility':accessibility,'captures':captures,'viewport_attempts':viewport_attempts,'source_boundary':'record source digest with collected reports'}
    (args.fixture/'discovery-desktop-result.json').write_text(json.dumps(report,indent=2));print('DISCOVERY_DESKTOP_REPORT:'+json.dumps(report),flush=True)
finally:
    if process and process.poll() is None:
        try:quit_app()
        except Exception:
            process.terminate();process.wait(timeout=30)
