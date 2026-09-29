"""Validate and benchmark ironweaver against other graph libraries on
medium-sized graphs.

Datasets (downloaded once into benchmarks/data/, or generated):
- github:   the GitHub developer network (SNAP "musae-github"), 37,700
            nodes, 289,003 undirected edges; from the karateclub repository.
- facebook: the Facebook page-page network (SNAP "musae-facebook"), 22,470
            nodes, 170,823 undirected edges; from the karateclub repository.
- kron:     a directed Kronecker (R-MAT) graph with the Graph500 parameters
            (a=0.57, b=0.19, c=0.19), as in the GAP Benchmark Suite; scale 16
            (65,536 node ids) and edge factor 16, without self-loops and
            duplicate edges. Deterministic for a seed.

Libraries (each is skipped if not installed): ironweaver, networkx, igraph,
rustworkx, networkit. Every result is checked against ironweaver's (the
"check" column: "=" identical, "≈" within the tolerance given, "~" a
different algorithm or definition, so only compared by a quality measure).
Edge weights, where used, are deterministic in 1.0..10.9.

Timings are the best of --repeats runs, with Python's cyclic garbage
collector off while timing (as `timeit` does). The "ironweaver" column includes
building the projection (`project()`) each time, as a one-off call would;
"ironweaver (reused)" runs on a projection built beforehand, which is how
several analyses on one graph run. The "Build" row is the time to create the
graph from an edge list in Python.

    python benchmarks/compare_libraries.py                   # all datasets
    python benchmarks/compare_libraries.py --datasets github --repeats 1
"""

from __future__ import annotations

import argparse
import datetime
import gc
import math
import os
import platform
import random
import sys
import time
import urllib.request
from typing import Any, Callable

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "data")
KARATECLUB = "https://raw.githubusercontent.com/benedekrozemberczki/karateclub/master/dataset/node_level/{}/edges.csv"


# ---------------------------------------------------------------------------
# Datasets
# ---------------------------------------------------------------------------

class Dataset:
    def __init__(self, name: str, n: int, edges: list[tuple[int, int]], directed: bool, source: str):
        self.name, self.n, self.edges, self.directed, self.source = name, n, edges, directed, source
        # Deterministic weights 1.0..10.9
        self.weights = [1.0 + ((a * 7919 + b * 104729) % 100) / 10 for a, b in edges]
        # A well-connected source for BFS / SSSP: the node with the most edges
        degree = [0] * n
        for a, b in edges:
            degree[a] += 1
            degree[b] += 1
        self.root = max(range(n), key=degree.__getitem__)


def karateclub(name: str, label: str) -> Dataset:
    os.makedirs(DATA, exist_ok=True)
    path = os.path.join(DATA, f"{name}.csv")
    if not os.path.exists(path):
        url = KARATECLUB.format(name)
        print(f"downloading {url}", file=sys.stderr)
        urllib.request.urlretrieve(url, path + ".part")
        os.replace(path + ".part", path)
    seen, edges = set(), []
    with open(path) as fh:
        next(fh)  # header "id_1,id_2"
        for line in fh:
            a, b = map(int, line.split(","))
            key = (min(a, b), max(a, b))
            if a != b and key not in seen:
                seen.add(key)
                edges.append(key)
    n = 1 + max(max(e) for e in edges)
    return Dataset(name, n, edges, False, label)


def kron(scale: int = 16, edge_factor: int = 16, seed: int = 1) -> Dataset:
    """R-MAT edges with the Graph500 parameters; node ids shuffled."""
    rng = random.Random(seed)
    n = 1 << scale
    a, b, c = 0.57, 0.19, 0.19
    perm = list(range(n))
    rng.shuffle(perm)
    seen, edges = set(), []
    for _ in range(edge_factor * n):
        u = v = 0
        for _ in range(scale):
            r = rng.random()
            u, v = u << 1, v << 1
            if r < a:
                pass
            elif r < a + b:
                v |= 1
            elif r < a + b + c:
                u |= 1
            else:
                u |= 1
                v |= 1
        u, v = perm[u], perm[v]
        if u != v and (u, v) not in seen:
            seen.add((u, v))
            edges.append((u, v))
    return Dataset("kron", n, edges, True, f"Kronecker scale {scale}, edge factor {edge_factor} (Graph500 R-MAT)")


