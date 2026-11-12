"""Isolated native Windows PHP/Composer path diagnostic; no user/project code."""
import hashlib,json,os,pathlib,stat,subprocess,tempfile,urllib.request,zipfile,io
assert os.name=='nt','Windows native diagnostic only'
catalog=json.loads((pathlib.Path(__file__).resolve().parent.parent/'runtime/catalog.json').read_text())
packages=list(catalog['packages'].values())
php=next(p for p in packages if p['target']=='windows-x64' and p['kind']=='php' and p['version'].startswith('8.5.'))
composer=next(p for p in packages if p['target']=='windows-x64' and p['kind']=='composer')
output=pathlib.Path('output/playwright/m8-probe');output.mkdir(parents=True,exist_ok=True)
def download(package):
 with urllib.request.urlopen(package['url'],timeout=90) as response:data=response.read(150*1024*1024+1)
 assert len(data)<=150*1024*1024 and hashlib.sha256(data).hexdigest()==package['sha256']
 return data
cases=[]
with tempfile.TemporaryDirectory(prefix='devforge-m8-bootstrap-') as tmp:
 root=pathlib.Path(tmp);runtime=root/'runtime';runtime.mkdir()
 with zipfile.ZipFile(io.BytesIO(download(php))) as z:
  for entry in z.infolist():
   p=pathlib.PurePosixPath(entry.filename.replace('\\','/'));assert not p.is_absolute() and '..'not in p.parts and not any(':'in part for part in p.parts) and not stat.S_ISLNK(entry.external_attr>>16)
  z.extractall(runtime)
 phar=root/'composer.phar';phar.write_bytes(download(composer));binary=next(runtime.rglob('php.exe'))
 for index,name in enumerate(['ascii-config',"M8 \u040e\u0437\u0431\u0435\u043a & ' config"]):
  directory=root/name;directory.mkdir();scan=directory/'scan';scan.mkdir();ini=directory/'php.ini'
  text='date.timezone=UTC\nextension_dir="'+str(binary.parent/'ext').replace('\\','/')+'"\n'
  for extension in ['openssl','mbstring','fileinfo','curl','pdo_pgsql','pdo_sqlite','sqlite3','intl','sodium','zip']:
   if (binary.parent/'ext'/('php_'+extension+'.dll')).is_file():text+='extension='+extension+'\n'
  ini.write_text(text,encoding='utf-8')
  env={key:value for key,value in os.environ.items() if key in ['SystemRoot','WINDIR','TEMP','TMP','HOME','USERPROFILE','APPDATA','LOCALAPPDATA']}
  env.update({'PATH':str(binary.parent)+os.pathsep+os.environ['PATH'],'LC_ALL':'C','LANG':'C','PHPRC':str(ini),'PHP_INI_SCAN_DIR':str(scan),'COMPOSER_HOME':str(directory/'composer'),'COMPOSER_CACHE_DIR':str(root/'cache')})
  script="echo json_encode(['ini'=>php_ini_loaded_file(),'openssl'=>extension_loaded('openssl'),'curl'=>extension_loaded('curl'),'zip'=>extension_loaded('zip')]);"
  probe=subprocess.run([str(binary),'-c',str(ini),'-r',script],cwd=directory,env=env,capture_output=True,timeout=30)
  target=root/('payload-'+str(index))
  create=subprocess.run([str(binary),'-c',str(ini),str(phar),'create-project','laravel/laravel',str(target),'^13.0','--no-install','--no-scripts','--no-plugins','--no-interaction','--prefer-dist'],cwd=directory,env=env,capture_output=True,timeout=120)
  cases.append({'case':'ascii' if index==0 else 'unicode','php_exit':probe.returncode,'php_stdout':probe.stdout.decode('utf-8','replace')[:3000],'php_stderr':probe.stderr.decode('utf-8','replace')[:3000],'composer_exit':create.returncode,'composer_stdout':create.stdout.decode('utf-8','replace')[-3000:],'composer_stderr':create.stderr.decode('utf-8','replace')[-4000:],'project_created':(target/'composer.json').is_file()})
report={'native_windows':True,'php':php['version'],'composer':composer['version'],'cases':cases}
(output/'bootstrap-probe.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print('M8_BOOTSTRAP_PROBE_REPORT:'+json.dumps(report))
