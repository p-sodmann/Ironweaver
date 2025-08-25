from __future__ import annotations

import os

from ._ironweaver import Vertex


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


def parse_lgf(
    text: str,
    graph: Vertex | None = None,
    base_path: str | None = None,
) -> Vertex:
    """Parse LGF text into a :class:`Vertex` graph.

    Parameters
    ----------
    text:
        Input text in LGF format.
    graph:
        Existing graph to add nodes and edges to. If ``None``, a new graph is
        created.
    base_path:
        Base path used to resolve relative ``import`` statements.

    Returns
    -------
    Vertex
        The parsed graph.
    """
    graph = graph or Vertex()
    base_path = base_path or ""
    current_node = None
    current_edge = None
    edge_indent = 0

    for raw_line in text.splitlines():
        stripped = raw_line.strip()
        if not stripped or stripped == "#":
            continue
        indent = len(raw_line) - len(raw_line.lstrip())

        if indent == 0 and stripped.startswith("import(") and stripped.endswith(")"):
            import_path = stripped[len("import(") : -1].strip()
            if (import_path.startswith("\"") and import_path.endswith("\"")) or (
                import_path.startswith("'") and import_path.endswith("'")
            ):
                import_path = import_path[1:-1]
            full_path = os.path.join(base_path, import_path)
            with open(full_path, "r", encoding="utf-8") as f:
                imported_text = f.read()
            imported_base = os.path.dirname(full_path)
            parse_lgf(imported_text, graph=graph, base_path=imported_base)
            current_node = None
            current_edge = None
            edge_indent = 0
            continue

        if indent == 0:
            parts = stripped.split()
            node_id = parts[0]
            labels = parts[1:] if len(parts) > 1 else []
            
            if graph.has_node(node_id):
                current_node = graph.get_node(node_id)
                # Update labels for existing node
                current_node.attr_set("labels", labels)
            else:
                attrs = {"labels": labels}
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


def parse_lgf_file(path: str) -> Vertex:
    """Parse an LGF file from ``path`` into a :class:`Vertex` graph."""
    with open(path, "r", encoding="utf-8") as f:
        text = f.read()
    return parse_lgf(text, base_path=os.path.dirname(path))


__all__ = ["parse_lgf", "parse_lgf_file"]