DATASETS: dict[str, Callable[[], Dataset]] = {
    "github": lambda: karateclub("github", "GitHub developers (SNAP musae-github)"),
    "facebook": lambda: karateclub("facebook", "Facebook pages (SNAP musae-facebook)"),
    "kron": kron,
}


# ---------------------------------------------------------------------------
# Graph construction per library
# ---------------------------------------------------------------------------

def build_ironweaver(ds: Dataset):
    """Bulk loading: `add_nodes`, then `add_edges` with a weight column."""
    from ironweaver import Vertex
    g = Vertex()
    ids = [str(i) for i in range(ds.n)]
    g.add_nodes(ids)
    g.add_edges([(ids[a], ids[b]) for a, b in ds.edges], attrs={"weight": ds.weights})
    return g


def ironweaver_simple_undirected(g, ds: Dataset):
    """For communities on a directed dataset: an ironweaver graph with one
    undirected edge per node pair, the graph the other libraries (and the
    modularity score) see; a `direction="both"` projection of the directed
    graph would count a pair joined both ways as a double-weight edge.
    Built once per graph, outside the timing."""
    from ironweaver import Vertex
    if not ds.directed:
        return g
    if id(g) not in _UNDIRECTED:
        pairs = sorted({(min(a, b), max(a, b)) for a, b in ds.edges})
        u = Vertex()
        u.add_nodes([str(i) for i in range(ds.n)])
        u.add_edges([(str(a), str(b)) for a, b in pairs])
        _UNDIRECTED[id(g)] = (g, u)
    return _UNDIRECTED[id(g)][1]


def build_networkx(ds: Dataset):
    import networkx as nx
    g = nx.DiGraph() if ds.directed else nx.Graph()
    g.add_nodes_from(range(ds.n))
    g.add_weighted_edges_from((a, b, w) for (a, b), w in zip(ds.edges, ds.weights))
    return g


def build_igraph(ds: Dataset):
    import igraph as ig
    g = ig.Graph(n=ds.n, edges=ds.edges, directed=ds.directed)
    g.es["weight"] = ds.weights
    return g


def build_rustworkx(ds: Dataset):
    import rustworkx as rx
    g = rx.PyDiGraph() if ds.directed else rx.PyGraph()
    g.add_nodes_from(range(ds.n))
    g.add_edges_from([(a, b, w) for (a, b), w in zip(ds.edges, ds.weights)])
    return g


def build_networkit(ds: Dataset):
    import networkit as nk
    g = nk.Graph(ds.n, weighted=True, directed=ds.directed)
    for (a, b), w in zip(ds.edges, ds.weights):
        g.addEdge(a, b, w)
    return g


def networkit_undirected(g, ds: Dataset):
    """An undirected networkit graph of the dataset: one edge per node pair,
    with the lighter weight. Built from the edge list because networkit
    11.2's `graphtools.toUndirected` miscounts edges when a directed graph
    has both u -> v and v -> u (`numberOfEdges()` drops by one per pair),
    which makes later algorithms such as `KruskalMSF` corrupt the heap."""
    import networkit as nk
    if not ds.directed:
        return g
    # Built once per graph and reused: the conversion (a Python loop) is
    # not what's being timed
    if id(g) in _UNDIRECTED:
        return _UNDIRECTED[id(g)][1]
    lightest: dict[tuple[int, int], float] = {}
    for (a, b), w in zip(ds.edges, ds.weights):
        key = (min(a, b), max(a, b))
        lightest[key] = min(w, lightest.get(key, w))
    u = nk.Graph(ds.n, weighted=True, directed=False)
    for (a, b), w in lightest.items():
        u.addEdge(a, b, w)
    _UNDIRECTED[id(g)] = (g, u)  # keeps g alive, so its id stays unique
    return u


_UNDIRECTED: dict[int, tuple[Any, Any]] = {}


BUILDERS = {
    "ironweaver": build_ironweaver,
    "networkx": build_networkx,
    "igraph": build_igraph,
    "rustworkx": build_rustworkx,
    "networkit": build_networkit,
}


def available() -> list[str]:
    libs = []
    for name in BUILDERS:
        try:
            __import__(name)
            libs.append(name)
        except ImportError:
            print(f"{name} not installed: skipped", file=sys.stderr)
    return libs


# ---------------------------------------------------------------------------
# Operations: one function per library, results normalised to plain Python
# (dicts by int node, partitions as sorted tuples, numbers)
# ---------------------------------------------------------------------------

