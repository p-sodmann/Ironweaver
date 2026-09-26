"""Smoke test for benchmarks/compare_networkx.py.

Runs the comparison on a tiny graph so the benchmark keeps working (and its
ironweaver/networkx result checks keep passing) as the library evolves.
"""

import importlib.util
import os
import sys

import pytest

pytest.importorskip("networkx")
pytest.importorskip("ironweaver")

SCRIPT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "benchmarks", "compare_networkx.py")


def load_benchmark():
    spec = importlib.util.spec_from_file_location("compare_networkx", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module  # dataclasses resolve annotations via sys.modules
    spec.loader.exec_module(module)
    return module


def test_benchmark_writes_markdown_table(tmp_path, capsys):
    bench = load_benchmark()
    out = tmp_path / "comparison.md"
    bench.main(["--sizes", "200:800", "--repeats", "1", "--output", str(out)])

    text = out.read_text(encoding="utf-8")
    assert text.startswith("# ironweaver vs networkx")
    assert "## 200 nodes, 999 edges" in text
    assert "| Operation | Description | ironweaver | networkx | Speedup | Notes |" in text
    for name in ("Build graph", "BFS (full)", "Shortest path (Dijkstra)", "Subgraph by ids",
                 "Remove nodes", "Random walks", "Load from JSON string"):
        assert f"| {name} |" in text
