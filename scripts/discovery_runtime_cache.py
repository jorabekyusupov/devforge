#!/usr/bin/env python3
"""Audit current reviewed catalog archives; superseded runtime identities stay intact."""
import argparse
import json
from pathlib import Path
import m0_runtime_audit as transport

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('target', choices=['macos-arm64', 'macos-x64', 'windows-x64'])
parser.add_argument('cache', type=Path)
args = parser.parse_args()
repo = Path(__file__).resolve().parent.parent
catalog = json.loads((repo/'runtime/catalog.json').read_text())
sources = json.loads((repo/'runtime/m0-candidates.json').read_text())
args.cache.mkdir(parents=True, exist_ok=True)
report = {'target': args.target, 'scope': 'Current non-superseded catalog: upstream archive SHA256 and static architecture audit', 'artifacts': []}
for package_id, package in sorted(catalog['packages'].items()):
    if package_id in catalog.get('superseded', {}) or package['target'] != args.target or not (
        package['kind'] in ['caddy', 'composer']
        or package['kind'] == 'php' and package['version'].startswith('8.5.')
        or package['kind'] == 'node' and package['version'].lstrip('v').startswith('24.')
    ):
        continue
    candidate = dict(next(entry for entry in sources if entry['url']==package['url'] and entry.get('sha256')==package['sha256']))
    result = transport.audit(candidate, args.cache)
    result['catalog_id'] = package_id
    report['artifacts'].append(result)
    (args.cache.parent/'discovery-runtime-downloads.json').write_text(json.dumps(report, indent=2)+'\n')
    if result['status'] == 'failed' or result.get('computed_sha256') != package['sha256']:
        raise SystemExit(f'Current catalog archive failed integrity/transport: {package_id}: {result}')
print(f'Current native fixture archives verified: {len(report["artifacts"])}')