def partition(groups) -> list[tuple[int, ...]]:
    return sorted(tuple(sorted(int(x) for x in g)) for g in groups)


def by_int(d: dict) -> dict[int, Any]:
    return {int(k): v for k, v in d.items()}


def dense(values) -> dict[int, Any]:
    return dict(enumerate(values))


class Projections:
    """A Vertex whose `project(...)` results are built once and reused: the
    "ironweaver (reused)" column, which times the algorithms alone."""

    def __init__(self, vertex):
        self.vertex, self.cache = vertex, {}

    def derived(self, key, make):
        """Another reused-projection wrapper built from this vertex, once."""
        if key not in self.cache:
            self.cache[key] = make(self.vertex)
        return self.cache[key]

    def project(self, **kw):
        key = tuple(sorted(kw.items()))
        if key not in self.cache:
            self.cache[key] = self.vertex.project(**kw)
        return self.cache[key]


REUSED = "ironweaver (reused)"


def iw(g, ds: Dataset, **kw):
    """The ironweaver projection matching the dataset (undirected: "both")."""
    return g.project(direction="out" if ds.directed else "both", **kw)


def modularity(ds: Dataset, groups) -> float:
    """Modularity (unweighted, edges as undirected), with networkx."""
    import networkx as nx
    u = nx.Graph()
    u.add_nodes_from(range(ds.n))
    u.add_edges_from(ds.edges)
    return nx.community.modularity(u, [set(map(int, c)) for c in groups])


def op_wcc():
    def ironweaver(g, ds):
        return partition(iw(g, ds).weakly_connected_components())

    def networkx(g, ds):
        import networkx as nx
        comps = nx.weakly_connected_components(g) if ds.directed else nx.connected_components(g)
        return partition(comps)

    def igraph(g, ds):
        return partition(g.connected_components(mode="weak"))

    def rustworkx(g, ds):
        import rustworkx as rx
        return partition(rx.weakly_connected_components(g) if ds.directed else rx.connected_components(g))

    def networkit(g, ds):
        import networkit as nk
        cc = (nk.components.WeaklyConnectedComponents if ds.directed else nk.components.ConnectedComponents)(g)
        cc.run()
        return partition(cc.getComponents())

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_scc():
    def ironweaver(g, ds):
        return partition(iw(g, ds).strongly_connected_components())

    def networkx(g, ds):
        import networkx as nx
        return partition(nx.strongly_connected_components(g))

    def igraph(g, ds):
        return partition(g.connected_components(mode="strong"))

    def rustworkx(g, ds):
        import rustworkx as rx
        return partition(rx.strongly_connected_components(g))

    def networkit(g, ds):
        import networkit as nk
        cc = nk.components.StronglyConnectedComponents(g)
        cc.run()
        return partition(cc.getComponents())

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_bfs():
    def ironweaver(g, ds):
        return by_int(iw(g, ds).bfs_levels([str(ds.root)]))

    def networkx(g, ds):
        import networkx as nx
        return dict(nx.single_source_shortest_path_length(g, ds.root))

    def igraph(g, ds):
        d = g.distances(source=ds.root, mode="out")[0]
        return {v: int(x) for v, x in enumerate(d) if x != math.inf}

    def rustworkx(g, ds):
        import rustworkx as rx
        return {v: level for level, layer in enumerate(rx.bfs_layers(g, [ds.root])) for v in layer}

    def networkit(g, ds):
        import networkit as nk
        b = nk.distance.BFS(g, ds.root, storePaths=False)
        b.run()
        return {v: int(x) for v, x in enumerate(b.getDistances()) if x < 1e300}

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_sssp():
    def ironweaver(g, ds):
        return by_int(iw(g, ds, weight="weight").distances([str(ds.root)])[str(ds.root)])

    def networkx(g, ds):
        import networkx as nx
        return dict(nx.single_source_dijkstra_path_length(g, ds.root))

    def igraph(g, ds):
        d = g.distances(source=ds.root, weights="weight", mode="out")[0]
        return {v: x for v, x in enumerate(d) if x != math.inf}

    def rustworkx(g, ds):
        import rustworkx as rx
        d = dict(rx.dijkstra_shortest_path_lengths(g, ds.root, edge_cost_fn=float))
        d[ds.root] = 0.0
        return d

    def networkit(g, ds):
        import networkit as nk
        d = nk.distance.Dijkstra(g, ds.root, storePaths=False)
        d.run()
        return {v: x for v, x in enumerate(d.getDistances()) if x < 1e300}

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_pagerank():
    # Every library with dangling-node mass spread uniformly (networkx's
    # definition); networkit needs DISTRIBUTE_SINKS for that.
    def ironweaver(g, ds):
        return by_int(iw(g, ds).pagerank(tol=1e-10, max_iter=1000))

    def networkx(g, ds):
        import networkx as nx
        return nx.pagerank(g, weight=None, tol=1e-10, max_iter=1000)

    def igraph(g, ds):
        return dense(g.pagerank(damping=0.85, directed=ds.directed))

    def rustworkx(g, ds):
        import rustworkx as rx
        if not ds.directed:
            return None  # rustworkx has PageRank for directed graphs only
        return dict(rx.pagerank(g, alpha=0.85, weight_fn=lambda _: 1.0, tol=1e-10, max_iter=1000))

    def networkit(g, ds):
        import networkit as nk
        pr = nk.centrality.PageRank(nk.graphtools.toUnweighted(g), damp=0.85, tol=1e-12,
                                    distributeSinks=nk.centrality.SinkHandling.DistributeSinks)
        pr.run()
        return dense(pr.scores())

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_core():
    def ironweaver(g, ds):
        return by_int(iw(g, ds).core_number())

    def networkx(g, ds):
        import networkx as nx
        return nx.core_number(g.to_undirected(as_view=True) if ds.directed else g)

    def igraph(g, ds):
        return dense(g.coreness(mode="all") if not ds.directed else g.as_undirected().simplify().coreness())

    def rustworkx(g, ds):
        import rustworkx as rx
        return dict(rx.core_number(g.to_undirected(multigraph=False) if ds.directed else g))

    def networkit(g, ds):
        import networkit as nk
        u = networkit_undirected(g, ds)
        c = nk.centrality.CoreDecomposition(u)
        c.run()
        return {v: int(x) for v, x in enumerate(c.scores())}

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_clustering():
    def ironweaver(g, ds):
        return by_int(iw(g, ds).clustering())

    def networkx(g, ds):
        import networkx as nx
        return nx.clustering(nx.Graph(g) if ds.directed else g)

    def igraph(g, ds):
        u = g.as_undirected().simplify() if ds.directed else g
        return dense(u.transitivity_local_undirected(mode="zero"))

    def networkit(g, ds):
        import networkit as nk
        u = networkit_undirected(g, ds)
        c = nk.centrality.LocalClusteringCoefficient(u)
        c.run()
        return dense(c.scores())

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, networkit=networkit)


