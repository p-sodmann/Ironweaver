"""Test edge filtering on traversal methods."""

from ironweaver import Vertex


def build_graph():
    v = Vertex()
    v.add_node("a", {})
    v.add_node("b", {})
    v.add_node("c", {})
    v.add_node("d", {})
    v.add_edge("a", "b", {"type": "knows"})
    v.add_edge("a", "c", {"type": "follows"})
    v.add_edge("b", "d", {"type": "knows"})
    v.add_edge("c", "d", {"type": "follows"})
    return v


def test_dict_filter():
    v = build_graph()
    result = v.get_node("a").traverse(depth=3, filter={"type": "knows"})
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "b", "d"], f"Expected ['a','b','d'] got {keys}"
    print("PASS: dict filter")


def test_lambda_traverse():
    v = build_graph()
    result = v.get_node("a").traverse(depth=3, filter=lambda e: e.type == "knows")
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "b", "d"], f"Expected ['a','b','d'] got {keys}"
    print("PASS: lambda traverse")


def test_lambda_bfs():
    v = build_graph()
    result = v.get_node("a").bfs(depth=3, filter=lambda e: e.type == "follows")
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "c", "d"], f"Expected ['a','c','d'] got {keys}"
    print("PASS: lambda bfs")


def test_lambda_complex():
    v = build_graph()
    result = v.get_node("a").traverse(
        depth=3, filter=lambda e: e.type in ("knows", "follows")
    )
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "b", "c", "d"], f"Expected all nodes got {keys}"
    print("PASS: lambda complex")


def test_edge_filter_kwarg():
    v = build_graph()
    result = v.get_node("a").bfs(
        depth=3, edge_filter=lambda e: e.attr("type") == "knows"
    )
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "b", "d"], f"Expected ['a','b','d'] got {keys}"
    print("PASS: edge_filter kwarg")


def test_bfs_search_lambda():
    v = build_graph()
    found = v.get_node("a").bfs_search(
        "d", depth=5, filter=lambda e: e.type == "knows"
    )
    assert found is not None and found.id == "d", f"Expected node d, got {found}"
    print("PASS: bfs_search lambda (knows)")

    found = v.get_node("a").bfs_search(
        "d", depth=5, filter=lambda e: e.type == "follows"
    )
    assert found is not None and found.id == "d", f"Expected node d, got {found}"
    print("PASS: bfs_search lambda (follows)")


def test_no_filter():
    v = build_graph()
    result = v.get_node("a").traverse(depth=3)
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "b", "c", "d"], f"Expected all nodes got {keys}"
    print("PASS: no filter")


def test_has_attr():
    v = build_graph()
    result = v.get_node("a").traverse(
        depth=3, filter=lambda e: e.has_attr("type") and e.type == "knows"
    )
    keys = sorted(result.nodes.keys())
    assert keys == ["a", "b", "d"], f"Expected ['a','b','d'] got {keys}"
    print("PASS: has_attr")


if __name__ == "__main__":
    test_dict_filter()
    test_lambda_traverse()
    test_lambda_bfs()
    test_lambda_complex()
    test_edge_filter_kwarg()
    test_bfs_search_lambda()
    test_no_filter()
    test_has_attr()
    print("\nAll tests passed!")
