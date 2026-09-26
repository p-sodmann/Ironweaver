"""Compare the memory footprint of ironweaver and networkx; write a Markdown report.

ironweaver keeps most of a graph in Rust allocations that ``tracemalloc`` and
``sys.getsizeof`` cannot see, so this measures what the operating system sees:
the process's resident set size (RSS). Every measurement runs in a fresh
Python subprocess that imports both libraries, records its RSS, does one thing
(build a graph, load a file, ...) and reports how much the RSS grew. The graph
is generated on the fly from a seeded RNG, so no input data is held in memory
while measuring and both libraries get exactly the same graph.

Measured per graph size:

* **Resident graph** - RSS growth after building the graph (with and without
  node/edge attributes), i.e. what it costs to keep the graph in memory.
* **Peak while loading / saving JSON** - highest RSS reached during the
  operation (Linux only; uses ``/proc/self/clear_refs`` to reset the peak).
* **File size** - bytes on disk for JSON and binary formats.

Usage::

    python benchmarks/compare_networkx_memory.py
    python benchmarks/compare_networkx_memory.py --sizes 10000:50000,100000:500000 --repeats 3

Requires ``networkx`` in addition to the built ``ironweaver`` module, and
Linux (``/proc``) or ``psutil`` for reading the RSS.
"""

from __future__ import annotations

import argparse
import datetime
import gc
import json
import os
import pickle
import platform
import random
import statistics
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field

DEFAULT_OUTPUT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "performance_results",
    "networkx_memory.md",
)
EDGE_TYPES = ("knows", "likes", "follows")
LIBS = ("ironweaver", "networkx")


# ---------------------------------------------------------------------------
# Measuring memory
# ---------------------------------------------------------------------------

def _proc_status(field_name: str) -> int | None:
    """A ``kB`` field of /proc/self/status, in bytes (None if unavailable)."""
    try:
        with open("/proc/self/status", encoding="ascii") as fh:
            for line in fh:
                if line.startswith(field_name + ":"):
                    return int(line.split()[1]) * 1024
    except OSError:
        pass
    return None


def rss() -> int:
    """Current resident set size in bytes."""
    value = _proc_status("VmRSS")
    if value is not None:
        return value
    try:
        import psutil
    except ImportError as e:  # pragma: no cover - non-Linux without psutil
        raise SystemExit("reading the RSS needs Linux (/proc) or `pip install psutil`") from e
    return psutil.Process().memory_info().rss


def reset_peak() -> bool:
    """Reset the peak RSS (Linux >= 4.0). Returns False if unsupported."""
    try:
        with open("/proc/self/clear_refs", "w", encoding="ascii") as fh:
            fh.write("5")
        return _proc_status("VmHWM") is not None
    except OSError:
        return False


def peak_rss() -> int | None:
    return _proc_status("VmHWM")


def settle() -> None:
    gc.collect()
    gc.collect()


# ---------------------------------------------------------------------------
# The graph (generated lazily so no input data sits in memory)
# ---------------------------------------------------------------------------

def iter_edges(n_nodes: int, n_edges: int, seed: int, with_attrs: bool = True):
    """Same random multigraph as benchmarks/compare_networkx.py: random edges
    plus an ``n0 -> n1 -> ...`` chain. Yields fresh objects every call."""
    rng = random.Random(seed)
    for _ in range(n_edges):
        u = rng.randrange(n_nodes)
        v = rng.randrange(n_nodes)
        etype = rng.choice(EDGE_TYPES)
        weight = round(rng.uniform(0.1, 10.0), 3)
        yield f"n{u}", f"n{v}", ({"type": etype, "weight": weight} if with_attrs else None)
    for i in range(n_nodes - 1):
        yield f"n{i}", f"n{i + 1}", ({"type": "next", "weight": 5.0} if with_attrs else None)


def build_ironweaver(n_nodes: int, n_edges: int, seed: int, with_attrs: bool = True):
    from ironweaver import Vertex

    g = Vertex()
    for i in range(n_nodes):
        if with_attrs:
            g.add_node(f"n{i}", {"index": i, "group": i % 10})
        else:
            g.add_node(f"n{i}")
    for u, v, attr in iter_edges(n_nodes, n_edges, seed, with_attrs):
        if attr is None:
            g.add_edge(u, v)
        else:
            g.add_edge(u, v, attr)
    return g


def build_networkx(n_nodes: int, n_edges: int, seed: int, with_attrs: bool = True):
    import networkx as nx

    # MultiDiGraph: ironweaver keeps parallel edges, so networkx must too.
    g = nx.MultiDiGraph()
    for i in range(n_nodes):
        if with_attrs:
            g.add_node(f"n{i}", index=i, group=i % 10)
        else:
            g.add_node(f"n{i}")
    for u, v, attr in iter_edges(n_nodes, n_edges, seed, with_attrs):
        if attr is None:
            g.add_edge(u, v)
        else:
            g.add_edge(u, v, **attr)
    return g


