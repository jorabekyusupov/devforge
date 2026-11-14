#!/usr/bin/env python3
"""Reproducible internal packaging. Never publishes or enables automatic updates."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, sys
from urllib.parse import quote
ROOT = pathlib.Path(__file__).resolve().parents[1]
def run(*args, **kw):
    if kw.get("text"): kw.setdefault("encoding", "utf-8")
    try:
        return subprocess.run(args, cwd=ROOT, check=True, **kw)
    except subprocess.CalledProcessError as error:
        if error.stderr: print(error.stderr, file=sys.stderr)
        raise
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
MANAGER_TARGETS = {'aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc'}

def manager_manifest(sidecars, generated, target):
    """Pin the copied CLI, not PATH or an executable selected at GUI runtime."""
    if target not in MANAGER_TARGETS:
        raise ValueError('Unsupported manager target')
    suffix = '.exe' if 'windows' in target else ''
    copied = sidecars / ('devforge-' + target + suffix)
    if copied.is_symlink() or not copied.is_file():
        raise ValueError('Copied manager CLI must be a regular file')
    value = {'schema': 1, 'target': target, 'filename': 'devforge' + suffix, 'sha256': sha(copied)}
    path = generated / 'manager-artifact.json'
    path.write_text(json.dumps(value, indent=2), encoding='utf-8')
    return value

def manager_bundle_config(config, sidecars, target):
    if target not in MANAGER_TARGETS:
        raise ValueError('Unsupported manager target')
    if 'apple' in target:
        # Tauri 2.12.4 app.rs copies macOS.files without adding it to
        # sign_paths. keychain.rs signs explicit targets without --deep.
        # Preserve the pre-signed CLI bytes whose hash the GUI compiles.
        config['bundle']['externalBin'] = [name for name in config['bundle']['externalBin'] if name != 'binaries/devforge']
        config['bundle'].setdefault('macOS', {}).setdefault('files', {})['MacOS/devforge'] = str(sidecars / ('devforge-' + target))
    return config

def capture_manager_authority(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError('Manager authority must be a regular file')
    with path.open('rb') as source:
        raw = source.read(1025)
    if len(raw) > 1024:
        raise ValueError('Manager authority exceeds byte limit')
    return raw, json.loads(raw)

def verify_authority_unchanged(path, captured):
    current, _ = capture_manager_authority(path)
    if current != captured:
        raise ValueError('Manager authority changed during build; rebuild from stable inputs')

def verify_manager_payload(candidate, manifest, target, scope):
    if target not in MANAGER_TARGETS or manifest['target'] != target:
        raise ValueError('Manager payload target mismatch')
    if candidate.is_symlink() or not candidate.is_file() or sha(candidate) != manifest['sha256']:
        raise ValueError('Final manager payload differs from compiled authority')
    return {'target': target, 'filename': manifest['filename'], 'sha256': manifest['sha256'],
            'verification_scope': scope, 'hash_matches_compiled_authority': True,
            'bundled_cli_verified': scope == 'macos-app-bundle', 'installed_cli_verified': False}

def prepare(target, profile='release'):
    generated = ROOT/'distribution/generated'
    if generated.exists(): shutil.rmtree(generated)
    generated.mkdir(parents=True)
    metadata = json.loads(run('cargo','metadata','--locked','--format-version','1','--offline','--filter-platform',target, capture_output=True, text=True).stdout)
    packages=[]; missing=[]
    packaging_notices=generated/'notices/packaging';packaging_notices.mkdir(parents=True)
    for text in (ROOT/'distribution/windows').glob('TAURI-LICENSE-*'):shutil.copy2(text,packaging_notices/text.name)
    for pkg in metadata['packages']:
        if pkg['source'] is None: continue
        identity=f"{pkg['name']}-{pkg['version']}"
        parent=pathlib.Path(pkg['manifest_path']).parent
        texts=[p for p in parent.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT'))]
        if not texts: missing.append(identity)
        for p in texts:
            out=generated/'notices'/'cargo'/identity/p.name;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out)
        packages.append({'type':'library','name':pkg['name'],'version':pkg['version'],'purl':f"pkg:cargo/{pkg['name']}@{pkg['version']}",'licenses':[{'expression':pkg['license']}] if pkg.get('license') else [],'properties':[{'name':'devforge:scope','value':'cargo-lock-inventory-not-link-map'}]})
    lock=json.loads((ROOT/'package-lock.json').read_text(encoding='utf-8'))
    for key,pkg in lock['packages'].items():
        if not key.startswith('node_modules/') or pkg.get('dev') or pkg.get('link'):continue
        parent=ROOT/key;name=key.split('node_modules/')[-1];identity=name.replace('/','_')+'-'+pkg['version']
        texts=[p for p in parent.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT'))] if parent.exists() else []
        if not texts:missing.append(identity)
        for p in texts:
            out=generated/'notices'/'npm'/identity/p.name;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out)
        packages.append({'type':'library','name':name,'version':pkg['version'],'purl':f"pkg:npm/{quote(name,safe='/')}@{pkg['version']}",'licenses':[{'expression':pkg['license']}] if pkg.get('license') else []})
    (generated/'sbom.cdx.json').write_text(json.dumps({'bomFormat':'CycloneDX','specVersion':'1.6','version':1,'components':packages},indent=2))
    shutil.copy2(ROOT/'runtime/catalog.json',generated/'runtime-catalog.json')
    (generated/'distribution.json').write_text(json.dumps({'schema':1,'channel':'internal-unsigned','target':target,'version':next(p['version'] for p in metadata['packages'] if p['name']=='devforge-desktop'),'public_release_allowed':False,'managed_runtime_archives_bundled':False,'gates':['os-signing-notarization','redistribution-license-review','minimum-os-native-acceptance'],'missing_root_license_texts':missing,'sbom_scope':'Cargo lock inventory and production npm dependencies; not a binary linkage attestation','runtime_redistribution_gate':'Open: upstream runtime archives are fetched on demand; transitive provider notices/reproducibility require review'},indent=2))
    suffix='.exe' if 'windows' in target else ''
    sidecars=ROOT/'apps/desktop/src-tauri/binaries';sidecars.mkdir(exist_ok=True)
    for name in ['devforge','devforge-helper']:
        shutil.copy2(ROOT/'target'/profile/(name+suffix),sidecars/(name+'-'+target+suffix))
    if 'apple' in target:
        run('/usr/bin/codesign', '--force', '--sign', '-', '--options', 'runtime',
            '--identifier', 'com.devforge.manager', str(sidecars / ('devforge-' + target)))
        run('/usr/bin/codesign', '--verify', '--strict', str(sidecars / ('devforge-' + target)))
    manager_manifest(sidecars, generated, target)
    return generated

def main():
    p=argparse.ArgumentParser();p.add_argument('--prepare-only',action='store_true');p.add_argument('--app-only',action='store_true', help='macOS internal app bundle only, without a DMG');args=p.parse_args()
    target=run('rustc','-vV',capture_output=True,text=True).stdout.split('host: ')[1].splitlines()[0]
    if args.app_only and 'apple' not in target:raise SystemExit('--app-only requires macOS')
    if target not in ['aarch64-apple-darwin','x86_64-apple-darwin','x86_64-pc-windows-msvc']:raise SystemExit('Unsupported packaging target')
    # Capture provenance before any artifact build, not after concurrent work
    # may have advanced HEAD. This is deliberately not a frozen source snapshot.
    build_input_commit = run('git','rev-parse','HEAD',capture_output=True,text=True).stdout.strip()
    build_input_dirty = bool(run('git','status','--porcelain',capture_output=True,text=True).stdout.strip())
    run('cargo','fetch','--locked','--target',target)
    if not args.prepare_only:run('cargo','build','--locked','--release','-p','devforge-cli','-p','devforge-serving','--bin','devforge','--bin','devforge-helper')
    prepare(target)
    authority_path = ROOT/'distribution/generated/manager-artifact.json'
    authority_bytes, manager = capture_manager_authority(authority_path)
    if args.prepare_only:return
    config=json.loads((ROOT/'distribution/bundle.json').read_text(encoding='utf-8'))
    if 'apple' in target:
        config['bundle'].update({'targets':['app'] if args.app_only else ['app','dmg'],'macOS':{'signingIdentity':'-'}})
    manager_bundle_config(config, ROOT/'apps/desktop/src-tauri/binaries', target)
    if (ROOT/'target/release/bundle').exists(): shutil.rmtree(ROOT/'target/release/bundle')
    config_path=ROOT/'apps/desktop/src-tauri/tauri.internal.generated.json'
    config_path.write_text(json.dumps(config,indent=2))
    build_env=os.environ.copy();build_env['CI']='true';build_env['DEVFORGE_PACKAGE_MANAGER']='1';build_env.pop('TAURI_BUNDLER_DMG_IGNORE_CI',None)
    run('npm'+('.cmd' if os.name=='nt' else ''),'run','tauri','--','build','--config',str(config_path),env=build_env)
    bundle=ROOT/'target/release/bundle'
    verify_authority_unchanged(authority_path, authority_bytes)
    candidate = bundle/'macos/DevForge.app/Contents/MacOS/devforge' if 'apple' in target else ROOT/'apps/desktop/src-tauri/binaries'/('devforge-'+target+'.exe')
    manager_attestation = verify_manager_payload(candidate, manager, target, 'macos-app-bundle' if 'apple' in target else 'nsis-external-binary-source')
    if 'apple' in target:
        run('/usr/bin/codesign', '--verify', '--strict', str(candidate))
        run('/usr/bin/codesign', '--verify', '--strict', str(bundle/'macos/DevForge.app'))
    artifacts=[p for p in bundle.rglob('*') if p.is_file() and p.suffix in ['.dmg','.exe']]
    report={'schema':1,'manager_artifact':manager_attestation,'channel':'internal-unsigned','target':target,'public_release_allowed':False,'commit':build_input_commit,'dirty_at_build':build_input_dirty,'source_tree_frozen':False,'provenance_scope':'HEAD and dirty flag captured before build; unfrozen working tree may contain concurrent changes','artifacts':[{'path':str(p.relative_to(ROOT)),'sha256':sha(p),'bytes':p.stat().st_size} for p in artifacts]}
    out=ROOT/'output/distribution';out.mkdir(parents=True,exist_ok=True);(out/'package-manifest.json').write_text(json.dumps(report,indent=2))
    print('M6_PACKAGE_REPORT:'+json.dumps(report),flush=True)
if __name__=='__main__':main()