def op_betweenness():
    # Unweighted, normalised as networkx does
    def ironweaver(g, ds):
        return by_int(iw(g, ds).betweenness_centrality())

    def networkx(g, ds):
        import networkx as nx
        return nx.betweenness_centrality(g)

    def igraph(g, ds):
        n = ds.n
        scale = 1 / ((n - 1) * (n - 2)) * (1 if ds.directed else 2)
        return {v: x * scale for v, x in enumerate(g.betweenness(directed=ds.directed))}

    def rustworkx(g, ds):
        import rustworkx as rx
        return dict(rx.betweenness_centrality(g, normalized=True))

    def networkit(g, ds):
        import networkit as nk
        b = nk.centrality.Betweenness(nk.graphtools.toUnweighted(g), normalized=True)
        b.run()
        return dense(b.scores())

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


def op_communities():
    # Different algorithms: compared by the modularity of what they find
    def ironweaver(g, ds):
        if ds.directed:  # the simple undirected graph the others get
            if isinstance(g, Projections):
                g = g.derived("simple", lambda v: Projections(ironweaver_simple_undirected(v, ds)))
            else:
                g = ironweaver_simple_undirected(g, ds)
        return g.project(direction="both").leiden(seed=1)

    def networkx(g, ds):
        import networkx as nx
        return nx.community.louvain_communities(g.to_undirected() if ds.directed else g, weight=None, seed=1)

    def igraph(g, ds):
        import igraph as ig
        ig.set_random_number_generator(random.Random(1))
        u = g.as_undirected() if ds.directed else g
        return list(u.community_leiden(objective_function="modularity", n_iterations=-1))

    def networkit(g, ds):
        import networkit as nk
        nk.engineering.setSeed(1, True)
        u = nk.graphtools.toUnweighted(networkit_undirected(g, ds))
        pl = nk.community.ParallelLeiden(u)
        pl.run()
        groups: dict[int, list[int]] = {}
        for v, c in enumerate(pl.getPartition().getVector()):
            groups.setdefault(c, []).append(v)
        return list(groups.values())

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, networkit=networkit)


