"""Benchmark ironweaver against networkx and write a Markdown comparison report.

Both libraries get the same random directed graph (same node ids, edges and
edge attributes) and run equivalent operations. Every operation is timed as
the best of several repeats, and the results of both libraries are checked
against each other so the timings compare like with like.

Usage::

    python benchmarks/compare_networkx.py
    python benchmarks/compare_networkx.py --sizes 1000:5000,50000:250000 --repeats 5
    python benchmarks/compare_networkx.py --output performance_results/networkx_comparison.md

Requires ``networkx`` in addition to the built ``ironweaver`` module.
"""

from __future__ import annotations

import argparse
import datetime
import gc
import json
import os
import platform
import random
import sys
import time
from dataclasses import dataclass, field
from typing import Any, Callable

import networkx as nx

from ironweaver import Vertex

DEFAULT_OUTPUT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "performance_results",
    "networkx_comparison.md",
)
EDGE_TYPES = ("knows", "likes", "follows")


@dataclass
class Result:
    name: str
    description: str
    ironweaver_s: float | None
    networkx_s: float | None
    note: str = ""


@dataclass
class SizeReport:
    nodes: int
    edges: int
    results: list[Result] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def best_of(fn: Callable[[], Any], repeats: int, setup: Callable[[], Any] | None = None) -> tuple[float, Any]:
    """Return (best wall time in seconds, result of the last run).

    If *setup* is given, it runs untimed before every repeat and its return
    value is passed to *fn*.
    """
    best = float("inf")
    result = None
    for _ in range(repeats):
        arg = setup() if setup is not None else None
        gc.collect()
        start = time.perf_counter()
        result = fn(arg) if setup is not None else fn()
        best = min(best, time.perf_counter() - start)
    return best, result


def make_edges(n_nodes: int, n_edges: int, seed: int) -> list[tuple[str, str, dict]]:
    rng = random.Random(seed)
    edges = []
    for _ in range(n_edges):
        u = f"n{rng.randrange(n_nodes)}"
        v = f"n{rng.randrange(n_nodes)}"
        edges.append((u, v, {"type": rng.choice(EDGE_TYPES), "weight": round(rng.uniform(0.1, 10.0), 3)}))
    # A backbone chain keeps the graph (mostly) reachable from n0 so the
    # traversal and path benchmarks do real work.
    for i in range(n_nodes - 1):
        edges.append((f"n{i}", f"n{i + 1}", {"type": "next", "weight": 5.0}))
    return edges


def build_ironweaver(n_nodes: int, edges) -> Vertex:
    g = Vertex()
    for i in range(n_nodes):
        g.add_node(f"n{i}", {"index": i, "group": i % 10})
    for u, v, attr in edges:
        g.add_edge(u, v, dict(attr))
    return g


def build_networkx(n_nodes: int, edges) -> nx.MultiDiGraph:
    # MultiDiGraph: ironweaver keeps parallel edges, so networkx must too.
    g = nx.MultiDiGraph()
    for i in range(n_nodes):
        g.add_node(f"n{i}", index=i, group=i % 10)
    for u, v, attr in edges:
        g.add_edge(u, v, **attr)
    return g


def nx_multi_source_within(g: nx.MultiDiGraph, seeds: list[str], depth: int) -> set[str]:
    """Nodes within *depth* hops of any seed (multi-source BFS)."""
    seen = set(seeds)
    frontier = list(seeds)
    for _ in range(depth):
        nxt = []
        for u in frontier:
            for v in g.successors(u):
                if v not in seen:
                    seen.add(v)
                    nxt.append(v)
        frontier = nxt
    return seen


def nx_random_walks(g: nx.MultiDiGraph, start: str, length: int, attempts: int, seed: int) -> list[list[str]]:
    """Pure-Python random walks over networkx adjacency (networkx has no built-in)."""
    rng = random.Random(seed)
    adj = {n: [v for _, v in g.out_edges(n)] for n in g}
    walks, seen = [], set()
    for _ in range(attempts):
        walk = [start]
        cur = start
        for _ in range(length - 1):
            nbrs = adj[cur]
            if not nbrs:
                break
            cur = rng.choice(nbrs)
            walk.append(cur)
        key = tuple(walk)
        if key not in seen:
            seen.add(key)
            walks.append(walk)
    return walks


