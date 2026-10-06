"""Run existing Ironweaver benchmark workloads against two local builds.

Both revisions must be built with the same compiler, dependencies and flags.
Pass directories containing their ironweaver/ Python packages (including the
compiled extension). Workers execute serially on this machine with identical
CPU affinity and thread counts. Each operation is interleaved between the two
workers; barriers prevent concurrent timing or preparation. Historical reports from other machines are
never used as a baseline. Example:

 python benchmarks/compare_revisions.py /tmp/base/python /tmp/new/python \
     --pairs 6 --output /tmp/comparison.json

The networkx suite retains its reference checks. The library suite runs the
Ironweaver fresh/reused-projection workloads, not unrelated competitors.
Memory workloads retain their fresh-process measurements and file sizes.
Default dataset sizes and existing applicability/betweenness limits are kept;
every excluded workload is recorded. No timing assertions belong in CI.
"""
from __future__ import annotations

import argparse
import gc
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import pickle
import random
import statistics
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
CASES = ["networkx:1000:5000", "networkx:20000:100000",
         "libraries:facebook", "libraries:github", "libraries:kron",
         "memory:10000:50000", "memory:100000:500000"]


def identity():
    boot = Path('/proc/sys/kernel/random/boot_id').read_bytes()
    return {"boot_sha256": hashlib.sha256(boot).hexdigest(),
            "platform": platform.platform(), "python": sys.version,
            "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "affinity": sorted(os.sched_getaffinity(0)),
            "rayon_threads": os.environ['RAYON_NUM_THREADS'],
            "dependencies": {p: importlib.metadata.version(p) for p in
                             ('networkx', 'numpy', 'scipy', 'psutil', 'maturin')}}


SYNCHRONIZED = False


def barrier(event, name, expected):
    if SYNCHRONIZED:
        print(json.dumps({'event': event, 'operation': name}), flush=True)
        if sys.stdin.readline().strip() != expected:
            raise RuntimeError('benchmark controller stopped or protocol mismatch')


def measure(fn, repeats, setup=None, disable_gc=False, name=None, warmups=1):
    name = name or str(fn.__code__.co_firstlineno)
    barrier('ready', name, 'RUN')
    samples = []
    result = None
    for i in range(repeats + warmups):
        arg = setup() if setup is not None else None
        # Do not charge destruction of the previous output to the next call.
        result = None
        gc.collect()
        if disable_gc:
            gc.disable()
        try:
            start = time.perf_counter_ns()
            result = fn(arg) if setup is not None else fn()
            elapsed = (time.perf_counter_ns() - start) / 1e9
        finally:
            if disable_gc:
                gc.enable()
        if i >= warmups:
            samples.append(elapsed)
    barrier('done', name, 'NEXT')
    return statistics.median(samples), result, samples


