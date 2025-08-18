from __future__ import annotations

from ironweaver import Vertex


def _parse_value(value: str):
    value = value.strip()
    if value.isdigit():
        return int(value)
    try:
        return float(value)
    except ValueError:
        pass
    if (value.startswith("\"") and value.endswith("\"")) or (
        value.startswith("'") and value.endswith("'")
    ):
        return value[1:-1]
    if value.lower() in {"true", "false"}:
        return value.lower() == "true"
    return value


def parse_igf3(text: str) -> Vertex:
    """Parse LGF text into a :class:`Vertex` graph.

    Parameters
    ----------
    text:
        Input text in LGF format.

    Returns
    -------
    Vertex
        The parsed graph.
    """
    graph = Vertex()
    current_node = None
    current_edge = None
    edge_indent = 0

    for raw_line in text.splitlines():
        stripped = raw_line.strip()
        if not stripped or stripped == "#":
            continue
        indent = len(raw_line) - len(raw_line.lstrip())

        if indent == 0:
            parts = stripped.split()
            node_id = parts[0]
            labels = parts[1:]
            attrs = {"labels": labels} if labels else {}
            if graph.has_node(node_id):
                current_node = graph.get_node(node_id)
            else:
                graph.add_node(node_id, attrs)
                current_node = graph.get_node(node_id)
            current_edge = None
            continue

        if stripped.startswith("->"):
            rest = stripped[2:].strip()
            target, typ = rest.split(None, 1)
            if not graph.has_node(target):
                graph.add_node(target, {})
            current_edge = graph.add_edge(current_node.id, target, {"type": typ})
            edge_indent = indent
            continue

        key, _, value = stripped.partition("=")
        key = key.strip()
        value = _parse_value(value)

        if current_edge is not None and indent > edge_indent:
            attrs = dict(current_edge.attr)
            attrs[key] = value
            current_edge.attr = attrs
        else:
            current_node.attr_set(key, value)
            current_edge = None

    return graph
