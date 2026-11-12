#!/usr/bin/env python3
"""Disposable-host install/reinstall/uninstall + release localized-shell measurements.
Does not touch system trust. Data/DB fixtures are retained by installer operations.
"""
import argparse, ctypes, hashlib, json, os, pathlib, platform, shutil, signal, sqlite3, subprocess, time
ROOT=pathlib.Path(__file__).resolve().parents[1]
def run(*args,**kwargs):return subprocess.run(args,check=True,**kwargs)
def wait(test,seconds=60):
    until=time.monotonic()+seconds
    while time.monotonic()<until:
        value=test()
        if value:return value
        time.sleep(.1)
    raise TimeoutError('native package assertion timed out')
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def nsis_command(executable, assignment):
    # NSIS parses the entire final assignment itself, including spaces. CRT/list
    # argument quoting would change /D= and _?= semantics. No shell is used.
    values=[str(executable),assignment]
    if any(any(c in v for c in ['"','\r','\n','\0']) for v in values):
        raise ValueError('invalid NSIS path')
    return subprocess.list2cmdline([str(executable)])+' /S '+assignment
def main():
    p=argparse.ArgumentParser();p.add_argument('fixture',type=pathlib.Path);p.add_argument('--disposable-host',action='store_true',required=True);args=p.parse_args()
    fixture=args.fixture.resolve();fixture.mkdir(parents=True,exist_ok=True);data=fixture/'data';data.mkdir(exist_ok=True)
    os.environ['DEVFORGE_DATA_DIR']=str(data)
    out=ROOT/'output/distribution';out.mkdir(parents=True,exist_ok=True)
    installed=fixture/'Installed DevForge';bundle=ROOT/'target/release/bundle';mount=None
    report={'schema':1,'os':platform.platform(),'architecture':platform.machine(),'channel':'internal-unsigned','signing_verified':False,'cold_start_definition':'first process launch per locale after preference seed; OS disk caches are not flushed','memory_scope':'desktop main process RSS/working set only; WebView helpers and managed runtimes excluded','measurements':[],'checks':{}}
    windows=os.name=='nt'
    report['hardware']={'cpu':platform.processor(),'logical_cpus':os.cpu_count()}
    if not windows:
        report['hardware']['cpu']=run('sysctl','-n','machdep.cpu.brand_string',capture_output=True,text=True).stdout.strip()
        report['hardware']['physical_memory_bytes']=int(run('sysctl','-n','hw.memsize',capture_output=True,text=True).stdout.strip())
    else:
        class MemoryStatus(ctypes.Structure):_fields_=[('length',ctypes.c_uint32),('load',ctypes.c_uint32)]+[(name,ctypes.c_uint64) for name in ['totalPhysical','availablePhysical','totalPageFile','availablePageFile','totalVirtual','availableVirtual','availableExtendedVirtual']]
        memory=MemoryStatus();memory.length=ctypes.sizeof(memory)
        assert ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(memory))
        report['hardware']['physical_memory_bytes']=memory.totalPhysical
    report['package_manifest']=json.loads((out/'package-manifest.json').read_text())
    report['administrator_enabled']=bool(ctypes.windll.shell32.IsUserAnAdmin()) if windows else os.geteuid()==0
    assert not report['administrator_enabled']
    if windows:
        installer=next((bundle/'nsis').glob('*-setup.exe'))
        def install():
            subprocess.run(nsis_command(installer,'/D='+str(installed)),check=True,timeout=120);wait(lambda:(installed/'devforge.exe').exists())
        def uninstall():
            uninstaller=installed/'uninstall.exe';subprocess.run(nsis_command(uninstaller,'_?='+str(installed)),check=True,timeout=120);wait(lambda:not (installed/'devforge-desktop.exe').exists());time.sleep(2)
        application=installed/'devforge-desktop.exe';cli=installed/'devforge.exe'
    else:
        dmg=next((bundle/'dmg').glob('*.dmg'));mount=fixture/'Mounted DevForge';mount.mkdir(exist_ok=True)
        run('hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(dmg),stdout=subprocess.DEVNULL)
        def install():
            if installed.exists():shutil.rmtree(installed)
            run('ditto',str(mount/'DevForge.app'),str(installed/'DevForge.app'))
        def uninstall():shutil.rmtree(installed/'DevForge.app')
        application=installed/'DevForge.app/Contents/MacOS/devforge-desktop';cli=installed/'DevForge.app/Contents/MacOS/devforge'
    process=None
    try:
        # Denied targets use existing OS protections; do not change ACLs/TCC/UAC.
        if windows:
            denied=pathlib.Path(os.environ.get('ProgramW6432',os.environ['ProgramFiles']))/('DevForge-M6-denied-'+str(os.getpid()))
            denied_attempt=subprocess.run(nsis_command(installer,'/D='+str(denied)),capture_output=True,timeout=60)
        else:
            denied=mount/'Denied-DevForge.app'
            denied_attempt=subprocess.run(['ditto',str(mount/'DevForge.app'),str(denied)],capture_output=True,timeout=60)
        report['denied_target']={'return_code':denied_attempt.returncode,'directory_exists':denied.exists()}
        print('M6_DENIED_TARGET:'+json.dumps(report['denied_target']),flush=True)
        assert denied_attempt.returncode!=0 and not denied.exists(),report['denied_target']
        install();assert application.exists();assert cli.exists()
        report['checks']['permission_denied_target_then_retry']=True
        report['checks']['first_install']=True
        report['prerequisites']=json.loads(run(str(cli),'distribution','prerequisites',capture_output=True,text=True).stdout)
        run(str(cli),'distribution','check-uninstall')
        project=fixture/'Project outside app';project.mkdir(exist_ok=True);(project/'keep.txt').write_text('user project survives')
        db=data/'db-preservation-sentinel';db.write_bytes(os.urandom(1024));before=digest(db)
        # Existing native Laravel/PG fixture, when present, must survive replacement byte-for-byte.
        def owned_data_hashes():
            return {str(p.relative_to(data)):digest(p) for folder in ['projects','databases'] for p in (data/folder).rglob('*') if p.is_file()} if data.exists() else {}
        native_before=owned_data_hashes();report['preserved_existing_project_db_files']=len(native_before)
        def verify_preserved():
            current=owned_data_hashes()
            assert all(current.get(key)==value for key,value in native_before.items())
        # The standard Windows uninstaller's data-removal paths must also survive
        # with the application's actual default roots, beyond the fixture override.
        default_sentinels=[]
        if windows:
            for name in ['APPDATA','LOCALAPPDATA']:
                folder=pathlib.Path(os.environ[name])/'com.devforge.desktop';folder.mkdir(parents=True,exist_ok=True)
                sentinel=folder/('m6-preserve-'+str(os.getpid()));sentinel.write_bytes(b'preserve-default-root');default_sentinels.append(sentinel)
        for locale in ['ru','uz-Latn','en']:
            pref=data/'preferences.sqlite3'
            con=sqlite3.connect(pref);con.execute('CREATE TABLE IF NOT EXISTS preferences (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL)');con.execute('PRAGMA user_version=1');con.execute('INSERT INTO preferences(id,value) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET value=excluded.value',(json.dumps({'language':locale,'theme':'light'}),));con.commit();con.close()
            ready=fixture/(locale+'-startup.json');ready.unlink(missing_ok=True)
            started=time.monotonic();process=subprocess.Popen([str(application),'--startup-report',str(ready)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            wait(lambda:ready.exists() and ready.stat().st_size>0)
            startup=json.loads(ready.read_text());startup['parent_wall_ms']=(time.monotonic()-started)*1000
            assert startup['release'] and startup['locale']==locale and startup['native_ui_synced']
            secondary=subprocess.run([str(application)],capture_output=True,timeout=30)
            assert secondary.returncode==0 and process.poll() is None
            blocked=subprocess.run([str(cli),'distribution','check-stopped'],capture_output=True,text=True)
            assert blocked.returncode!=0
            if windows:
                attempt=subprocess.run(nsis_command(installed/'uninstall.exe','_?='+str(installed)),capture_output=True,timeout=60)
                assert attempt.returncode!=0 and application.exists() and process.poll() is None
            time.sleep(5)
            if windows:
                # Native process counters, with explicit 64-bit ctypes signatures.
                from ctypes import wintypes
                kernel=ctypes.WinDLL('kernel32',use_last_error=True);psapi=ctypes.WinDLL('psapi',use_last_error=True)
                kernel.OpenProcess.argtypes=[wintypes.DWORD,wintypes.BOOL,wintypes.DWORD];kernel.OpenProcess.restype=wintypes.HANDLE
                kernel.GetProcessTimes.argtypes=[wintypes.HANDLE,*([ctypes.POINTER(wintypes.FILETIME)]*4)];kernel.GetProcessTimes.restype=wintypes.BOOL
                kernel.CloseHandle.argtypes=[wintypes.HANDLE]
                class Memory(ctypes.Structure):_fields_=[('cb',wintypes.DWORD),('PageFaultCount',wintypes.DWORD)]+[(name,ctypes.c_size_t) for name in ['PeakWorkingSetSize','WorkingSetSize','QuotaPeakPagedPoolUsage','QuotaPagedPoolUsage','QuotaPeakNonPagedPoolUsage','QuotaNonPagedPoolUsage','PagefileUsage','PeakPagefileUsage']]
                psapi.GetProcessMemoryInfo.argtypes=[wintypes.HANDLE,ctypes.POINTER(Memory),wintypes.DWORD]
                handle=kernel.OpenProcess(0x410,False,startup['pid']);assert handle
                def cpu():
                    times=[wintypes.FILETIME() for _ in range(4)];assert kernel.GetProcessTimes(handle,*[ctypes.byref(v) for v in times]);return sum((v.dwHighDateTime<<32)|v.dwLowDateTime for v in times[2:])/1e7
                t0=cpu();start=time.monotonic();time.sleep(5);cpu_percent=(cpu()-t0)/(time.monotonic()-start)*100
                mem=Memory();mem.cb=ctypes.sizeof(mem);assert psapi.GetProcessMemoryInfo(handle,ctypes.byref(mem),mem.cb);rss=mem.WorkingSetSize;kernel.CloseHandle(handle)
            else:
                def cpu():
                    value=run('ps','-o','time=','-p',str(startup['pid']),capture_output=True,text=True).stdout.strip();parts=value.split(':');return float(parts[-1])+60*int(parts[-2])+(3600*int(parts[-3]) if len(parts)>2 else 0)
                t0=cpu();start=time.monotonic();time.sleep(5);cpu_percent=(cpu()-t0)/(time.monotonic()-start)*100
                rss=int(run('ps','-o','rss=','-p',str(startup['pid']),capture_output=True,text=True).stdout.strip())*1024
            startup.update({'idle_main_cpu_percent_one_core':cpu_percent,'idle_main_rss_bytes':rss,'idle_sample_seconds':5});report['measurements'].append(startup)
            # Forced termination also tests the OS-released installer lease, without accessibility automation.
            process.kill();process.wait(timeout=30);process=None;wait(lambda:subprocess.run([str(cli),'distribution','check-stopped'],capture_output=True).returncode==0)
        report['checks']['running_app_blocks_uninstall']=True if windows else 'CLI preflight blocks; Finder Trash cannot execute app hooks'
        report['checks']['three_locales_release_native_shell']=True
        report['checks']['second_launch_keeps_original_process']=True
        install();assert digest(db)==before and (project/'keep.txt').read_text()=='user project survives';verify_preserved()
        report['checks']['same_version_reinstall_preserves_project_db']=True
        run(str(cli),'distribution','check-uninstall');uninstall()
        assert digest(db)==before and (project/'keep.txt').exists();verify_preserved()
        report['checks']['uninstall_preserves_project_db']=True
        install();run(str(cli),'doctor',stdout=subprocess.DEVNULL);assert digest(db)==before;uninstall()
        report['checks']['reinstall_after_uninstall']=True
        report['checks']['default_data_roots_preserved']=bool(default_sentinels) if windows else 'Finder removes only the isolated app bundle'
        for sentinel in default_sentinels:
            assert sentinel.read_bytes()==b'preserve-default-root';sentinel.unlink()
    finally:
        if process and process.poll() is None:process.kill();process.wait(timeout=30)
        if mount:subprocess.run(['hdiutil','detach',str(mount)],check=False,stdout=subprocess.DEVNULL)
        (out/'acceptance.json').write_text(json.dumps(report,indent=2))
    print('M6_ACCEPTANCE_REPORT:'+json.dumps(report),flush=True)
if __name__=='__main__':main()