BUILDERS = {"ironweaver": build_ironweaver, "networkx": build_networkx}


def save_json(lib: str, g, path: str) -> None:
    if lib == "ironweaver":
        g.save_to_json(path)
    else:
        import networkx as nx

        with open(path, "w", encoding="utf-8") as fh:
            json.dump(nx.node_link_data(g, edges="edges"), fh)


def load_json(lib: str, path: str):
    if lib == "ironweaver":
        from ironweaver import Vertex

        return Vertex.load_from_json(path)
    import networkx as nx

    with open(path, encoding="utf-8") as fh:
        return nx.node_link_graph(json.load(fh), edges="edges")


def save_binary(lib: str, g, path: str) -> None:
    if lib == "ironweaver":
        g.save_to_binary(path)
    else:
        with open(path, "wb") as fh:
            pickle.dump(g, fh, protocol=pickle.HIGHEST_PROTOCOL)


# ---------------------------------------------------------------------------
# Worker: one measurement in a fresh process
# ---------------------------------------------------------------------------

def worker(lib: str, scenario: str, n_nodes: int, n_edges: int, seed: int, path: str) -> dict:
    # Import both libraries up front so neither's import cost lands in a
    # measurement, and warm up the builder so first-call allocations do not
    # either.
    import networkx  # noqa: F401
    import ironweaver  # noqa: F401

    build = BUILDERS[lib]
    build(10, 10, seed)
    settle()

    if scenario in ("graph", "graph_no_attrs"):
        before = rss()
        g = build(n_nodes, n_edges, seed, with_attrs=(scenario == "graph"))
        settle()
        return {"bytes": rss() - before, "check": len(g)}

    if scenario == "load_json":
        before = rss()
        if not reset_peak():
            return {"bytes": None}
        g = load_json(lib, path)
        peak = peak_rss()
        settle()
        return {"bytes": peak - before, "resident": rss() - before, "check": len(g)}

    if scenario == "save_json":
        g = build(n_nodes, n_edges, seed)
        settle()
        before = rss()
        if not reset_peak():
            return {"bytes": None}
        save_json(lib, g, path)
        return {"bytes": peak_rss() - before, "check": len(g)}

    raise ValueError(f"unknown scenario {scenario!r}")


def measure(lib: str, scenario: str, n_nodes: int, n_edges: int, seed: int, path: str) -> dict:
    """Run :func:`worker` in a fresh interpreter and return its result."""
    cmd = [sys.executable, os.path.abspath(__file__), "--worker", lib, scenario,
           str(n_nodes), str(n_edges), str(seed), path]
    out = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------

@dataclass
class Row:
    name: str
    description: str
    ironweaver: int | None
    networkx: int | None
    note: str = ""


@dataclass
class SizeReport:
    nodes: int
    edges: int
    memory: list[Row] = field(default_factory=list)
    files: list[Row] = field(default_factory=list)


def median_bytes(results: list[dict]) -> int | None:
    values = [r["bytes"] for r in results if r.get("bytes") is not None]
    return int(statistics.median(values)) if values else None


def run_size(n_nodes: int, n_edges: int, repeats: int, seed: int, workdir: str) -> SizeReport:
    total_edges = n_edges + n_nodes - 1
    report = SizeReport(n_nodes, total_edges)

    # Input files for the load benchmark, and the file-size table.
    json_paths, files = {}, {}
    for lib in LIBS:
        g = BUILDERS[lib](n_nodes, n_edges, seed)
        json_paths[lib] = os.path.join(workdir, f"{lib}_{n_nodes}.json")
        save_json(lib, g, json_paths[lib])
        bin_path = os.path.join(workdir, f"{lib}_{n_nodes}.bin")
        save_binary(lib, g, bin_path)
        files[lib] = (os.path.getsize(json_paths[lib]), os.path.getsize(bin_path))
        del g
        settle()

    def run(scenario: str) -> dict[str, int | None]:
        result = {}
        for lib in LIBS:
            path = json_paths[lib] if scenario == "load_json" else os.path.join(workdir, f"out_{lib}.json")
            runs = [measure(lib, scenario, n_nodes, n_edges, seed, path) for _ in range(repeats)]
            checks = {r.get("check") for r in runs if r.get("bytes") is not None}
            assert checks <= {n_nodes}, f"{lib} {scenario}: expected {n_nodes} nodes, got {checks}"
            result[lib] = median_bytes(runs)
        return result

    per = f"per node + {total_edges / n_nodes:.0f} edges"
    m = run("graph")
    report.memory.append(Row("Resident graph", "nodes (`index`, `group`) and edges (`type`, `weight`)",
                             m["ironweaver"], m["networkx"], per_item(m, n_nodes, per)))
    m = run("graph_no_attrs")
    report.memory.append(Row("Resident graph, no attributes", "structure only",
                             m["ironweaver"], m["networkx"], per_item(m, n_nodes, per)))
    m = run("load_json")
    report.memory.append(Row("Peak while loading JSON", "graph + parser buffers, from a file",
                             m["ironweaver"], m["networkx"],
                             "`load_from_json` vs `json.load` + `node_link_graph`"))
    m = run("save_json")
    report.memory.append(Row("Extra peak while saving JSON", "on top of the resident graph",
                             m["ironweaver"], m["networkx"],
                             "`save_to_json` vs `node_link_data` + `json.dump`"))

    report.files.append(Row("JSON file", "compact JSON", files["ironweaver"][0], files["networkx"][0],
                            "`save_to_json` vs `node_link_data` + `json.dump`"))
    report.files.append(Row("Binary file", "native binary format", files["ironweaver"][1], files["networkx"][1],
                            "`save_to_binary` (bincode) vs `pickle` (highest protocol)"))
    return report


