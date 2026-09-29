"""Deeply nested values and crash-safe saving."""

import os
import struct

import pytest

from ironweaver import Vertex

MAX_DEPTH = 100  # ironweaver_core::format::MAX_DEPTH


def nested(depth):
    """A value `depth` levels deep: depth - 1 lists around a string."""
    value = "MARK"
    for _ in range(depth - 1):
        value = [value]
    return value


def graph_with(value):
    g = Vertex()
    g.add_node("a", {"k": value})
    return g


def test_deepest_value_round_trips(tmp_path):
    g = graph_with(nested(MAX_DEPTH))
    assert Vertex.load_from_json(g.save_to_json())["a"].attr["k"] == nested(MAX_DEPTH)
    g.save_to_binary(str(tmp_path / "g.bin"))
    assert Vertex.load_from_binary(str(tmp_path / "g.bin"))["a"].attr["k"] == nested(MAX_DEPTH)


@pytest.mark.parametrize("value", [nested(MAX_DEPTH + 1), nested(100_000)])
def test_too_deep_values_raise_instead_of_crashing(tmp_path, value):
    g = graph_with(value)
    with pytest.raises(RuntimeError, match="nested more than"):
        g.save_to_json()
    with pytest.raises(RuntimeError, match="nested more than"):
        g.save_to_binary(str(tmp_path / "g.bin"))
    assert not os.path.exists(tmp_path / "g.bin")


def test_self_containing_value_raises():
    loop = []
    loop.append(loop)
    with pytest.raises(RuntimeError, match="containing themselves"):
        graph_with(loop).save_to_json()


def test_crafted_deep_binary_file_raises(tmp_path):
    path = tmp_path / "g.bin"
    graph_with(["MARK"]).save_to_binary(str(path))
    data = path.read_bytes()
    level = struct.pack("<IQ", 6, 1)  # tag of a one-item list, then its length
    at = data.index(b"MARK") - 12 - len(level)
    assert data[at:at + len(level)] == level
    path.write_bytes(data[:at] + level * 100_000 + data[at:])
    with pytest.raises(RuntimeError, match="nested more than"):
        Vertex.load_from_binary(str(path))


def test_graph_dict_loop_raises():
    doc = {}
    doc["nodes"] = doc
    with pytest.raises(RuntimeError, match="nested too deeply"):
        Vertex.load_from_json(doc)


@pytest.mark.parametrize("fmt", ["json", "binary"])
def test_failed_save_keeps_the_previous_file(tmp_path, fmt):
    path = tmp_path / f"g.{fmt}"
    save = "save_to_json" if fmt == "json" else "save_to_binary"
    load = Vertex.load_from_json if fmt == "json" else Vertex.load_from_binary
    good = graph_with("ok")
    getattr(good, save)(str(path))
    before = path.read_bytes()

    with pytest.raises(RuntimeError):
        getattr(graph_with(nested(MAX_DEPTH + 1)), save)(str(path))
    assert path.read_bytes() == before
    assert load(str(path))["a"].attr["k"] == "ok"
    assert os.listdir(tmp_path) == [path.name]  # no temporary file left behind


def test_save_overwrites_and_keeps_permissions(tmp_path):
    path = tmp_path / "g.json"
    path.write_text("old")
    os.chmod(path, 0o600)
    graph_with("new").save_to_json(str(path))
    assert Vertex.load_from_json(str(path))["a"].attr["k"] == "new"
    if os.name == "posix":
        assert os.stat(path).st_mode & 0o777 == 0o600