def op_mst():
    def ironweaver(g, ds):
        return sum(w for _, _, w in iw(g, ds, weight="weight").minimum_spanning_tree())

    def networkx(g, ds):
        import networkx as nx
        # A multigraph keeps both edges of a u -> v / v -> u pair (to_undirected
        # would keep one of them), so the lighter one can be picked
        if ds.directed:
            u = nx.MultiGraph()
            u.add_edges_from(g.edges(data=True))
        else:
            u = g
        return sum(e[-1]["weight"] for e in nx.minimum_spanning_edges(u, data=True))  # multigraphs add keys

    def igraph(g, ds):
        u = g.as_undirected(combine_edges="min") if ds.directed else g
        return sum(u.spanning_tree(weights="weight").es["weight"])

    def rustworkx(g, ds):
        import rustworkx as rx
        u = g.to_undirected(multigraph=True) if ds.directed else g  # both edges of a pair
        return sum(w for _, _, w in rx.minimum_spanning_edges(u, weight_fn=float))

    def networkit(g, ds):
        import networkit as nk
        u = networkit_undirected(g, ds)
        k = nk.graph.KruskalMSF(u)
        k.run()
        return k.getTotalWeight()

    return dict(ironweaver=ironweaver, networkx=networkx, igraph=igraph, rustworkx=rustworkx, networkit=networkit)


# Comparisons: (symbol, note) given ironweaver's result and another's
def same(ref, got):
    return ("=", "") if ref == got else ("✗", "different result")


def close(tol):
    def check(ref: dict, got: dict):
        if set(ref) != set(got):
            return "✗", f"{len(set(ref) ^ set(got))} nodes differ"
        dev = max((abs(ref[k] - got[k]) for k in ref), default=0.0)
        if dev == 0:
            return "=", ""
        return ("≈", f"max diff {dev:.0e}") if dev <= tol else ("✗", f"max diff {dev:.1e}")
    return check


def total_close(ref: float, got: float):
    return ("=", "") if ref == got else (("≈", f"diff {abs(ref - got):.0e}") if abs(ref - got) < 1e-6 * ref else ("✗", f"{got} vs {ref}"))


OPS = [
    # name, ops, check, directed-only, max nodes*edges for networkx (it is slow)
    ("Weakly connected components", op_wcc, same, False),
    ("Strongly connected components", op_scc, same, True),
    ("BFS levels from one node", op_bfs, same, False),
    ("Dijkstra from one node (weighted)", op_sssp, close(1e-9), False),
    ("PageRank", op_pagerank, close(1e-6), False),
    ("Core number", op_core, same, False),
    ("Local clustering", op_clustering, close(1e-12), False),
    ("Betweenness (exact)", op_betweenness, close(1e-9), False),
    ("Communities (Leiden / Louvain)", op_communities, None, False),
    ("Minimum spanning forest (weight)", op_mst, total_close, False),
]

SLOW_FOR_NETWORKX = {"Betweenness (exact)"}
# Exact betweenness costs about nodes * edges: only on graphs where that's small
BETWEENNESS_BUDGET = 5e9


def timed(fn: Callable[[], Any], repeats: int) -> tuple[float, Any]:
    """Best time of `repeats` runs, with Python's cyclic GC off while timing
    (as `timeit` does): otherwise a run that allocates many objects pays for
    rescanning everything the benchmark process holds (the datasets, the
    other libraries' graphs), which is noise, not the library's cost."""
    best, result = math.inf, None
    for _ in range(repeats):
        gc.collect()
        gc.disable()
        try:
            start = time.perf_counter()
            result = fn()
            best = min(best, time.perf_counter() - start)
        finally:
            gc.enable()
    return best, result


def fmt(t: float | None) -> str:
    if t is None:
        return "–"
    if t < 1e-3:
        return f"{t * 1e6:.0f} µs"
    if t < 1:
        return f"{t * 1e3:.1f} ms"
    return f"{t:.2f} s"


