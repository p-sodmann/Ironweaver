"""Attribute value types beyond JSON's: bytes, dates and date-times keep
their types through saving and loading, and compare in filter expressions."""

import datetime as dt
import json

import pytest

import ironweaver as iw

UTC = dt.timezone.utc
PLUS2 = dt.timezone(dt.timedelta(hours=2))
MINUS = dt.timezone(-dt.timedelta(hours=3, minutes=30, seconds=15))

VALUES = {
    "bytes": b"\x00\xffhi",
    "empty": b"",
    "array": bytearray(b"ab"),
    "date": dt.date(2024, 2, 29),
    "old": dt.date(1, 1, 1),
    "aware": dt.datetime(2024, 5, 1, 12, 30, 5, 250, tzinfo=PLUS2),
    "utc": dt.datetime(1969, 7, 20, 20, 17, 40, tzinfo=UTC),
    "odd": dt.datetime(1901, 1, 1, tzinfo=MINUS),
    "naive": dt.datetime(9999, 12, 31, 23, 59, 59, 999999),
    "nested": [dt.date(2000, 1, 1), {"b": b"x", "t": dt.datetime(2000, 1, 1)}],
}


def roundtrips(tmp_path):
    g = iw.Vertex()
    g.add_node("n", VALUES)
    g.add_edge("n", "n", {"when": VALUES["aware"]})
    g.meta["saved"] = VALUES["date"]
    for name, save, load in [
        ("g.json", g.save_to_json, iw.Vertex.load_from_json),
        ("g.bin", g.save_to_binary, iw.Vertex.load_from_binary),
        ("g16.bin", g.save_to_binary_f16, iw.Vertex.load_from_binary),
    ]:
        path = str(tmp_path / name)
        save(path)
        yield name, load(path)


def test_types_survive_saving(tmp_path):
    for name, loaded in roundtrips(tmp_path):
        attr = loaded["n"].attr
        for key, value in VALUES.items():
            want = bytes(value) if isinstance(value, bytearray) else value
            assert attr[key] == want, (name, key)
            assert type(attr[key]) is type(want), (name, key)
        # Offsets are kept (as fixed offsets), naive stays naive
        assert attr["aware"].utcoffset() == dt.timedelta(hours=2)
        assert attr["odd"].utcoffset() == MINUS.utcoffset(None)
        assert attr["naive"].tzinfo is None
        assert attr["nested"][1]["t"].tzinfo is None
        assert loaded["n"].edges[0].attr["when"] == VALUES["aware"]
        assert loaded.meta["saved"] == VALUES["date"]


def test_json_encoding(tmp_path):
    g = iw.Vertex()
    g.add_node("n", {"b": b"hi", "d": dt.date(2024, 5, 1), "t": VALUES["aware"], "l": dt.datetime(2024, 5, 1)})
    path = tmp_path / "g.json"
    g.save_to_json(str(path))
    attr = json.loads(path.read_text())["nodes"]["n"]["attr"]
    assert attr == {
        "b": {"Bytes": "aGk="},
        "d": {"Date": "2024-05-01"},
        "t": {"DateTime": "2024-05-01T12:30:05.000250+02:00"},
        "l": {"DateTime": "2024-05-01T00:00:00"},
    }


def test_zone_names_become_offsets(tmp_path):
    zoneinfo = pytest.importorskip("zoneinfo")
    try:
        berlin = zoneinfo.ZoneInfo("Europe/Berlin")
    except zoneinfo.ZoneInfoNotFoundError:
        pytest.skip("no time zone database")
    t = dt.datetime(2024, 7, 1, 12, tzinfo=berlin)
    g = iw.Vertex()
    g.add_node("n", {"t": t})
    path = str(tmp_path / "g.bin")
    g.save_to_binary(path)
    loaded = iw.Vertex.load_from_binary(path)["n"].attr["t"]
    assert loaded == t and loaded.utcoffset() == dt.timedelta(hours=2)


