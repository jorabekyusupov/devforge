#!/usr/bin/env python3
"""Fetch only the native discovery fixture's packages with existing audited transport.
The production catalog's pinned hashes remain authoritative; no catalog updates.
"""
import argparse
import json
import hashlib
import os
import re
import urllib.request
from pathlib import Path
import m0_runtime_audit as transport

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('target', choices=['macos-arm64', 'macos-x64', 'windows-x64'])
parser.add_argument('cache', type=Path)
args = parser.parse_args()
repo = Path(__file__).resolve().parent.parent
catalog = json.loads((repo/'runtime/catalog.json').read_text())['packages']
sources = {entry['url']: entry for entry in json.loads((repo/'runtime/m0-candidates.json').read_text())}
args.cache.mkdir(parents=True, exist_ok=True)
manifest_path = repo/'.github/fixtures/discovery-runtime-blobs.json'
reviewed = json.loads(manifest_path.read_text()) if manifest_path.exists() else {'packages': {}}

def original_archive(package_id, package, candidate):
    """Read a public original archive, bound to the unchanged reviewed SHA256."""
    entry = reviewed['packages'].get(package_id)
    if not entry:
        return None
    if entry['sha256'] != package['sha256'] or not re.fullmatch(r'[0-9a-f]{40}', entry['git_blob_sha']):
        raise SystemExit('Reviewed fixture cache does not match the product catalog pin')
    repository = reviewed['repository']
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise SystemExit('Invalid reviewed fixture repository')
    path = args.cache/(candidate['id']+'-'+package['url'].rsplit('/', 1)[-1])
    if path.exists() and transport.digest(path) == package['sha256']:
        return entry
    headers = {'Accept': 'application/vnd.github.raw+json', 'User-Agent': 'DevForge-native-acceptance'}
    token = os.environ.get('GITHUB_TOKEN')
    if token and os.environ.get('GITHUB_REPOSITORY') == repository:
        headers['Authorization'] = 'Bearer '+token
    request = urllib.request.Request('https://api.github.com/repos/'+repository+'/git/blobs/'+entry['git_blob_sha'], headers=headers)
    class StrictRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, response_headers, url):
            if not url.startswith('https://api.github.com/'):
                raise RuntimeError('Unexpected fixture-cache redirect host')
            return super().redirect_request(req, fp, code, msg, response_headers, url)
    partial = path.with_suffix(path.suffix+'.reviewed-part')
    try:
        digest = hashlib.sha256()
        size = 0
        with urllib.request.build_opener(StrictRedirect()).open(request, timeout=180) as response, partial.open('wb') as output:
            while True:
                chunk = response.read(1024*1024)
                if not chunk:
                    break
                size += len(chunk)
                if size > entry['bytes'] or size > 100*1024*1024:
                    raise RuntimeError('Reviewed fixture archive exceeds its recorded size')
                output.write(chunk)
                digest.update(chunk)
        if size != entry['bytes'] or digest.hexdigest() != package['sha256']:
            raise RuntimeError('Reviewed fixture archive failed the unchanged catalog SHA256')
        partial.replace(path)
    finally:
        partial.unlink(missing_ok=True)
    print(package_id+': original reviewed archive restored with unchanged catalog hash', flush=True)
    return entry
report = {'target': args.target, 'scope': 'pinned public runtime archives only; static integrity inspection', 'artifacts': []}
for package_id, package in sorted(catalog.items()):
    if package['target'] != args.target or not (
        package['kind'] in ['caddy', 'composer']
        or package['kind'] == 'php' and package['version'].startswith('8.5.')
        or package['kind'] == 'node' and package['version'].lstrip('v').startswith('24.')
    ):
        continue
    candidate = dict(sources[package['url']])
    candidate['sha256'] = package['sha256']
    cache_source = original_archive(package_id, package, candidate)
    result = transport.audit(candidate, args.cache)
    if cache_source:
        result['original_reviewed_cache'] = cache_source
    result['catalog_id'] = package_id
    report['artifacts'].append(result)
    (args.cache.parent/'discovery-runtime-downloads.json').write_text(json.dumps(report, indent=2)+'\n')
    if result['status'] == 'failed' or result.get('computed_sha256') != package['sha256']:
        raise SystemExit(f'Pinned fixture archive failed integrity/transport: {package_id}: {result}')
print(f'Pinned native fixture archives verified: {len(report["artifacts"])}')