def per_item(m: dict[str, int | None], n_nodes: int, label: str) -> str:
    parts = [f"{lib}: {fmt_bytes(m[lib] / n_nodes)}" for lib in LIBS if m[lib] is not None]
    return f"{', '.join(parts)} {label}" if parts else ""


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

def fmt_bytes(n: float | None) -> str:
    if n is None:
        return "–"
    for unit, size in (("GiB", 1 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)):
        if abs(n) >= size:
            return f"{n / size:,.1f} {unit}"
    return f"{n:,.0f} B"


def fmt_ratio(iw: int | None, nx_: int | None) -> str:
    if not iw or not nx_ or iw <= 0 or nx_ <= 0:
        return "–"
    ratio = nx_ / iw
    if ratio >= 1:
        return f"**{ratio:,.1f}× smaller**"
    return f"{1 / ratio:,.1f}× larger"


def table(rows: list[Row]) -> list[str]:
    lines = [
        "| Measurement | Description | ironweaver | networkx | ironweaver is | Notes |",
        "|---|---|---:|---:|---:|---|",
    ]
    for r in rows:
        lines.append(f"| {r.name} | {r.description} | {fmt_bytes(r.ironweaver)} | {fmt_bytes(r.networkx)} "
                     f"| {fmt_ratio(r.ironweaver, r.networkx)} | {r.note} |")
    return lines


def render(reports: list[SizeReport], repeats: int, seed: int) -> str:
    import networkx as nx

    lines = [
        "# ironweaver vs networkx: memory",
        "",
        f"Generated {datetime.datetime.now().strftime('%Y-%m-%d %H:%M')} by `benchmarks/compare_networkx_memory.py`.",
        "",
        "| | |",
        "|---|---|",
        f"| Python | {platform.python_version()} ({platform.python_implementation()}) |",
        f"| networkx | {nx.__version__} (`MultiDiGraph`) |",
        f"| Platform | {platform.platform()} |",
        f"| Repeats | median of {repeats}, each in a fresh process |",
        f"| Seed | {seed} |",
        "",
        "Both libraries hold the same random directed multigraph as in",
        "[networkx_comparison.md](networkx_comparison.md). Memory is the growth of the",
        "process's resident set size (RSS), measured in a fresh interpreter per",
        "measurement, because most of ironweaver's memory lives in Rust allocations",
        "that `tracemalloc` cannot see. Peaks use the kernel's high-water mark",
        "(`VmHWM`) and are only available on Linux. \"ironweaver is\" compares",
        "ironweaver with networkx (networkx ÷ ironweaver).",
        "",
    ]
    for rep in reports:
        lines += [f"## {rep.nodes:,} nodes, {rep.edges:,} edges", "", "### In memory", ""]
        lines += table(rep.memory)
        lines += ["", "### On disk", ""]
        lines += table(rep.files)
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
    argv = sys.argv[1:] if argv is None else argv
    if argv and argv[0] == "--worker":
        lib, scenario, n_nodes, n_edges, seed, path = argv[1:7]
        print(json.dumps(worker(lib, scenario, int(n_nodes), int(n_edges), int(seed), path)))
        return ""

    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--sizes", default="10000:50000,100000:500000",
                        help="comma-separated NODES[:EDGES] graph sizes (default: %(default)s)")
    parser.add_argument("--repeats", type=int, default=3,
                        help="fresh-process runs per measurement; the median is reported (default: %(default)s)")
    parser.add_argument("--seed", type=int, default=42, help="random seed (default: %(default)s)")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="Markdown file to write (default: %(default)s)")
    args = parser.parse_args(argv)

    reports = []
    with tempfile.TemporaryDirectory(prefix="iw_memory_") as workdir:
        for nodes, edges in parse_sizes(args.sizes):
            print(f"Measuring {nodes:,} nodes / {edges:,} random edges ...", file=sys.stderr)
            reports.append(run_size(nodes, edges, args.repeats, args.seed, workdir))

    markdown = render(reports, args.repeats, args.seed)
    os.makedirs(os.path.dirname(os.path.abspath(args.output)), exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as fh:
        fh.write(markdown)
    print(markdown)
    print(f"\nWrote {args.output}", file=sys.stderr)
    return args.output


if __name__ == "__main__":
    main()
