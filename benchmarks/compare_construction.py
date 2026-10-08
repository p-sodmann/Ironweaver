"""Serial AB/BA process pairs for construction_bench; keep every inner sample.

Pin one CPU for sequential cases, two CPUs for independent-graph concurrency.
95% Student-t intervals use independent paired log ratios, never inner samples.
"""
import argparse
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess

from scipy.stats import t

CASES = ['random', 'chain', 'parallel', 'self', 'typed', 'explicit',
         'nodes', 'duplicates', 'stale', 'neighbors', 'subgraph', 'subgraph-small', 'concurrent']


def summarize(pairs):
    before = [statistics.median(p['baseline']) for p in pairs]
    after = [statistics.median(p['candidate']) for p in pairs]
    logs = [math.log(a / b) for b, a in zip(before, after)]
    mean = statistics.mean(logs)
    half = t.ppf(.975, len(logs) - 1) * statistics.stdev(logs) / math.sqrt(len(logs))
    return dict(before=statistics.median(before), after=statistics.median(after),
                change=100 * math.expm1(mean),
                ci95=[100 * math.expm1(mean - half), 100 * math.expm1(mean + half)])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('baseline', type=Path); p.add_argument('candidate', type=Path)
    p.add_argument('--pairs', type=int, default=6)
    p.add_argument('--samples', type=int, default=7)
    p.add_argument('--sizes', default='1000,20000')
    p.add_argument('--cases', default=','.join(CASES))
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    cpus = sorted(os.sched_getaffinity(0))[:2]
    env = os.environ | dict(OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1')
    paths = dict(baseline=a.baseline.resolve(), candidate=a.candidate.resolve())
    out = dict(platform=platform.platform(), boot=Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
               binaries={k: dict(path=str(v), sha256=hashlib.sha256(v.read_bytes()).hexdigest()) for k, v in paths.items()},
               pairs=a.pairs, samples=a.samples, warmups=2, results={})
    a.output.parent.mkdir(parents=True, exist_ok=True)
    for n in map(int, a.sizes.split(',')):
        for case in a.cases.split(','):
            for threads in ([1, 2] if case == 'concurrent' else [1]):
                key = f'{case}/{n}/threads={threads}'
                rows = []
                out['results'][key] = dict(affinity=cpus[:threads], threads=threads, raw=rows)
                for i in range(a.pairs):
                    pair = dict(order=['baseline', 'candidate'] if i % 2 == 0 else ['candidate', 'baseline'])
                    for revision in pair['order']:
                        command = ['taskset', '-c', ','.join(map(str, cpus[:threads])), str(paths[revision]), case, str(n), str(a.samples)]
                        r = subprocess.run(command, env=env | {'RAYON_NUM_THREADS': str(threads)}, text=True, capture_output=True, check=True)
                        pair[revision] = [float(row['seconds']) for row in csv.DictReader(io.StringIO(r.stdout))]
                        assert len(pair[revision]) == a.samples
                    rows.append(pair)
                    a.output.write_text(json.dumps(out, indent=2) + '\n')
                out['results'][key] = dict(affinity=cpus[:threads], threads=threads, raw=rows, summary=summarize(rows))
                a.output.write_text(json.dumps(out, indent=2) + '\n')
                print(key, out['results'][key]['summary'], flush=True)


if __name__ == '__main__':
    main()
