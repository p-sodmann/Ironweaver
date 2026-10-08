"""Serial process pairs for every existing core creation/traversal fixture."""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess

from compare_construction import summarize


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('baseline', type=Path); p.add_argument('candidate', type=Path)
    p.add_argument('--pairs', type=int, default=6)
    p.add_argument('--filter', help='Only run one fixture for independent confirmation')
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    cpu = min(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpu})
    paths = dict(baseline=a.baseline.resolve(), candidate=a.candidate.resolve())
    out = dict(affinity=[cpu], threads=1, hashes={k: hashlib.sha256(v.read_bytes()).hexdigest() for k,v in paths.items()}, raw=[])
    env = os.environ | dict(RAYON_NUM_THREADS='1', OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1')
    if a.filter: env['IW_BENCH_FILTER'] = a.filter
    a.output.parent.mkdir(parents=True, exist_ok=True)
    for i in range(a.pairs):
        pair = dict(order=['baseline', 'candidate'] if i % 2 == 0 else ['candidate', 'baseline'])
        for revision in pair['order']:
            r = subprocess.run([str(paths[revision])], env=env, capture_output=True, text=True, check=True)
            pair[revision] = {row[0]: dict(seconds=float(row[1]), inner_count=int(row[2])) for row in csv.reader(io.StringIO(r.stdout))}
        assert pair['baseline'].keys() == pair['candidate'].keys()
        out['raw'].append(pair)
        a.output.write_text(json.dumps(out, indent=2)+'\n')
        print(f'completed traversal pair {i+1}', flush=True)
    out['summary'] = {key: summarize([dict(baseline=[pair['baseline'][key]['seconds']], candidate=[pair['candidate'][key]['seconds']]) for pair in out['raw']]) for key in out['raw'][0]['baseline']}
    a.output.write_text(json.dumps(out, indent=2)+'\n')


if __name__ == '__main__': main()
