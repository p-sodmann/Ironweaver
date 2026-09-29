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
    """Synchronous label propagation in pure Python, the LDBC CDLP rule:
    distinct in- and out-neighbours counted separately on a directed
    projection, distinct neighbours on an undirected one; smallest label
    wins ties."""
    ids = p.ids()
    index = {id: i for i, id in enumerate(ids)}
    nbrs = {}
    for id in ids:
        out = set(p.neighbors(id)) - {id}
        inc = set(p.neighbors(id, "in")) - {id}
        nbrs[id] = list(out) + (list(inc) if p.direction != "both" else [])
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


# --- Tier 2: centrality by shortest paths, similarity, communities, trees,
# --- embeddings, k shortest paths


def undirected(h):
    """h as an undirected multigraph, every edge kept (to_undirected would
    merge u->v and v->u edges that share a key)."""
    u = nx.MultiGraph()
    u.add_nodes_from(h)
    u.add_edges_from(h.edges(data=True))
    return u


def reference(h, direction):
    """The networkx graph matching a projection: undirected for "both"."""
    return undirected(h) if direction == "both" else oriented(h, direction)


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
@pytest.mark.parametrize("weighted", [False, True])
def test_betweenness(seed, direction, weighted):
    g, h = random_graphs(seed, n=60, m=150)
    o = reference(h, direction)
    weight = "weight" if weighted else None
    p = g.project(weight=weight, direction=direction)
    for normalized in (True, False):
        for endpoints in (False, True):
            got = p.betweenness_centrality(normalized=normalized, endpoints=endpoints)
            want = nx.betweenness_centrality(o, normalized=normalized, endpoints=endpoints, weight=weight)
            assert got == pytest.approx(want, rel=1e-9, abs=1e-12)
    # Sampling every node is exact; a sample is repeatable with a seed
    assert p.betweenness_centrality(k=60) == pytest.approx(p.betweenness_centrality())
    assert p.betweenness_centrality(k=10, seed=3) == p.betweenness_centrality(k=10, seed=3)
    if weighted:  # hop counts on a weighted projection
        assert p.betweenness_centrality(weighted=False) == pytest.approx(nx.betweenness_centrality(o))


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
@pytest.mark.parametrize("weighted", [False, True])
def test_closeness_and_harmonic(seed, direction, weighted):
    g, h = random_graphs(seed, n=60, m=120)
    o = reference(h, direction)
    weight = "weight" if weighted else None
    p = g.project(weight=weight, direction=direction)
    for wf in (True, False):
        want = nx.closeness_centrality(o, distance=weight, wf_improved=wf)
        assert p.closeness_centrality(wf_improved=wf) == pytest.approx(want, rel=1e-12)
    assert p.harmonic_centrality() == pytest.approx(nx.harmonic_centrality(o, distance=weight), rel=1e-12)


@pytest.mark.parametrize("seed", SEEDS)
def test_similarity(seed):
    g, h = random_graphs(seed, n=40, m=120)
    s = simple_undirected(h)
    p = g.project()
    rng = random.Random(seed)
    pairs = [(f"n{rng.randrange(40)}", f"n{rng.randrange(40)}") for _ in range(300)]
    pairs = [(a, b) for a, b in pairs if a != b]
    checks = {
        "jaccard": nx.jaccard_coefficient,
        "adamic_adar": nx.adamic_adar_index,
        "resource_allocation": nx.resource_allocation_index,
        "preferential_attachment": nx.preferential_attachment,
    }
    for metric, fn in checks.items():
        want = [x for _, _, x in fn(s, pairs)]
        assert p.similarity(pairs, metric) == pytest.approx(want, rel=1e-12), metric
    common = [len(list(nx.common_neighbors(s, a, b))) for a, b in pairs]
    assert p.similarity(pairs, "common_neighbors") == common

    # Top 5 by Jaccard, against every pair scored by networkx
    top = p.most_similar(k=5)
    for a in s:
        scores = [(b, x) for _, b, x in nx.jaccard_coefficient(s, [(a, b) for b in s if b != a]) if x > 0]
        order = {id: i for i, id in enumerate(p.ids())}
        scores.sort(key=lambda t: (-t[1], order[t[0]]))
        assert [b for b, _ in top[a]] == [b for b, _ in scores[:5]]
        assert [x for _, x in top[a]] == pytest.approx([x for _, x in scores[:5]])
    assert p.most_similar("n0", 3) == top["n0"][:3]
    assert list(p.most_similar(["n1", "n2"]).keys()) == ["n1", "n2"]
    with pytest.raises(ValueError, match="unknown similarity"):
        p.similarity(pairs, "cosine")
    with pytest.raises(ValueError, match="preferential_attachment"):
        p.most_similar(metric="preferential_attachment")


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
@pytest.mark.parametrize("weighted", [False, True])
def test_leiden_and_modularity(seed, direction, weighted):
    g, h = random_graphs(seed, n=120, m=300)
    u = undirected(h)
    weight = "weight" if weighted else None
    p = g.project(weight=weight, direction=direction)
    got = p.leiden(seed=seed)
    assert sorted(id for c in got for id in c) == sorted(p.ids())
    q = p.modularity(got)
    assert q == pytest.approx(nx.community.modularity(u, got, weight=weight or "__none__"), abs=1e-12)
    # Communities are connected; as good as Louvain
    s = simple_undirected(h)
    assert all(nx.is_connected(s.subgraph(c)) for c in got)
    louvain = nx.community.louvain_communities(u, weight=weight or "__none__", seed=seed)
    assert q >= p.modularity(louvain) - 0.02
    assert p.leiden(seed=seed) == got
    assert p.modularity(got, resolution=2) == pytest.approx(
        nx.community.modularity(u, got, weight=weight or "__none__", resolution=2), abs=1e-12
    )


