"""Run all applicable existing Ironweaver workloads using frozen extensions.

Each invocation is one independent process. The orchestrator alternates artifact
order, pins CPUs and fixes thread counts. Keep all observations, not best times.
NetworkX calls in compare_networkx are run once for its existing correctness
assertions; only Ironweaver calls are sampled. Graph construction and projection
setup are excluded where the upstream workload excludes them.
"""
from __future__ import annotations
import argparse
import gc
import inspect
import json
import linecache
import os
from pathlib import Path
import pickle
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))


def sample(fn, setup=None, seconds=0.12):
    # Expensive operations get one measured call per independent process.
    # Otherwise use the pilot as a warm-up, then retain all timed calls.
    arg = None if setup is None else setup()
    gc.collect()
    gc.disable()
    try:
        t = time.perf_counter()
        result = fn() if setup is None else fn(arg)
        pilot = time.perf_counter() - t
    finally:
        gc.enable()
    if pilot >= 1.0:
        return pilot, result, [pilot]
    del result
    elapsed, count, values = 0.0, 0, []
    gc.collect()
    while count < 64 and (elapsed < seconds or count < 3):
        arg = None if setup is None else setup()
        gc.disable()
        try:
            t = time.perf_counter()
            result = fn() if setup is None else fn(arg)
            dt = time.perf_counter() - t
        finally:
            gc.enable()
        values.append(dt)
        elapsed += dt
        count += 1
        if elapsed < seconds and count < 64:
            del result
    return elapsed / count, result, values


def emit(name, value, samples=None, unit="seconds"):
    print(json.dumps(dict(name=name, value=value, samples=samples, unit=unit)), flush=True)


def networkx_suite():
    import compare_networkx as b
    for n, e in b.parse_sizes(os.environ.get("IW_BENCH_SIZES", "1000:5000,20000:100000")):
        operation = [0]
        def timed(fn, repeats, setup=None):
            frame = inspect.currentframe().f_back
            line = linecache.getline(frame.f_code.co_filename, frame.f_lineno).strip()
            if line.startswith(('t_iw,', 't_loop,')):
                operation[0] += 1
                t, r, values = sample(fn, setup)
                emit(f"networkx/{n}/{operation[0]:02d}", t, values)
                return t, r
            t = time.perf_counter()
            r = fn() if setup is None else fn(setup())
            return time.perf_counter() - t, r
        b.best_of = timed
        report = b.run_size(n, e, 1, 42)
        # Workload labels are attached after the run; batch loop is a separate control.
        names = []
        for row in report.results:
            names.append(row.name)
            if row.name == 'Batch shortest paths':
                names.append('Loop shortest paths (control)')
        emit(f"networkx/{n}/labels", names, unit="labels")


def libraries_suite(data):
    import compare_libraries as b
    for key in os.environ.get('IW_BENCH_DATASETS', 'facebook,github,kron').split(','):
        with open(data / f'{key}.pickle', 'rb') as f:
            ds = pickle.load(f)
        t, g, values = sample(lambda: b.build_ironweaver(ds))
        emit(f"libraries/{key}/Build graph", t, values)
        reused = b.Projections(g)
        for name, make, check, directed_only in b.OPS:
            if directed_only and not ds.directed:
                continue
            if name == 'Betweenness (exact)' and ds.n * len(ds.edges) > b.BETWEENNESS_BUDGET:
                emit(f"libraries/{key}/{name}", 'upstream nodes*edges budget exceeded', unit="skip")
                continue
            fn = make()['ironweaver']
            for mode, graph in [('one-off', g), ('reused', reused)]:
                t, r, values = sample(lambda: fn(graph, ds))
                emit(f"libraries/{key}/{mode}/{name}", t, values)
                del r
        b._UNDIRECTED.clear()
        del g, reused, ds
        gc.collect()


def memory_suite(data):
    import compare_networkx_memory as b
    for n, e in [(10000, 50000), (100000, 500000)]:
        path = str(data / f'graph-{n}.json')
        for scenario in ['graph', 'graph_no_attrs', 'load_json', 'save_json']:
            use_path = path if scenario == 'load_json' else str(data / f'out-{n}.json')
            r = b.measure('ironweaver', scenario, n, e, 42, use_path)
            assert r.get('check') in (None, n), r
            emit(f"memory/{n}/{scenario}", r.get('bytes'), unit="bytes")
            if 'resident' in r:
                emit(f"memory/{n}/{scenario}/resident", r['resident'], unit="bytes")
        g = b.build_ironweaver(n, e, 42)
        for kind, save in [('json', b.save_json), ('binary', b.save_binary)]:
            out = str(data / f'out-{n}.{kind}')
            save('ironweaver', g, out)
            emit(f"files/{n}/{kind}", os.path.getsize(out), unit="bytes")
        del g


def prepare(data):
    import compare_libraries as b
    import compare_networkx_memory as m
    data.mkdir(parents=True, exist_ok=True)
    for key, make in b.DATASETS.items():
        path = data / f'{key}.pickle'
        if not path.exists() or path.stat().st_size == 0:
            ds = make()
            with open(path.with_suffix('.part'), 'wb') as f:
                pickle.dump(ds, f)
            path.with_suffix('.part').replace(path)
    for n, e in [(10000, 50000), (100000, 500000)]:
        path = data / f'graph-{n}.json'
        if not path.exists():
            g = m.build_ironweaver(n, e, 42)
            m.save_json('ironweaver', g, str(path))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('suite', choices=['prepare', 'networkx', 'libraries', 'memory'])
    parser.add_argument('--data', type=Path, default=Path('work/data'))
    args = parser.parse_args()
    if args.suite == 'prepare':
        prepare(args.data)
    elif args.suite == 'networkx':
        networkx_suite()
    elif args.suite == 'libraries':
        libraries_suite(args.data)
    else:
        memory_suite(args.data)
