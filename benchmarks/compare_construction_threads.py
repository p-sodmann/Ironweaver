"""Paired 1/2-thread independent graph construction using one frozen binary."""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
from compare_construction import summarize

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('binary',type=Path)
p.add_argument('--pairs',type=int,default=12)
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
cpus=sorted(os.sched_getaffinity(0))[:2]
assert len(cpus)==2
out=dict(binary=str(a.binary.resolve()),sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),affinity=cpus,samples=7,raw=[])
for n in [1000,20000,100000]:
    rows=[]
    for i in range(a.pairs):
        pair=dict(order=[1,2] if i%2==0 else [2,1])
        for threads in pair['order']:
            r=subprocess.run(['taskset','-c',','.join(map(str,cpus[:threads])),str(a.binary.resolve()),'concurrent',str(n),'7'],env=os.environ|dict(RAYON_NUM_THREADS=str(threads),OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1'),capture_output=True,text=True,check=True)
            pair['baseline' if threads==1 else 'candidate']=[float(row['seconds']) for row in csv.DictReader(io.StringIO(r.stdout))]
        rows.append(pair)
    out['raw'].append(dict(n=n,pairs=rows,summary=summarize(rows)))
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(out,indent=2)+'\n')
    print(n,out['raw'][-1]['summary'],flush=True)
