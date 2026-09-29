"""The LDBC Graphalytics validation suite: BFS, WCC, CDLP, LCC, PageRank
and SSSP on the benchmark's validation graphs, against its reference
outputs (tests/data/graphalytics, from github.com/ldbc/ldbc_graphalytics),
with the parameters of its validation tests."""

import math
import os

import pytest

from ironweaver import Vertex

DATA = os.path.join(os.path.dirname(__file__), "data", "graphalytics")
UNREACHABLE = 9223372036854775807


def adjacency_graph(name, directed):
    """A vertex-based input: `id neighbour ...` per line. Duplicate edges
    (and, undirected, the two directions) are one edge."""
    nodes, edges = set(), set()
    with open(os.path.join(DATA, name)) as fh:
        for line in fh:
            ids = line.split()
            if not ids:
                continue
            nodes.update(ids)
            for other in ids[1:]:
                a, b = ids[0], other
                if not directed and int(b) < int(a):
                    a, b = b, a
                edges.add((a, b))
    return build(nodes, {e: 1.0 for e in edges})


def edge_file_graph(name):
    with open(os.path.join(DATA, name + ".v")) as fh:
        nodes = {line.strip() for line in fh if line.strip()}
    weights = {}
    with open(os.path.join(DATA, name + ".e")) as fh:
        for line in fh:
            if line.strip():
                a, b, w = line.split()
                weights[(a, b)] = float(w)
    return build(nodes, weights)


def build(nodes, weights):
    # Nodes in id order: CDLP breaks ties by the smallest label (vertex id),
    # ironweaver by projection order
    g = Vertex()
    for id in sorted(nodes, key=int):
        g.add_node(id)
    for (a, b), w in sorted(weights.items(), key=lambda kv: (int(kv[0][0]), int(kv[0][1]))):
        g.add_edge(a, b, {"weight": w})
    return g


def reference(name, parse=float):
    with open(os.path.join(DATA, name)) as fh:
        return {line.split()[0]: parse(line.split()[1]) for line in fh if line.strip()}


def partition_of(labels):
    groups = {}
    for id, label in labels.items():
        groups.setdefault(label, set()).add(id)
    return sorted(map(sorted, groups.values()))


def project(g, directed, **kw):
    return g.project(direction="out" if directed else "both", **kw)


# (case, graph loader, reference file, parameters)
EXAMPLE_D = ("example-directed", lambda: edge_file_graph("example/example-directed"), True)
EXAMPLE_U = ("example-undirected", lambda: edge_file_graph("example/example-undirected"), False)


def cases(algorithm, dir_params, undir_params, example_params):
    out = [
        pytest.param(lambda: adjacency_graph(f"{algorithm}/dir-input", True), True, f"{algorithm}/dir-output",
                     dir_params, id="dir"),
        pytest.param(lambda: adjacency_graph(f"{algorithm}/undir-input", False), False, f"{algorithm}/undir-output",
                     undir_params, id="undir"),
    ]
    for name, loader, directed in (EXAMPLE_D, EXAMPLE_U):
        out.append(pytest.param(loader, directed, f"example/{name}-{algorithm.upper()}", example_params[directed],
                                id=name))
    return out


@pytest.mark.parametrize("load, directed, ref, params",
                         cases("bfs", {"source": "1"}, {"source": "1"}, {True: {"source": "1"}, False: {"source": "2"}}))
def test_bfs(load, directed, ref, params):
    got = project(load(), directed).bfs_levels([params["source"]])
    want = reference(ref, int)
    assert {id: got.get(id, UNREACHABLE) for id in want} == want


@pytest.mark.parametrize("load, directed, ref, params", cases("wcc", {}, {}, {True: {}, False: {}}))
def test_wcc(load, directed, ref, params):
    got = project(load(), directed).weakly_connected_components()
    assert sorted(map(sorted, got)) == partition_of(reference(ref, int))


@pytest.mark.parametrize("load, directed, ref, params",
                         cases("cdlp", {"iterations": 5}, {"iterations": 5},
                               {True: {"iterations": 2}, False: {"iterations": 2}}))
def test_cdlp(load, directed, ref, params):
    got = project(load(), directed).label_propagation(max_iter=params["iterations"])
    assert sorted(map(sorted, got)) == partition_of(reference(ref, int))


@pytest.mark.parametrize("load, directed, ref, params", cases("lcc", {}, {}, {True: {}, False: {}}))
def test_lcc(load, directed, ref, params):
    got = project(load(), directed).clustering(directed=directed)
    want = reference(ref)
    assert {id: got[id] for id in want} == pytest.approx(want, abs=1e-6)


@pytest.mark.parametrize("load, directed, ref, params",
                         cases("pr", {"iterations": 14}, {"iterations": 26},
                               {True: {"iterations": 2}, False: {"iterations": 2}}))
def test_pagerank(load, directed, ref, params):
    got = project(load(), directed).pagerank(0.85, max_iter=params["iterations"], tol=0)
    want = reference(ref)
    # The reference values carry ~1e-6 relative error (lower-precision
    # arithmetic); Graphalytics itself accepts deviations up to 1e-4
    assert {id: got[id] for id in want} == pytest.approx(want, rel=1e-5, abs=1e-12)


SSSP = [
    pytest.param(lambda: edge_file_graph("sssp/dir-input"), True, "sssp/dir-output", "1", id="dir"),
    pytest.param(lambda: edge_file_graph("sssp/undir-input"), False, "sssp/undir-output", "1", id="undir"),
    pytest.param(EXAMPLE_D[1], True, "example/example-directed-SSSP", "1", id="example-directed"),
    pytest.param(EXAMPLE_U[1], False, "example/example-undirected-SSSP", "2", id="example-undirected"),
]


@pytest.mark.parametrize("load, directed, ref, source", SSSP)
def test_sssp(load, directed, ref, source):
    got = project(load(), directed, weight="weight").distances([source])[source]
    want = reference(ref)
    got = {id: got.get(id, math.inf) for id in want}
    assert got == pytest.approx(want, abs=1e-9)
