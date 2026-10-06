"""Independently repeat workloads flagged by the complete revision screen.

Uses the same frozen artifacts, datasets, synchronization and paired statistics
as compare_revisions.py. This targeted repeat supplements the full suite; it
does not replace it. It includes all screen metrics whose 95% speedup interval
was below 1, even when the interval did not establish a >5% slowdown.
"""
from pathlib import Path
import hashlib
import json

import compare_revisions as runner

HARNESS = Path(runner.__file__)
original_identity = runner.identity


def identity():
    result = original_identity()
    result['comparison_harness_sha256'] = hashlib.sha256(HARNESS.read_bytes()).hexdigest()
    return result


def worker(case, repeats):
    import ironweaver
    import ironweaver._ironweaver as extension

    build_path = Path(ironweaver.__file__).parent.parent / 'build.json'
    output = {'identity': identity(), 'package': ironweaver.__file__,
              'build': json.loads(build_path.read_text()),
              'extension_sha256': hashlib.sha256(Path(extension.__file__).read_bytes()).hexdigest(),
              'metrics': {}, 'excluded': []}
    suite, dataset = case.split(':')
    if suite == 'confirm-networkx':
        import compare_networkx as bench

        n, m = (1000, 5000) if dataset == 'small' else (20000, 100000)
        edges = bench.make_edges(n, m, 42)
        output['dataset'] = {'sha256': hashlib.sha256(repr(edges).encode()).hexdigest()}
        graph = bench.build_ironweaver(n, edges)
        start = graph.get_node('n0')
        # Match the full suite's preceding full traversal, outside the timer.
        start.bfs()
        if dataset == 'large':
            name, fn = 'BFS (depth 3)', lambda: start.bfs(depth=3)
        else:
            name, fn = 'Most similar nodes', lambda: graph.project().most_similar(k=10)
        _, result, samples = runner.measure(fn, repeats, name=name)
        if dataset == 'large':
            reference = bench.build_networkx(n, edges)
            expected = ['n0'] + [v for _, v in bench.nx.bfs_edges(reference, 'n0', depth_limit=3)]
            assert set(result.keys()) == set(expected)
        else:
            assert len(result) == n
        output['metrics'][name] = {'unit': 'seconds', 'samples': samples}
    elif suite == 'confirm-libraries':
        import compare_libraries as bench

        ds = bench.DATASETS[dataset]()
        output['dataset'] = {'nodes': ds.n, 'edges': len(ds.edges),
                             'sha256': hashlib.sha256(repr(ds.edges).encode()).hexdigest()}
        graph = bench.build_ironweaver(ds)
        reused = bench.Projections(graph)
        selected = {'Minimum spanning forest (weight)': ['reused']} if dataset == 'facebook' else {
            'Dijkstra from one node (weighted)': ['reused'], 'Local clustering': ['fresh']}
        for name, make, _, _ in bench.OPS:
            if name not in selected:
                continue
            fn = make()['ironweaver']
            for kind in selected[name]:
                target = graph if kind == 'fresh' else reused
                _, _, samples = runner.measure(lambda: fn(target, ds), repeats,
                                                disable_gc=True, name=name + ' / ' + kind)
                output['metrics'][name + ' / ' + kind] = {'unit': 'seconds', 'samples': samples}
        assert len(output['metrics']) == sum(map(len, selected.values()))
    else:
        raise ValueError(case)
    return output


if __name__ == '__main__':
    # run_pair launches this file, while retaining the original controller.
    runner.__file__ = __file__
    runner.identity = identity
    runner.worker = worker
    runner.CASES = ['confirm-networkx:small', 'confirm-networkx:large',
                    'confirm-libraries:facebook', 'confirm-libraries:github']
    runner.main()
