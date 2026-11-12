#!/usr/bin/env python3
"""Read-only CLI inventory around explicitly authorized disposable CI helper steps."""
import argparse,json,os,pathlib,subprocess
p=argparse.ArgumentParser();p.add_argument('phase',choices=['before','after']);p.add_argument('root',type=pathlib.Path);p.add_argument('cli',type=pathlib.Path);a=p.parse_args();os.environ['DEVFORGE_DATA_DIR']=str(a.root.resolve());cli=str(a.cli.resolve())+('.exe' if os.name=='nt' and not str(a.cli).endswith('.exe') else '')
checked=subprocess.run([cli,'distribution','check-uninstall'],capture_output=True,text=True)
assert (checked.returncode!=0) if a.phase=='before' else checked.returncode==0,checked.stderr
output=a.root/('m6-cleanup-'+a.phase)
plan=subprocess.run([cli,'distribution','cleanup-plan',str(output.resolve())],capture_output=True,text=True,check=True)
result=json.loads(plan.stdout);assert result['applied'] is False and result['data_preserved']
assert len(result['plans'])==(2 if a.phase=='before' else 0),result
if a.phase=='before':
 actions=[]
 for item in result['plans']:
  body=json.loads(pathlib.Path(item['plan']).read_text());actions.append(body['action']['operation'])
 assert sorted(actions)==['hosts','trust_remove']
(a.root/('m6-cleanup-'+a.phase+'.json')).write_text(json.dumps(result,indent=2));print('M6_CLEANUP_REPORT:'+json.dumps({'phase':a.phase,**result}))