def worker(case, repeats):
    import ironweaver
    import ironweaver._ironweaver as extension
    build_path = Path(ironweaver.__file__).parent.parent / 'build.json'
    output = {"identity": identity(), "package": ironweaver.__file__,
              "build": json.loads(build_path.read_text()) if build_path.exists() else None,
              "extension_sha256": hashlib.sha256(Path(extension.__file__).read_bytes()).hexdigest(),
              "metrics": {}, "excluded": []}
    suite, *parts = case.split(':')
    if suite == 'networkx':
        import compare_networkx as bench
        records = []
        cache = os.environ.get('IRONWEAVER_REFERENCE_CACHE')
        reference_names = {'nx', 'build_networkx', 'nx_remove', 'remove_nodes_from', 'nx_random_walks', 'number_of_edges'}
        context = json.dumps([identity(), case, hashlib.sha256(Path(bench.__file__).read_bytes()).hexdigest()], sort_keys=True)
        def timer(fn, count, setup=None):
            line = fn.__code__.co_firstlineno
            if cache and reference_names.intersection(fn.__code__.co_names):
                key = hashlib.sha256((context + str(line)).encode()).hexdigest()
                path = Path(cache) / (key + '.pickle')
                if path.exists():
                    with path.open('rb') as f:
                        result = pickle.load(f)  # only locally generated reference results
                else:
                    arg = setup() if setup is not None else None
                    result = fn(arg) if setup is not None else fn()
                    with path.open('wb') as f:
                        pickle.dump(result, f, protocol=pickle.HIGHEST_PROTOCOL)
                return 0.0, result
            median, result, samples = measure(fn, count, setup=setup)
            records.append((median, samples, line))
            return median, result
        bench.best_of = timer
        report = bench.run_size(*map(int, parts), repeats, 42)
        for row in report.results:
            samples = next(samples for median, samples, _ in records if median is row.ironweaver_s)
            output['metrics'][row.name] = {"unit": "seconds", "samples": samples}
        loop_line = next(i for i, line in enumerate(Path(bench.__file__).read_text().splitlines(), 1)
                         if 't_loop, _ = best_of' in line)
        output['metrics']['Batch paths / individual calls'] = {
            'unit': 'seconds', 'samples': next(samples for _, samples, line in records if line == loop_line)}
        output['reference_checks'] = 'all original run_size assertions completed'
    elif suite == 'libraries':
        import compare_libraries as bench
        ds = bench.DATASETS[parts[0]]()
        output['dataset'] = {'nodes': ds.n, 'edges': len(ds.edges),
                             'sha256': hashlib.sha256(repr(ds.edges).encode()).hexdigest()}
        _, graph, samples = measure(lambda: bench.build_ironweaver(ds), repeats, disable_gc=True, name='Build graph')
        output['metrics']['Build graph'] = {'unit': 'seconds', 'samples': samples}
        reused = bench.Projections(graph)
        for name, make, check, directed_only in bench.OPS:
            if directed_only and not ds.directed:
                output['excluded'].append([name, 'directed-only operation on undirected dataset'])
                continue
            if name == 'Betweenness (exact)' and ds.n * len(ds.edges) > bench.BETWEENNESS_BUDGET:
                output['excluded'].append([name, 'existing nodes*edges > 5e9 budget'])
                continue
            fn = make()['ironweaver']
            # Match the existing suite's one-run limit for exact betweenness.
            # Earlier algorithms have already initialized the runtime/pool.
            count = 1 if name == 'Betweenness (exact)' else repeats
            warmups = 0 if name == 'Betweenness (exact)' else 1
            _, fresh, samples = measure(lambda: fn(graph, ds), count, disable_gc=True,
                                       name=name + ' / fresh', warmups=warmups)
            output['metrics'][name + ' / fresh'] = {'unit': 'seconds', 'samples': samples}
            _, cached, samples = measure(lambda: fn(reused, ds), count, disable_gc=True,
                                        name=name + ' / reused', warmups=warmups)
            output['metrics'][name + ' / reused'] = {'unit': 'seconds', 'samples': samples}
            if check is not None:
                symbol, note = check(fresh, cached)
                assert symbol in ('=', '≈'), (name, symbol, note)
            else:
                assert math.isclose(bench.modularity(ds, fresh), bench.modularity(ds, cached), abs_tol=1e-9)
    elif suite == 'memory':
        import compare_networkx_memory as bench
        nodes, edges = map(int, parts)
        with tempfile.TemporaryDirectory(prefix='ironweaver-memory-') as folder:
            graph = bench.build_ironweaver(nodes, edges, 42)
            json_path = str(Path(folder) / 'input.json')
            bin_path = str(Path(folder) / 'input.bin')
            bench.save_json('ironweaver', graph, json_path)
            bench.save_binary('ironweaver', graph, bin_path)
            for name, path in [('JSON file', json_path), ('Binary file', bin_path)]:
                output['metrics'][name] = {'unit': 'bytes', 'samples': [Path(path).stat().st_size]}
            del graph
            bench.settle()
            for scenario in ('graph', 'graph_no_attrs', 'load_json', 'save_json'):
                path = json_path if scenario == 'load_json' else str(Path(folder) / 'output.json')
                barrier('ready', scenario, 'RUN')
                runs = [bench.measure('ironweaver', scenario, nodes, edges, 42, path) for _ in range(repeats)]
                barrier('done', scenario, 'NEXT')
                assert all(r.get('bytes') is not None and r.get('check') == nodes for r in runs), runs
                output['metrics'][scenario] = {'unit': 'bytes', 'samples': [r['bytes'] for r in runs]}
    else:
        raise ValueError(case)
    return output


def summarize(pairs):
    result = {}
    keys = pairs[0]['baseline']['metrics'].keys()
    for pair in pairs:
        assert pair['baseline']['metrics'].keys() == pair['candidate']['metrics'].keys() == keys
        assert pair['baseline']['excluded'] == pair['candidate']['excluded']
        assert pair['baseline'].get('dataset') == pair['candidate'].get('dataset')
    for key in keys:
        before = [statistics.median(p['baseline']['metrics'][key]['samples']) for p in pairs]
        after = [statistics.median(p['candidate']['metrics'][key]['samples']) for p in pairs]
        entry = {'unit': pairs[0]['baseline']['metrics'][key]['unit'],
                 'baseline_median': statistics.median(before), 'candidate_median': statistics.median(after)}
        if min(before + after) > 0:
            logs = [math.log(a / b) for a, b in zip(before, after)]
            rng = random.Random(42)
            boot = sorted(math.exp(statistics.mean(rng.choices(logs, k=len(logs)))) for _ in range(10000))
            entry.update(ratio=math.exp(statistics.mean(logs)), ci95=[boot[250], boot[9749]])
            entry['regression_over_5pct'] = boot[9749] < 1 / 1.05
        result[key] = entry
    return result


