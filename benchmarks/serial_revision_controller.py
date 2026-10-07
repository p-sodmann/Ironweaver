"""Checkpoint serial full-suite workers and completed AB/BA process pairs.

Uses compare_revisions' unchanged worker, reference cache and summaries. Resume
complete pairs only; incomplete workers are retained as abandoned observations.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

import compare_revisions as runner

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('baseline',type=Path);p.add_argument('candidate',type=Path)
p.add_argument('--pairs',type=int,default=6)
p.add_argument('--repeats',type=int,default=3)
p.add_argument('--data',type=Path,required=True)
p.add_argument('--reference-cache',type=Path,required=True)
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
env=os.environ|dict(PYTHONHASHSEED='42',RAYON_NUM_THREADS=str(len(os.sched_getaffinity(0))),OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',IRONWEAVER_DATA=str(a.data.resolve()),IRONWEAVER_REFERENCE_CACHE=str(a.reference_cache.resolve()))
packages=dict(baseline=a.baseline.resolve(),candidate=a.candidate.resolve())
workers=a.output.parent/'broad-workers';workers.mkdir(parents=True,exist_ok=True)
status=a.output.parent/'broad-status.json'
expected={revision:hashlib.sha256(next((package/'ironweaver').glob('*.so')).read_bytes()).hexdigest() for revision,package in packages.items()}
report=dict(pairs=a.pairs,repeats=a.repeats,cases={case:[] for case in runner.CASES})
identity=None
for i in range(a.pairs):
    for case in (runner.CASES if i%2==0 else list(reversed(runner.CASES))):
        key=f'pair-{i:02}-{case.replace(":","-")}'
        saved=workers/(key+'.json')
        if saved.exists():
            pair=json.loads(saved.read_text())
        else:
            # Never silently reuse a baseline timed before an interrupted job.
            for old in workers.glob(key+'-*'):
                old.rename(old.with_name('abandoned-'+str(time.time_ns())+'-'+old.name))
            pair=dict(order=['baseline','candidate'] if i%2==0 else ['candidate','baseline'])
            for revision in pair['order']:
                path=workers/(key+'-'+revision+'.json')
                cmd=[sys.executable,str(Path(runner.__file__).resolve()),str(a.baseline),str(a.candidate),'--worker',case,'--repeats',str(a.repeats),'--output',str(path)]
                status.write_text(json.dumps(dict(phase='broad',pid=os.getpid(),pair=i+1,total=a.pairs,case=case,revision=revision,started=time.time()),indent=2)+'\n')
                print(f'pair {i+1}/{a.pairs} {case} {revision}',flush=True)
                with (workers/(key+'-'+revision+'.log')).open('w') as log:
                    subprocess.run(cmd,env=env|dict(PYTHONPATH=str(packages[revision])),stdout=log,stderr=subprocess.STDOUT,check=True)
                pair[revision]=json.loads(path.read_text())
            saved.write_text(json.dumps(pair,indent=2)+'\n')
        for revision in pair['order']:
            run=pair[revision]
            assert run['extension_sha256']==expected[revision]
            assert Path(run['package']).is_relative_to(packages[revision])
            if identity is None: identity=run['identity']
            assert run['identity']==identity
        for key in ('rustc','cargo_lock_sha256','flags','rustflags','bindings_sha256'):
            assert pair['baseline']['build'][key]==pair['candidate']['build'][key],key
        report['cases'][case].append(pair)
        a.output.write_text(json.dumps(report,indent=2)+'\n')
report['summary']={case:runner.summarize(pairs) for case,pairs in report['cases'].items()}
a.output.write_text(json.dumps(report,indent=2)+'\n')
status.write_text(json.dumps(dict(phase='complete',pid=os.getpid(),finished=time.time()),indent=2)+'\n')
print('complete: all broad-suite process pairs persisted',flush=True)
