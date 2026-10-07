"""Focused unchanged-binding filter/BFS controls using frozen extensions.

Independent serial AB/BA process pairs; CPU 0, one Rayon/BLAS/OpenMP thread.
Each sample batches 128 calls, including output destruction/loop overhead.
Source construction and correctness checks are excluded. Preserve all samples.
"""
import argparse
import gc
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time

from compare_construction import summarize


def worker(n):
    import ironweaver
    import ironweaver._ironweaver as extension
    g=ironweaver.Vertex()
    ids=[f'n{i}' for i in range(n)]
    g.add_nodes(ids)
    g.add_edges([(ids[i%n],ids[(i+1)%n]) for i in range(n*5)])
    keep=['isolated0','isolated1','isolated2']
    g.add_nodes(keep)
    start=g.get_node(keep[0])
    output=dict(package=ironweaver.__file__,extension_sha256=hashlib.sha256(Path(extension.__file__).read_bytes()).hexdigest(),affinity=sorted(os.sched_getaffinity(0)),rayon_threads=os.environ['RAYON_NUM_THREADS'],batch=128,warmups=2,metrics={})
    for name,fn,count in [('filter-3-isolated',lambda:g.filter(ids=keep),3),('bfs-isolated',lambda:start.bfs(),1)]:
        check=fn()
        assert check.node_count()==count
        assert check.to_networkx().number_of_edges()==0
        check=None
        gc.collect();gc.disable()
        samples=[]
        try:
            for i in range(9):
                result=None
                tick=time.perf_counter_ns()
                for _ in range(128): result=fn()
                result=None
                elapsed=(time.perf_counter_ns()-tick)/1e9/128
                if i>=2: samples.append(elapsed)
        finally: gc.enable()
        output['metrics'][name]=samples
    return output


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('baseline',type=Path);p.add_argument('candidate',type=Path)
    p.add_argument('--pairs',type=int,default=10)
    p.add_argument('--sizes',default='1000,20000,100000')
    p.add_argument('--worker',type=int)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    if a.worker:
        a.output.write_text(json.dumps(worker(a.worker),indent=2)+'\n');return
    os.sched_setaffinity(0,{min(os.sched_getaffinity(0))})
    env=os.environ|dict(RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',PYTHONHASHSEED='42')
    packages=dict(baseline=a.baseline.resolve(),candidate=a.candidate.resolve())
    output=dict(batch=128,samples=7,warmups=2,cases={})
    a.output.parent.mkdir(parents=True,exist_ok=True)
    for n in map(int,a.sizes.split(',')):
        rows=[];output['cases'][str(n)]=rows
        for i in range(a.pairs):
            pair=dict(order=['baseline','candidate'] if i%2==0 else ['candidate','baseline'])
            with tempfile.TemporaryDirectory(prefix='ironweaver-binding-focus-') as folder:
                for revision in pair['order']:
                    path=Path(folder)/(revision+'.json')
                    subprocess.run([sys.executable,str(Path(__file__).resolve()),str(a.baseline),str(a.candidate),'--worker',str(n),'--output',str(path)],env=env|dict(PYTHONPATH=str(packages[revision])),check=True)
                    pair[revision]=json.loads(path.read_text())
                    assert Path(pair[revision]['package']).is_relative_to(packages[revision])
            rows.append(pair);a.output.write_text(json.dumps(output,indent=2)+'\n')
        output.setdefault('summary',{})[str(n)]={name:summarize([{revision:pair[revision]['metrics'][name] for revision in ('baseline','candidate')} for pair in rows]) for name in rows[0]['baseline']['metrics']}
        a.output.write_text(json.dumps(output,indent=2)+'\n')
        print(n,output['summary'][str(n)],flush=True)


if __name__=='__main__': main()
