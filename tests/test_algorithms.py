"""Projection analytics (components, DAGs, centrality, structure,
communities, BFS) compared with networkx on random multigraphs."""

import random

import pytest

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

nx = pytest.importorskip("networkx")

SEEDS = range(6)
DIRECTIONS = ["out", "in", "both"]


def random_graphs(seed, n=80, m=240, dag=False):
    """The same random multigraph (self-loops, parallel edges) as an
    ironweaver Vertex and a networkx MultiDiGraph."""
    rng = random.Random(seed)
    g, h = Vertex(), nx.MultiDiGraph()
    for i in range(n):
        g.add_node(f"n{i}")
        h.add_node(f"n{i}")
    for _ in range(m):
        a, b = rng.randrange(n), rng.randrange(n)
        if dag:
            if a == b:
                continue
            a, b = min(a, b), max(a, b)
        w = rng.uniform(0.5, 5)
        g.add_edge(f"n{a}", f"n{b}", {"weight": w})
        h.add_edge(f"n{a}", f"n{b}", weight=w)
    return g, h


def oriented(h, direction):
    """The networkx MultiDiGraph a projection with `direction` corresponds
    to ("both": every edge in both directions; networkx's own
    `to_undirected` would merge u->v and v->u edges that share a key)."""
    if direction == "in":
        return h.reverse(copy=True)
    if direction == "both":
        o = nx.MultiDiGraph()
        o.add_nodes_from(h)
        for a, b, d in h.edges(data=True):
            o.add_edge(a, b, **d)
            o.add_edge(b, a, **d)
        return o
    return h


def simple_undirected(h):
    s = nx.Graph(h.to_undirected())
    s.remove_edges_from(list(nx.selfloop_edges(s)))
    return s


def as_sets(groups):
    return sorted(map(frozenset, groups), key=sorted)


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
def test_components(seed, direction):
    g, h = random_graphs(seed)
    p = g.project(direction=direction)
    weak = p.weakly_connected_components()
    strong = p.strongly_connected_components()
    assert as_sets(weak) == as_sets(nx.weakly_connected_components(h))
    assert as_sets(strong) == as_sets(nx.strongly_connected_components(oriented(h, direction)))
    # Largest first, members in projection order
    order = {id: i for i, id in enumerate(p.ids())}
    for groups in (weak, strong):
        assert [len(c) for c in groups] == sorted((len(c) for c in groups), reverse=True)
        assert all(c == sorted(c, key=order.__getitem__) for c in groups)


@pytest.mark.parametrize("seed", SEEDS)
def test_topological_sort_and_cycles(seed):
    g, h = random_graphs(seed, dag=True)
    p = g.project()
    order = {id: i for i, id in enumerate(p.ids())}
    assert p.topological_sort() == list(nx.lexicographical_topological_sort(h, key=order.__getitem__))
    assert p.find_cycle() is None
    rev = g.project(direction="in").topological_sort()
    assert rev == list(nx.lexicographical_topological_sort(h.reverse(), key=order.__getitem__))

    g2, h2 = random_graphs(seed)
    p2 = g2.project()
    cycle = p2.find_cycle()
    assert cycle is not None and len(set(cycle)) == len(cycle)
    for a, b in zip(cycle, cycle[1:] + cycle[:1]):
        assert h2.has_edge(a, b)
    with pytest.raises(ValueError, match="cycle"):
        p2.topological_sort()


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
def test_degree_centrality(seed, direction):
    g, h = random_graphs(seed)
    p = g.project(direction=direction)
    o = oriented(h, direction)
    assert p.degree_centrality() == pytest.approx(nx.out_degree_centrality(o))
    assert p.degree_centrality("in") == pytest.approx(nx.in_degree_centrality(o))
    if direction == "both":  # in + out of the original graph
        assert p.degree_centrality() == pytest.approx(nx.degree_centrality(h))


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
@pytest.mark.parametrize("weighted", [False, True])
def test_pagerank(seed, direction, weighted):
    pytest.importorskip("scipy")  # networkx.pagerank needs it
    g, h = random_graphs(seed)
    o = oriented(h, direction)
    weight = "weight" if weighted else None
    p = g.project(weight=weight, direction=direction)
    got = p.pagerank()
    want = nx.pagerank(o, weight=weight)
    assert got == pytest.approx(want, abs=1e-6)

    pers = {"n1": 2.0, "n5": 1.0, "n9": 0.5}
    got = p.pagerank(alpha=0.7, personalization=pers)
    want = nx.pagerank(o, alpha=0.7, personalization=pers, weight=weight)
    assert got == pytest.approx(want, abs=1e-6)


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
def test_triangles_clustering_cores(seed, direction):
    g, h = random_graphs(seed, n=60, m=300)
    p = g.project(direction=direction)
    s = simple_undirected(h)
    assert p.triangles() == nx.triangles(s)
    assert p.clustering() == pytest.approx(nx.clustering(s))
    assert p.core_number() == nx.core_number(s)


