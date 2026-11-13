#!/usr/bin/env python3
"""Fetch only the native discovery fixture's packages with existing audited transport.
The production catalog's pinned hashes remain authoritative; no catalog updates.
"""
import argparse
import json
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
    result = transport.audit(candidate, args.cache)
    result['catalog_id'] = package_id
    report['artifacts'].append(result)
    (args.cache.parent/'discovery-runtime-downloads.json').write_text(json.dumps(report, indent=2)+'\n')
    if result['status'] == 'failed' or result.get('computed_sha256') != package['sha256']:
        raise SystemExit(f'Pinned fixture archive failed integrity/transport: {package_id}: {result}')
print(f'Pinned native fixture archives verified: {len(report["artifacts"])}')