def test_leiden_errors():
    p = random_graphs(0)[0].project()
    with pytest.raises(ValueError, match="randomness"):
        p.leiden(randomness=0)
    with pytest.raises(ValueError, match="in no community"):
        p.modularity([["n0"]])
    with pytest.raises(ValueError, match="more than one community"):
        p.modularity([p.ids(), ["n0"]])


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("weighted", [False, True])
def test_minimum_spanning_tree(seed, weighted):
    g, h = random_graphs(seed, n=80, m=160)
    u = undirected(h)
    weight = "weight" if weighted else None
    p = g.project(weight=weight)
    for maximum in (False, True):
        got = p.minimum_spanning_tree(maximum=maximum)
        fn = nx.maximum_spanning_tree if maximum else nx.minimum_spanning_tree
        want = fn(u, weight=weight or "__none__")
        assert len(got) == want.number_of_edges() == 80 - nx.number_connected_components(u)
        total = sum(d.get(weight, 1) if weight else 1 for _, _, d in want.edges(data=True))
        assert sum(w for _, _, w in got) == pytest.approx(total)
        forest = nx.Graph([(a, b) for a, b, _ in got])
        assert nx.is_forest(forest)


def simple_digraph(h, direction, weight):
    """Lightest edge per ordered pair, without self-loops."""
    d = nx.DiGraph()
    d.add_nodes_from(h)
    edges = oriented(h, direction).edges(data=True)
    for a, b, data in edges:
        if a != b:
            w = data["weight"] if weight else 1
            if not d.has_edge(a, b) or w < d[a][b]["weight"]:
                d.add_edge(a, b, weight=w)
    return d


@pytest.mark.parametrize("seed", SEEDS)
@pytest.mark.parametrize("direction", DIRECTIONS)
@pytest.mark.parametrize("weighted", [False, True])
def test_k_shortest_paths(seed, direction, weighted):
    import itertools

    g, h = random_graphs(seed, n=40, m=120)
    weight = "weight" if weighted else None
    p = g.project(weight=weight, direction=direction)
    d = simple_digraph(h, direction, weight)
    for s, t in [("n0", "n1"), ("n2", "n3"), ("n5", "n9")]:
        got = p.k_shortest_paths(s, t, 8)
        try:
            want = list(itertools.islice(nx.shortest_simple_paths(d, s, t, weight="weight"), 8))
        except nx.NetworkXNoPath:
            want = []
        cost = lambda path: sum(d[a][b]["weight"] for a, b in zip(path, path[1:]))  # noqa: E731
        assert [x["cost"] for x in got] == pytest.approx([cost(w) for w in want])
        for x in got:
            path = x["nodelist"]
            assert path[0] == s and path[-1] == t and len(set(path)) == len(path)
            assert x["cost"] == pytest.approx(cost(path))
        assert len({tuple(x["nodelist"]) for x in got}) == len(got)


def test_fastrp_and_node2vec():
    g, _ = random_graphs(1, n=50, m=200)
    p = g.project(direction="both", weight="weight")
    emb = p.fastrp(16, seed=4)
    assert list(emb) == p.ids() and all(len(v) == 16 for v in emb.values())
    assert p.fastrp(16, seed=4) == emb and p.fastrp(16, seed=5) != emb
    assert p.fastrp(4, iteration_weights=[1.0], self_influence=0.5, normalization_strength=-0.5, seed=1)
    with pytest.raises(ValueError, match="dimension"):
        p.fastrp(0)

    walks = p.node2vec_walks(10, 3, p=0.5, q=2.0, seed=7)
    assert len(walks) == 150 and [w[0] for w in walks[:50]] == p.ids()
    for w in walks:
        assert all(b in p.neighbors(a) for a, b in zip(w, w[1:]))
    assert walks == p.node2vec_walks(10, 3, p=0.5, q=2.0, seed=7)
    some = p.node2vec_walks(5, 2, sources=["n3", "n4"], seed=1)
    assert [w[0] for w in some] == ["n3", "n4", "n3", "n4"]
    with pytest.raises(ValueError, match="q must be"):
        p.node2vec_walks(q=-1)
