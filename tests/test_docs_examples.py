"""Run the Python examples in the documentation.

Every ```python block in README.md, llms.txt and docs/*.md is executed, in
order, in one namespace per file (later blocks may use names defined by
earlier ones), inside a temporary working directory. Reference material that
is not meant to run (signatures, sketches) must use a ```text fence.

matplotlib is replaced by a stub and networkx drawing is disabled, so the
visualisation examples run without a display or the optional dependency.
"""

import contextlib
import io
import os
import re
import sys
import types

import pytest

try:
    import ironweaver  # noqa: F401
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOC_FILES = ["README.md", "llms.txt"] + sorted(
    os.path.join("docs", f) for f in os.listdir(os.path.join(ROOT, "docs")) if f.endswith(".md")
)
BLOCK = re.compile(r"^```python\n(.*?)^```", re.S | re.M)

# Files some examples read; created in the temporary working directory.
LGF_SAMPLE = """\
alice Person
  name = "Alice"
  -knows-> bob
bob Person
"""

def _complex_lgf_example():
    """The "Complex Example" from docs/LGF.md, which its later examples load
    as social_network.lgf."""
    with open(os.path.join(ROOT, "docs", "LGF.md"), encoding="utf-8") as fh:
        match = re.search(r"### Complex Example.*?```lgf\n(.*?)```", fh.read(), re.S)
    return match.group(1) if match else LGF_SAMPLE


FIXTURE_FILES = {
    "graph.lgf": LGF_SAMPLE,
    "my_graph.lgf": LGF_SAMPLE,
    "social_network.lgf": _complex_lgf_example(),
}


@pytest.fixture
def sandbox(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    for name, content in FIXTURE_FILES.items():
        (tmp_path / name).write_text(content, encoding="utf-8")

    plt = types.ModuleType("matplotlib.pyplot")
    plt.__getattr__ = lambda name: (lambda *a, **k: None)
    mpl = types.ModuleType("matplotlib")
    mpl.pyplot = plt
    monkeypatch.setitem(sys.modules, "matplotlib", mpl)
    monkeypatch.setitem(sys.modules, "matplotlib.pyplot", plt)
    try:
        import networkx as nx
        monkeypatch.setattr(nx, "draw", lambda *a, **k: None, raising=False)
    except ImportError:
        pass
    return tmp_path


@pytest.mark.parametrize("doc", DOC_FILES)
def test_examples_run(doc, sandbox):
    with open(os.path.join(ROOT, doc), encoding="utf-8") as fh:
        blocks = BLOCK.findall(fh.read())
    assert blocks, f"{doc} has no python examples"
    if any("networkx" in b or "to_networkx" in b for b in blocks):
        pytest.importorskip("networkx")

    namespace = {"__name__": "__docs__"}
    for i, block in enumerate(blocks):
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                exec(compile(block, f"{doc} (python block {i + 1})", "exec"), namespace)
        except Exception as e:
            first = block.strip().splitlines()[0]
            pytest.fail(f"{doc}, python block {i + 1} ({first!r}) failed: {type(e).__name__}: {e}")


def test_documented_gotchas_hold():
    """The copy/live rules stated in the docs' Gotchas section."""
    from ironweaver import Vertex

    g = Vertex()
    a = g.add_node("a", {"x": 1})
    g.add_node("b")
    e = g.add_edge("a", "b", {"type": "t"})

    # Attribute dicts, edge lists and vertex.nodes are copies
    a.attr["x"] = 2
    assert a.attr == {"x": 1}
    e.attr["type"] = "u"
    assert e.attr == {"type": "t"}
    a.edges.append(e)
    assert len(a.edges) == 1
    g.nodes["c"] = a
    assert "c" not in g

    # ... so use attr_set or assign a whole dict
    a.attr_set("x", 3)
    assert a.attr_get("x") == 3
    a.attr = {"x": 4}
    assert a.attr == {"x": 4}

    # vertex.meta and callback lists are live
    g.meta["k"] = 1
    assert g.meta["k"] == 1
    g.on_node_add_callbacks.append(lambda v, n: None)
    assert len(g.on_node_add_callbacks) == 1

    # Nodes are handles: equal (not necessarily identical) per node
    assert g["a"] == a and hash(g["a"]) == hash(a)
    assert a.vertex is g

    # bfs/traverse/filter results hold copies of the nodes
    for sub in (a.bfs(), a.traverse(), g.filter(ids=["a", "b"])):
        assert sub["a"] != a
        sub["a"].attr_set("x", 99)
        assert a.attr_get("x") == 4
    sub = g.filter(ids=["a", "b"])
    # ... but a filter result shares vertex.meta with its source
    assert sub.meta is g.meta
