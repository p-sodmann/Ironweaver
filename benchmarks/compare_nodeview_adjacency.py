"""Compare native NodeView queries with equivalent Python edge-snapshot queries.

Run against an optimized build: python benchmarks/compare_nodeview_adjacency.py
Outputs JSON; alternates baseline/candidate order and checks equal results."""
import json
import statistics
import sys
import timeit
from ironweaver import NodeView, Vertex

results = {'python': sys.version, 'measurements': []}
for size in [8, 1000]:
    g = Vertex()
    g.add_nodes(['hub', 'absent', *[str(i) for i in range(size)]])
    g.add_edges([('hub', str(i)) for i in range(size)])
    g.add_edges([(str(i), 'hub') for i in range(size)])
    node = g['hub']
    view = NodeView(node)
    checks = {
        'degree': (lambda: len(node.edges), lambda: view.degree),
        'in_degree': (lambda: len(node.inverse_edges), lambda: view.in_degree),
        'has_edge_to/first': (lambda: any(e.to_node.id == '0' for e in node.edges), lambda: view.has_edge_to('0')),
        'has_edge_to/miss': (lambda: any(e.to_node.id == 'absent' for e in node.edges), lambda: view.has_edge_to('absent')),
        'has_edge_from/miss': (lambda: any(e.from_node.id == 'absent' for e in node.inverse_edges), lambda: view.has_edge_from('absent')),
        'neighbor_ids': (lambda: {e.to_node.id for e in node.edges}, lambda: view.neighbor_ids),
    }
    for name, (before, after) in checks.items():
        assert before() == after()
        samples = [[], []]
        for repeat in range(7):
            for which in ([0, 1] if repeat % 2 == 0 else [1, 0]):
                fn = [before, after][which]
                samples[which].append(timeit.timeit(fn, number=300) / 300)
        a, b = [statistics.median(sample) for sample in samples]
        results['measurements'].append({'query': name, 'degree': size, 'before_us': a*1e6, 'after_us': b*1e6, 'speedup': a/b})
print(json.dumps(results, indent=2))