def test_sub_second_offsets_are_refused(tmp_path):
    g = iw.Vertex()
    g.add_node("n", {"t": dt.datetime(2024, 1, 1, tzinfo=dt.timezone(dt.timedelta(microseconds=1)))})
    with pytest.raises(RuntimeError, match="microseconds"):
        g.save_to_json(str(tmp_path / "g.json"))


def test_expressions_compare_dates_and_bytes():
    g = iw.Vertex()
    g.add_node("a", {"d": dt.date(2024, 1, 1), "t": dt.datetime(2024, 1, 1, 12, tzinfo=UTC), "b": b"a"})
    g.add_node("b", {"d": dt.date(2025, 1, 1), "t": dt.datetime(2024, 1, 1, 13, tzinfo=PLUS2), "b": b"b"})
    g.add_node("c", {"t": dt.datetime(2024, 1, 1, 12)})

    def ids(expr):
        return sorted(g.filter(where=expr).nodes)

    assert ids(iw.attr("d") > dt.date(2024, 6, 1)) == ["b"]
    assert ids(iw.attr("d") == dt.date(2024, 1, 1)) == ["a"]
    # 13:00+02:00 is 11:00 UTC: earlier than a's 12:00 UTC
    assert ids(iw.attr("t") < dt.datetime(2024, 1, 1, 11, 30, tzinfo=UTC)) == ["b"]
    assert ids(iw.attr("t") == dt.datetime(2024, 1, 1, 14, tzinfo=PLUS2)) == ["a"]
    # Naive and aware date-times don't compare (as in Python)
    assert ids(iw.attr("t") >= dt.datetime(2000, 1, 1)) == ["c"]
    assert ids(iw.attr("b") == b"b") == ["b"]
    assert ids(iw.attr("b").is_in([b"a", b"z"])) == ["a"]
    assert repr(iw.attr("d") == dt.date(2024, 1, 1)) == '(attr("d") == date(2024-01-01))'


def test_special_floats_survive_saving(tmp_path):
    import math

    floats = [0.0, -0.0, math.nan, math.inf, -math.inf, 5e-324, 1.7976931348623157e308]
    for name, save, load in [
        ("g.json", "save_to_json", iw.Vertex.load_from_json),
        ("g.bin", "save_to_binary", iw.Vertex.load_from_binary),
    ]:
        g = iw.Vertex()
        g.add_node("n", {"floats": floats, "neg_zero": -0.0})
        path = str(tmp_path / name)
        getattr(g, save)(path)
        attr = load(path)["n"].attr
        for got, want in zip(attr["floats"], floats):
            assert got == want or (math.isnan(got) and math.isnan(want)), (name, want)
            assert math.copysign(1, got) == math.copysign(1, want) or math.isnan(want), (name, want)
        assert math.copysign(1, attr["neg_zero"]) == -1, name


def test_non_finite_floats_in_json(tmp_path):
    g = iw.Vertex()
    g.add_node("n", {"nan": float("nan"), "inf": float("inf"), "ninf": float("-inf"), "zero": -0.0})
    path = tmp_path / "g.json"
    g.save_to_json(str(path))
    text = path.read_text()
    assert '{"Float":"NaN"}' in text and '{"Float":"-Infinity"}' in text and '{"Float":-0.0}' in text
    attr = json.loads(text)["nodes"]["n"]["attr"]
    assert attr["inf"] == {"Float": "Infinity"}
    # Graph dicts load the same, also with the floats themselves in the tags
    for doc in (attr, {k: {"Float": float(v["Float"])} if isinstance(v["Float"], str) else v for k, v in attr.items()}):
        loaded = json.loads(text)
        loaded["nodes"]["n"]["attr"] = doc
        back = iw.Vertex.load_from_json(loaded)["n"].attr
        assert back["inf"] == float("inf") and back["ninf"] == float("-inf") and back["nan"] != back["nan"]
        assert str(back["zero"]) == "-0.0"