def path_cost(g: nx.MultiDiGraph, path: list[str]) -> float:
    return sum(min(d["weight"] for d in g.get_edge_data(u, v).values()) for u, v in zip(path, path[1:]))


# ---------------------------------------------------------------------------
# Benchmarks
# ---------------------------------------------------------------------------

def run_size(n_nodes: int, n_edges: int, repeats: int, seed: int) -> SizeReport:
    report = SizeReport(n_nodes, n_edges + n_nodes - 1)
    edges = make_edges(n_nodes, n_edges, seed)
    add = report.results.append
    rng = random.Random(seed + 1)
    src, dst = "n0", f"n{n_nodes - 1}"
    subset = [f"n{i}" for i in rng.sample(range(n_nodes), max(1, n_nodes // 5))]
    seeds = [f"n{i}" for i in rng.sample(range(n_nodes), max(1, min(100, n_nodes // 10)))]

    # Construction ---------------------------------------------------------
    t_iw, iw = best_of(lambda: build_ironweaver(n_nodes, edges), repeats)
    t_nx, g = best_of(lambda: build_networkx(n_nodes, edges), repeats)
    assert iw.node_count() == g.number_of_nodes()
    add(Result("Build graph", "add all nodes and edges one by one", t_iw, t_nx))

    # Traversal ------------------------------------------------------------
    start = iw.get_node(src)
    t_iw, r_iw = best_of(lambda: start.bfs(), repeats)
    t_nx, r_nx = best_of(lambda: [src] + [v for _, v in nx.bfs_edges(g, src)], repeats)
    assert set(r_iw.keys()) == set(r_nx)
    add(Result("BFS (full)", f"all nodes reachable from `{src}`", t_iw, t_nx, f"{len(r_nx):,} nodes reached"))

    t_iw, r_iw = best_of(lambda: start.bfs(depth=3), repeats)
    t_nx, r_nx = best_of(lambda: [src] + [v for _, v in nx.bfs_edges(g, src, depth_limit=3)], repeats)
    assert set(r_iw.keys()) == set(r_nx)
    add(Result("BFS (depth 3)", "depth-limited BFS", t_iw, t_nx, f"{len(r_nx):,} nodes reached"))

    t_iw, r_iw = best_of(lambda: start.bfs(filter={"type": "knows"}), repeats)
    knows = nx.subgraph_view(g, filter_edge=lambda u, v, k: g.edges[u, v, k]["type"] == "knows")
    t_nx, r_nx = best_of(lambda: [src] + [v for _, v in nx.bfs_edges(knows, src)], repeats)
    assert set(r_iw.keys()) == set(r_nx)
    add(Result("BFS (edge filter)", "only follow `type == \"knows\"` edges", t_iw, t_nx, f"{len(r_nx):,} nodes reached"))

    t_iw, r_iw = best_of(lambda: start.traverse(), repeats)
    t_nx, r_nx = best_of(lambda: list(nx.dfs_preorder_nodes(g, src)), repeats)
    assert set(r_iw.keys()) == set(r_nx)
    add(Result("DFS traversal", "pre-order DFS from source", t_iw, t_nx))

    t_iw, r_iw = best_of(lambda: start.bfs_search(dst), repeats)
    t_nx, r_nx = best_of(lambda: nx.has_path(g, src, dst), repeats)
    assert (r_iw is not None) == r_nx
    add(Result("BFS search", f"find `{dst}` from `{src}`", t_iw, t_nx,
               "networkx `has_path` uses bidirectional BFS"))

    # Paths ----------------------------------------------------------------
    t_iw, r_iw = best_of(lambda: iw.shortest_path_bfs(src, dst), repeats)
    t_nx, r_nx = best_of(lambda: nx.shortest_path(g, src, dst), repeats)
    assert len(r_iw.meta["nodelist"]) == len(r_nx)
    add(Result("Shortest path (unweighted)", "BFS shortest path", t_iw, t_nx,
               f"{len(r_nx) - 1} hops; networkx uses bidirectional BFS"))

    t_iw, r_iw = best_of(lambda: iw.shortest_path_dijkstra(src, dst, weight="weight"), repeats)
    t_nx, r_nx = best_of(lambda: nx.dijkstra_path(g, src, dst, weight="weight"), repeats)
    assert abs(r_iw.meta["cost"] - path_cost(g, r_nx)) < 1e-6
    add(Result("Shortest path (Dijkstra)", "weighted by `weight` attribute", t_iw, t_nx,
               f"cost {r_iw.meta['cost']:.2f}"))

    # Subgraphs ------------------------------------------------------------
    t_iw, r_iw = best_of(lambda: iw.filter(ids=subset), repeats)
    t_nx, r_nx = best_of(lambda: g.subgraph(subset).copy(), repeats)
    assert r_iw.node_count() == r_nx.number_of_nodes()
    add(Result("Subgraph by ids", f"{len(subset):,} nodes + edges between them, as a new graph", t_iw, t_nx))

    t_iw, r_iw = best_of(lambda: iw.filter(group=3), repeats)
    t_nx, r_nx = best_of(lambda: g.subgraph([n for n, d in g.nodes(data=True) if d["group"] == 3]).copy(), repeats)
    assert r_iw.node_count() == r_nx.number_of_nodes()
    add(Result("Subgraph by attribute", "nodes with `group == 3`", t_iw, t_nx))

    seed_graph = iw.filter(ids=seeds)
    t_iw, r_iw = best_of(lambda: seed_graph.expand(iw, depth=2), repeats)
    t_nx, r_nx = best_of(lambda: g.subgraph(nx_multi_source_within(g, seeds, 2)).copy(), repeats)
    assert r_iw.node_count() == r_nx.number_of_nodes()
    add(Result("Expand (depth 2)", f"{len(seeds)} seeds + neighbourhood, as a new graph", t_iw, t_nx,
               f"{r_nx.number_of_nodes():,} nodes"))

    # Mutation -------------------------------------------------------------
    victims = [f"n{i}" for i in rng.sample(range(n_nodes), max(1, n_nodes // 20))]

    def iw_remove(graph):
        for v in victims:
            graph.remove_node(v)
        return graph

    def nx_remove(graph):
        graph.remove_nodes_from(victims)
        return graph

    t_iw, r_iw = best_of(iw_remove, repeats, setup=lambda: build_ironweaver(n_nodes, edges))
    t_nx, r_nx = best_of(nx_remove, repeats, setup=lambda: build_networkx(n_nodes, edges))
    assert r_iw.node_count() == r_nx.number_of_nodes()
    add(Result("Remove nodes", f"remove {len(victims):,} nodes and their edges", t_iw, t_nx))

    # Analysis -------------------------------------------------------------
    t_iw, r_iw = best_of(lambda: iw.get_metadata()["edge_count"], repeats)
    t_nx, r_nx = best_of(lambda: g.number_of_edges(), repeats)
    assert r_iw == r_nx
    add(Result("Count edges", "`get_metadata()` vs `number_of_edges()`", t_iw, t_nx))

    # Random walks ---------------------------------------------------------
    attempts, length = 5_000, 20
    t_iw, r_iw = best_of(lambda: iw.random_walks(src, length, attempts, allow_revisit=True, seed=seed), repeats)
    t_nx, r_nx = best_of(lambda: nx_random_walks(g, src, length, attempts, seed), repeats)
    add(Result("Random walks", f"{attempts:,} walks of length {length}, deduplicated", t_iw, t_nx,
               "networkx has no random walks; pure Python over its adjacency"))

    # Serialization --------------------------------------------------------
    t_iw, s_iw = best_of(lambda: iw.save_to_json(), repeats)
    t_nx, s_nx = best_of(lambda: json.dumps(nx.node_link_data(g, edges="edges")), repeats)
    add(Result("Serialize to JSON string", "`save_to_json()` vs `node_link_data` + `json.dumps`", t_iw, t_nx))

    t_iw, r_iw = best_of(lambda: Vertex.load_from_json(s_iw), repeats)
    t_nx, r_nx = best_of(lambda: nx.node_link_graph(json.loads(s_nx), edges="edges"), repeats)
    assert r_iw.node_count() == r_nx.number_of_nodes()
    add(Result("Load from JSON string", "`load_from_json()` vs `json.loads` + `node_link_graph`", t_iw, t_nx))

    del iw, g
    gc.collect()
    return report


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

def fmt_time(seconds: float | None) -> str:
    if seconds is None:
        return "–"
    if seconds < 1e-3:
        return f"{seconds * 1e6:,.1f} µs"
    if seconds < 1:
        return f"{seconds * 1e3:,.2f} ms"
    return f"{seconds:,.3f} s"


def fmt_speedup(iw: float | None, nx_: float | None) -> str:
    if not iw or not nx_:
        return "–"
    ratio = nx_ / iw
    if ratio >= 1:
        return f"**{ratio:,.1f}× faster**"
    return f"{1 / ratio:,.1f}× slower"


def render(reports: list[SizeReport], repeats: int, seed: int) -> str:
    lines = [
        "# ironweaver vs networkx",
        "",
        f"Generated {datetime.datetime.now().strftime('%Y-%m-%d %H:%M')} by `benchmarks/compare_networkx.py`.",
        "",
        "| | |",
        "|---|---|",
        f"| Python | {platform.python_version()} ({platform.python_implementation()}) |",
        f"| networkx | {nx.__version__} |",
        f"| Platform | {platform.platform()} |",
        f"| CPU cores | {os.cpu_count()} |",
        f"| Repeats | best of {repeats} |",
        f"| Seed | {seed} |",
        "",
        "Both libraries get the same random directed multigraph (random edges plus a",
        "`n0 → n1 → …` chain so everything is reachable from `n0`). Each cell is the",
        "best wall-clock time over the repeats, and every row checks that both",
        "libraries return the same result. \"Speedup\" is networkx time ÷ ironweaver time.",
        "",
    ]
    for rep in reports:
        lines += [
            f"## {rep.nodes:,} nodes, {rep.edges:,} edges",
            "",
            "| Operation | Description | ironweaver | networkx | Speedup | Notes |",
            "|---|---|---:|---:|---:|---|",
        ]
        for r in rep.results:
            lines.append(
                f"| {r.name} | {r.description} | {fmt_time(r.ironweaver_s)} | {fmt_time(r.networkx_s)} "
                f"| {fmt_speedup(r.ironweaver_s, r.networkx_s)} | {r.note} |"
            )
        lines.append("")
    return "\n".join(lines)


def parse_sizes(text: str) -> list[tuple[int, int]]:
    sizes = []
    for part in text.split(","):
        nodes, _, edges = part.partition(":")
        n = int(nodes)
        sizes.append((n, int(edges) if edges else n * 5))
    return sizes


def main(argv: list[str] | None = None) -> str:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--sizes", default="1000:5000,20000:100000",
                        help="comma-separated NODES[:EDGES] graph sizes (default: %(default)s)")
    parser.add_argument("--repeats", type=int, default=3, help="repeats per operation (default: %(default)s)")
    parser.add_argument("--seed", type=int, default=42, help="random seed (default: %(default)s)")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="Markdown file to write (default: %(default)s)")
    args = parser.parse_args(argv)

    reports = []
    for nodes, edges in parse_sizes(args.sizes):
        print(f"Benchmarking {nodes:,} nodes / {edges:,} random edges ...", file=sys.stderr)
        reports.append(run_size(nodes, edges, args.repeats, args.seed))

    markdown = render(reports, args.repeats, args.seed)
    os.makedirs(os.path.dirname(os.path.abspath(args.output)), exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as fh:
        fh.write(markdown)
    print(markdown)
    print(f"\nWrote {args.output}", file=sys.stderr)
    return args.output


if __name__ == "__main__":
    main()