def cdlp_reference(p, max_iter):
    """Synchronous label propagation in pure Python (smallest label wins ties)."""
    ids = p.ids()
    index = {id: i for i, id in enumerate(ids)}
    nbrs = {id: set(p.neighbors(id)) | set(p.neighbors(id, "in")) for id in ids}
    for id in ids:
        nbrs[id].discard(id)
    labels = {id: index[id] for id in ids}
    for _ in range(max_iter):
        new = {}
        for id in ids:
            if not nbrs[id]:
                new[id] = labels[id]
                continue
            counts = {}
            for n in nbrs[id]:
                counts[labels[n]] = counts.get(labels[n], 0) + 1
            best = max(counts.values())
            new[id] = min(label for label, c in counts.items() if c == best)
        if new == labels:
            break
        labels = new
    groups = {}
    for id in ids:
        groups.setdefault(labels[id], []).append(id)
    return as_sets(groups.values())


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
def test_label_propagation(seed, direction):
    g, _ = random_graphs(seed, n=60, m=150)
    p = g.project(direction=direction)
    got = p.label_propagation(max_iter=15)
    assert as_sets(got) == cdlp_reference(p, 15)
    assert sum(map(len, got)) == 60


def test_label_propagation_finds_cliques():
    # Two 5-cliques joined by a single edge
    g = Vertex()
    cliques = [[f"{base}{i}" for i in range(5)] for base in "ab"]
    for clique in cliques:
        for id in clique:
            g.add_node(id)
        for i, a in enumerate(clique):
            for b in clique[i + 1:]:
                g.add_edge(a, b)
    g.add_edge("a4", "b0")
    assert as_sets(g.project().label_propagation()) == as_sets(cliques)


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
def test_bfs_levels(seed, direction):
    g, h = random_graphs(seed, n=2000, m=12000)
    o = oriented(h, direction)
    p = g.project(direction=direction)
    assert p.bfs_levels(["n0"]) == nx.single_source_shortest_path_length(o, "n0")
    assert p.bfs_levels(["n0"], max_depth=2) == nx.single_source_shortest_path_length(o, "n0", cutoff=2)
    multi = nx.multi_source_dijkstra_path_length(o, {"n1", "n2", "n3"}, weight=lambda *_: 1)
    assert p.bfs_levels(["n1", "n2", "n3", "n2"]) == multi


def test_errors_and_edge_cases():
    g, _ = random_graphs(0)
    p = g.project()
    with pytest.raises(ValueError, match="alpha"):
        p.pagerank(alpha=2)
    with pytest.raises(ValueError, match="Node with id 'zz' not found"):
        p.pagerank(personalization={"zz": 1.0})
    with pytest.raises(ValueError, match="all be zero"):
        p.pagerank(personalization={"n1": 0.0})
    with pytest.raises(ValueError, match="did not converge"):
        p.pagerank(max_iter=1)
    with pytest.raises(ValueError, match="Root node with id 'zz'"):
        p.bfs_levels(["zz"])
    with pytest.raises(ValueError, match="direction"):
        p.degree_centrality("both")

    empty = Vertex().project()
    assert empty.weakly_connected_components() == []
    assert empty.pagerank() == {}
    assert empty.topological_sort() == []
    assert empty.find_cycle() is None
    assert empty.label_propagation() == []

    one = Vertex()
    one.add_node("x")
    one.add_edge("x", "x")
    q = one.project()
    assert q.degree_centrality() == {"x": 1.0}
    assert q.find_cycle() == ["x"]
    assert (q.triangles(), q.core_number(), q.clustering()) == ({"x": 0}, {"x": 0}, {"x": 0.0})
