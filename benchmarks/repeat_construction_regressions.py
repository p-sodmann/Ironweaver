"""Independently repeat primary CI regression signals with the frozen protocol."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--summary',type=Path,required=True)
p.add_argument('--raw',type=Path,default=Path('work/raw'))
p.add_argument('--artifacts',type=Path,default=Path('work/artifacts'))
p.add_argument('--pairs',type=int,default=12)
a=p.parse_args()
rows=json.loads(a.summary.read_text())
suspects=[r for r in rows if r['status']=='regression']
root=Path(__file__).resolve().parent
b=a.artifacts/'baseline';c=a.artifacts/'candidate'
commands=[]
seen_construction=set()
seen_bindings=set()
repeat_traversal=False
for i,r in enumerate(suspects):
 if r['suite']=='construction':
  case,n,_=r['name'].split('/')
  if (case,n) in seen_construction: continue
  seen_construction.add((case,n))
  commands.append([sys.executable,str(root/'compare_construction.py'),str(b/'construction_bench'),str(c/'construction_bench'),'--pairs',str(a.pairs),'--samples','7','--sizes',n,'--cases',case,'--output',str(a.raw/f'repeat-construction-{i:02}.json')])
 elif r['suite']=='python-focus':
  n=r['name'].split('/')[-1]
  if n in seen_bindings: continue
  seen_bindings.add(n)
  commands.append([sys.executable,str(root/'compare_edgeless_bindings.py'),str(b),str(c),'--pairs',str(a.pairs),'--sizes',n,'--output',str(a.raw/f'repeat-binding-focus-{i:02}.json')])
 elif r['suite']=='traversal':
  if repeat_traversal: continue
  repeat_traversal=True
  # Several signals share this inexpensive process. Repeat its full context
  # once rather than rebuilding every fixture separately for each signal.
  commands.append([sys.executable,str(root/'compare_traversal_revisions.py'),str(b/'traversal_bench'),str(c/'traversal_bench'),'--pairs',str(a.pairs),'--output',str(a.raw/'repeat-traversal.json')])
 elif r['suite']=='deletion':
  shape,n=r['name'].split('/')
  commands.append([sys.executable,str(root/'compare_remove_node.py'),str(b/'bench_remove_node'),str(c/'bench_remove_node'),'--pairs',str(a.pairs),'--samples','3','--cases',shape+':'+n,'--output',str(a.raw/f'repeat-deletion-{i:02}.json')])
selection={}
suite=json.loads((a.raw/'suite.json').read_text())
for r in suspects:
 if r['suite'] not in ('networkx','libraries','memory'): continue
 parts=r['name'].split('/')
 length=3 if r['suite'] in ('networkx','memory') else 2
 case=':'.join(parts[:length]);name='/'.join(parts[length:])
 s=selection.setdefault(case,dict(names=[],lines=[]));s['names'].append(name)
 metric=suite['cases'][case][0]['baseline']['metrics'][name]
 if 'line' in metric: s['lines'].append(metric['line'])
if selection:
 path=a.raw/'repeat-selection.json';path.write_text(json.dumps(selection,indent=2)+'\n')
 commands.append(['taskset','-c','0,1',sys.executable,str(root/'compare_revisions.py'),str(b),str(c),'--serial','--data','work/data','--pairs',str(a.pairs),'--repeats','3','--reference-cache','work/reference-cache','--no-prime','--measure-only',str(path),'--cases',','.join(selection),'--output',str(a.raw/'repeat-suite.json')])
(a.raw/'repeat-plan.json').write_text(json.dumps(dict(suspects=suspects,commands=commands),indent=2)+'\n')
for i,cmd in enumerate(commands):
 print(f'independent confirmation {i+1}/{len(commands)}: {cmd}',flush=True)
 with (a.raw/f'repeat-job-{i:02}.log').open('w') as log:
  subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,check=True)
