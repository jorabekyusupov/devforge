#!/usr/bin/env python3
"""Reproducible internal packaging. Never publishes or enables automatic updates."""
import argparse, hashlib, json, os, pathlib, platform, shutil, subprocess, sys
ROOT = pathlib.Path(__file__).resolve().parents[1]
def run(*args, **kw):
    try:
        return subprocess.run(args, cwd=ROOT, check=True, **kw)
    except subprocess.CalledProcessError as error:
        if error.stderr: print(error.stderr, file=sys.stderr)
        raise
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def prepare(target, profile='release'):
    generated = ROOT/'distribution/generated'
    if generated.exists(): shutil.rmtree(generated)
    generated.mkdir(parents=True)
    metadata = json.loads(run('cargo','metadata','--locked','--format-version','1','--offline','--filter-platform',target, capture_output=True, text=True).stdout)
    packages=[]; missing=[]
    for pkg in metadata['packages']:
        if pkg['source'] is None: continue
        identity=f"{pkg['name']}-{pkg['version']}"
        parent=pathlib.Path(pkg['manifest_path']).parent
        texts=[p for p in parent.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT'))]
        if not texts: missing.append(identity)
        for p in texts:
            out=generated/'notices'/'cargo'/identity/p.name;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out)
        packages.append({'type':'library','name':pkg['name'],'version':pkg['version'],'purl':f"pkg:cargo/{pkg['name']}@{pkg['version']}",'licenses':[{'expression':pkg['license']}] if pkg.get('license') else [],'properties':[{'name':'devforge:scope','value':'cargo-lock-inventory-not-link-map'}]})
    lock=json.loads((ROOT/'package-lock.json').read_text())
    for key,pkg in lock['packages'].items():
        if not key.startswith('node_modules/') or pkg.get('dev') or pkg.get('link'):continue
        parent=ROOT/key;name=key.split('node_modules/')[-1];identity=name.replace('/','_')+'-'+pkg['version']
        texts=[p for p in parent.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT'))] if parent.exists() else []
        if not texts:missing.append(identity)
        for p in texts:
            out=generated/'notices'/'npm'/identity/p.name;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out)
        packages.append({'type':'library','name':name,'version':pkg['version'],'purl':f"pkg:npm/{name}@{pkg['version']}",'licenses':[{'expression':pkg['license']}] if pkg.get('license') else []})
    (generated/'sbom.cdx.json').write_text(json.dumps({'bomFormat':'CycloneDX','specVersion':'1.6','version':1,'components':packages},indent=2))
    shutil.copy2(ROOT/'runtime/catalog.json',generated/'runtime-catalog.json')
    (generated/'distribution.json').write_text(json.dumps({'schema':1,'channel':'internal-unsigned','target':target,'version':next(p['version'] for p in metadata['packages'] if p['name']=='devforge-desktop'),'public_release_allowed':False,'managed_runtime_archives_bundled':False,'gates':['os-signing-notarization','redistribution-license-review','minimum-os-native-acceptance'],'missing_root_license_texts':missing,'sbom_scope':'Cargo lock inventory and production npm dependencies; not a binary linkage attestation','runtime_redistribution_gate':'Open: upstream runtime archives are fetched on demand; transitive provider notices/reproducibility require review'},indent=2))
    suffix='.exe' if 'windows' in target else ''
    sidecars=ROOT/'apps/desktop/src-tauri/binaries';sidecars.mkdir(exist_ok=True)
    for name in ['devforge','devforge-helper']:
        shutil.copy2(ROOT/'target'/profile/(name+suffix),sidecars/(name+'-'+target+suffix))
    return generated

def main():
    p=argparse.ArgumentParser();p.add_argument('--prepare-only',action='store_true');args=p.parse_args()
    target=run('rustc','-vV',capture_output=True,text=True).stdout.split('host: ')[1].splitlines()[0]
    if target not in ['aarch64-apple-darwin','x86_64-apple-darwin','x86_64-pc-windows-msvc']:raise SystemExit('Unsupported packaging target')
    run('cargo','fetch','--locked','--target',target)
    if not args.prepare_only:run('cargo','build','--locked','--release','-p','devforge-cli','-p','devforge-serving','--bin','devforge','--bin','devforge-helper')
    prepare(target)
    if args.prepare_only:return
    config=json.loads((ROOT/'distribution/bundle.json').read_text())
    if 'apple' in target:
        config['bundle'].update({'targets':['app','dmg'],'macOS':{'signingIdentity':'-'}})
    if (ROOT/'target/release/bundle').exists(): shutil.rmtree(ROOT/'target/release/bundle')
    config_path=ROOT/'apps/desktop/src-tauri/tauri.internal.generated.json'
    config_path.write_text(json.dumps(config,indent=2))
    build_env=os.environ.copy();build_env['CI']='true';build_env.pop('TAURI_BUNDLER_DMG_IGNORE_CI',None)
    run('npm'+('.cmd' if os.name=='nt' else ''),'run','tauri','--','build','--config',str(config_path),env=build_env)
    bundle=ROOT/'target/release/bundle'
    artifacts=[p for p in bundle.rglob('*') if p.is_file() and p.suffix in ['.dmg','.exe']]
    report={'schema':1,'channel':'internal-unsigned','target':target,'public_release_allowed':False,'commit':run('git','rev-parse','HEAD',capture_output=True,text=True).stdout.strip(),'dirty_at_build':bool(run('git','status','--porcelain',capture_output=True,text=True).stdout.strip()),'artifacts':[{'path':str(p.relative_to(ROOT)),'sha256':sha(p),'bytes':p.stat().st_size} for p in artifacts]}
    out=ROOT/'output/distribution';out.mkdir(parents=True,exist_ok=True);(out/'package-manifest.json').write_text(json.dumps(report,indent=2))
    print('M6_PACKAGE_REPORT:'+json.dumps(report),flush=True)
if __name__=='__main__':main()