def run_pair(args, case, packages, env, pair_id):
    pair = {'order': ['baseline', 'candidate'] if pair_id % 2 == 0 else ['candidate', 'baseline']}
    with tempfile.TemporaryDirectory(prefix='ironweaver-pair-') as folder:
        processes, paths, logs = {}, {}, {}
        try:
            for revision in pair['order']:
                paths[revision] = Path(folder) / (revision + '.json')
                logs[revision] = (Path(folder) / (revision + '.log')).open('w+')
                command = [sys.executable, str(Path(__file__).resolve()), str(args.baseline), str(args.candidate),
                           '--worker', case, '--synchronized', '--repeats', str(args.repeats),
                           '--output', str(paths[revision])]
                processes[revision] = subprocess.Popen(command, env=env | {'PYTHONPATH': str(packages[revision])},
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=logs[revision], text=True, bufsize=1)
            while True:
                ready = {}
                for revision, process in processes.items():
                    line = process.stdout.readline()
                    if not line:
                        raise RuntimeError(f'{revision} worker terminated unexpectedly')
                    ready[revision] = json.loads(line)
                assert ready['baseline'] == ready['candidate'], ready
                if ready['baseline']['event'] == 'finished':
                    break
                assert ready['baseline']['event'] == 'ready', ready
                for revision in pair['order']:
                    process = processes[revision]
                    process.stdin.write('RUN\n'); process.stdin.flush()
                    done = json.loads(process.stdout.readline())
                    assert done == {'event': 'done', 'operation': ready[revision]['operation']}, done
                # Neither worker may prepare the next operation while the
                # other revision is being timed. Release both only now.
                for process in processes.values():
                    process.stdin.write('NEXT\n'); process.stdin.flush()
            for revision, process in processes.items():
                assert process.wait() == 0
                pair[revision] = json.loads(paths[revision].read_text())
        except BaseException:
            for process in processes.values():
                if process.poll() is None:
                    process.terminate()
                process.wait()
            for revision, log in logs.items():
                log.seek(0)
                print(revision, log.read(), file=sys.stderr)
            raise
        finally:
            for log in logs.values():
                log.close()
    return pair


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline', type=Path)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('--pairs', type=int, default=6)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--worker')
    parser.add_argument('--reference-cache', type=Path)
    parser.add_argument('--synchronized', action='store_true')
    args = parser.parse_args()
    if args.worker:
        global SYNCHRONIZED
        SYNCHRONIZED = args.synchronized
        args.output.write_text(json.dumps(worker(args.worker, args.repeats), indent=2) + '\n')
        if SYNCHRONIZED:
            print(json.dumps({'event': 'finished'}), flush=True)
        return
    if args.pairs < 1 or args.repeats < 1:
        parser.error('pairs and repeats must be positive')
    env = os.environ.copy()
    if args.reference_cache:
        args.reference_cache.mkdir(parents=True, exist_ok=True)
        env['IRONWEAVER_REFERENCE_CACHE'] = str(args.reference_cache.resolve())
    env.update(PYTHONHASHSEED='42', RAYON_NUM_THREADS=str(len(os.sched_getaffinity(0))),
               OMP_NUM_THREADS='1', OPENBLAS_NUM_THREADS='1')
    packages = {'baseline': args.baseline.resolve(), 'candidate': args.candidate.resolve()}
    for path in packages.values():
        assert (path / 'ironweaver').is_dir(), path
    cases = args.cases.split(',')
    report = {'pairs': args.pairs, 'repeats': args.repeats, 'cases': {case: [] for case in cases}}
    expected_identity = None
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.reference_cache:
        # Prime the NetworkX oracles before collecting any paired timings.
        # These are untimed reference computations, never Ironweaver workloads.
        for case in cases:
            if case.startswith('networkx:'):
                print('prepare reference checks', case, flush=True)
                with tempfile.TemporaryDirectory(prefix='ironweaver-reference-') as folder:
                    subprocess.run([sys.executable, str(Path(__file__).resolve()), str(args.baseline), str(args.candidate),
                                    '--worker', case, '--repeats', '1', '--output', str(Path(folder) / 'result.json')],
                                   env=env | {'PYTHONPATH': str(packages['baseline'])}, check=True)
    for pair_id in range(args.pairs):
        # Reverse case order too, rather than always measuring one case late.
        for case in (cases if pair_id % 2 == 0 else list(reversed(cases))):
            print(f'pair {pair_id + 1}/{args.pairs} {case}: interleaved operations', flush=True)
            pair = run_pair(args, case, packages, env, pair_id)
            for revision in pair['order']:
                run = pair[revision]
                assert Path(run['package']).is_relative_to(packages[revision]), run['package']
                if expected_identity is None:
                    expected_identity = run['identity']
                assert run['identity'] == expected_identity, 'machine/runtime/affinity changed; comparison invalid'
            builds = [pair[r]['build'] for r in ('baseline', 'candidate')]
            if any(builds):
                assert all(builds), 'both builds need matching metadata'
                for key in ('rustc', 'cargo_lock_sha256', 'flags', 'bindings_sha256'):
                    assert builds[0][key] == builds[1][key], ('build mismatch', key)
            report['cases'][case].append(pair)
            args.output.write_text(json.dumps(report, indent=2) + '\n')
    report['summary'] = {case: summarize(pairs) for case, pairs in report['cases'].items()}
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    for case, metrics in report['summary'].items():
        for key, metric in metrics.items():
            if metric.get('regression_over_5pct'):
                print('REGRESSION', case, key, metric, flush=True)


if __name__ == '__main__':
    main()