def run(ds: Dataset, libs: list[str], repeats: int, networkx_betweenness: bool) -> list[str]:
    lines = [f"### {ds.name}: {ds.source}", "",
             f"{ds.n:,} nodes, {len(ds.edges):,} {'directed' if ds.directed else 'undirected'} edges.", ""]
    header = "| Operation | " + " | ".join(libs) + " | Check |"
    lines += [header, "|" + "---|" * (len(libs) + 2)]

    graphs, build_times = {}, {}
    for lib in libs:
        if lib != REUSED:
            build_times[lib], graphs[lib] = timed(lambda: BUILDERS[lib](ds), 1)
    graphs[REUSED] = Projections(graphs["ironweaver"])
    lines.append("| Build graph from an edge list | " + " | ".join(fmt(build_times.get(l)) for l in libs) + " | |")

    for name, make, check, directed_only in OPS:
        if directed_only and not ds.directed:
            continue
        if name == "Betweenness (exact)" and ds.n * len(ds.edges) > BETWEENNESS_BUDGET:
            continue
        fns = make()
        fns[REUSED] = fns["ironweaver"]
        times, results = {}, {}
        for lib in libs:
            fn = fns.get(lib)
            if fn is None or (lib == "networkx" and name in SLOW_FOR_NETWORKX and not networkx_betweenness):
                continue
            t, r = timed(lambda: fn(graphs[lib], ds), 1 if name in SLOW_FOR_NETWORKX else repeats)
            if r is None:
                continue
            times[lib], results[lib] = t, r
            print(f"  {ds.name}: {name}: {lib} {fmt(t)}", file=sys.stderr)
        notes = []
        if check is None:
            for lib, r in results.items():
                if lib != REUSED:
                    notes.append(f"{lib}: Q={modularity(ds, r):.3f}, {len(r):,} groups")
        else:
            ref = results["ironweaver"]
            for lib, r in results.items():
                if not lib.startswith("ironweaver"):
                    symbol, note = check(ref, r)
                    notes.append(f"{lib} {symbol}{' (' + note + ')' if note else ''}")
        cells = []
        best = min(times.values())
        for lib in libs:
            cell = fmt(times.get(lib))
            if lib in times and times[lib] == best and len(times) > 1:
                cell = f"**{cell}**"
            cells.append(cell)
        lines.append(f"| {name} | " + " | ".join(cells) + " | " + "; ".join(notes) + " |")
    lines.append("")
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--datasets", default="facebook,github,kron", help="comma-separated: " + ", ".join(DATASETS))
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--networkx-betweenness", action="store_true",
                        help="also time networkx's exact betweenness (minutes)")
    parser.add_argument("--output", default=os.path.join(HERE, "..", "performance_results", "library_comparison.md"))
    args = parser.parse_args()

    libs = available()
    import importlib.metadata as md
    versions = ", ".join(f"{l} {md.version(l)}" for l in libs)
    libs.insert(1, REUSED)
    lines = [
        "# Graph library comparison",
        "",
        f"Generated {datetime.date.today()} by `benchmarks/compare_libraries.py` on "
        f"{platform.system()} {platform.machine()}, {os.cpu_count()} CPUs, Python {platform.python_version()}.",
        f"Versions: {versions}.",
        "",
        "Best of the runs; the fastest per row in bold. Check: `=` same result as ironweaver, `≈` within",
        "the stated difference, `✗` a different result. Communities are different algorithms, compared by",
        "modularity Q (higher is better). \"ironweaver\" includes building the projection each time;",
        "\"ironweaver (reused)\" runs on one built beforehand. Exact betweenness runs only where",
        "nodes × edges ≤ 5e9; networkx's only with `--networkx-betweenness`.",
        "",
    ]
    for name in args.datasets.split(","):
        ds = DATASETS[name.strip()]()
        print(f"{ds.name}: {ds.n:,} nodes, {len(ds.edges):,} edges", file=sys.stderr)
        lines += run(ds, libs, args.repeats, args.networkx_betweenness)
    os.makedirs(os.path.dirname(args.output), exist_ok=True)
    with open(args.output, "w") as fh:
        fh.write("\n".join(lines))
    print("\n".join(lines))
    print(f"\nWrote {os.path.abspath(args.output)}", file=sys.stderr)


if __name__ == "__main__":
    main()
