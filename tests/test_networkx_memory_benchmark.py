"""Smoke test for benchmarks/compare_networkx_memory.py.

Runs the memory comparison on a tiny graph so the script (and its fresh-process
workers) keeps working as the library evolves.
"""

import importlib.util
import os
import sys

import pytest

pytest.importorskip("networkx")
pytest.importorskip("ironweaver")

SCRIPT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                      "benchmarks", "compare_networkx_memory.py")


def load_benchmark():
    spec = importlib.util.spec_from_file_location("compare_networkx_memory", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module  # dataclasses resolve annotations via sys.modules
    spec.loader.exec_module(module)
    return module


def test_builders_produce_the_same_graph():
    bench = load_benchmark()
    iw = bench.build_ironweaver(50, 200, seed=7)
    g = bench.build_networkx(50, 200, seed=7)
    assert len(iw) == g.number_of_nodes() == 50
    assert iw.get_metadata()["edge_count"] == g.number_of_edges() == 249
    assert iw["n3"].attr == g.nodes["n3"]


def test_memory_benchmark_writes_markdown_table(tmp_path):
    bench = load_benchmark()
    out = tmp_path / "memory.md"
    bench.main(["--sizes", "300:1200", "--repeats", "1", "--output", str(out)])

    text = out.read_text(encoding="utf-8")
    assert text.startswith("# ironweaver vs networkx: memory")
    assert "## 300 nodes, 1,499 edges" in text
    assert "| Measurement | Description | ironweaver | networkx | ironweaver is | Notes |" in text
    for name in ("Resident graph", "Resident graph, no attributes", "Peak while loading JSON",
                 "Extra peak while saving JSON", "JSON file", "Binary file"):
        assert f"| {name} |" in text
