from __future__ import annotations

from typing import Any, Iterable, Mapping

from ._ironweaver import Vertex


def _format_scalar(value: Any) -> str:
    """Format a scalar attribute value so that ``_parse_value`` reads it back."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        if "\n" in value or "\r" in value:
            raise ValueError(f"LGF cannot represent multi-line strings: {value!r}")
        return f'"{value}"'
    raise ValueError(f"LGF cannot represent values of type {type(value).__name__}")


def _format_attr(key: str, value: Any, indent: str) -> list[str]:
    """Format one ``key = value`` attribute, using the multi-line form for lists."""
    if isinstance(value, (list, tuple)):
        if not value:
            return [f"{indent}{key} = []"]
        lines = [f"{indent}{key} = ["]
        for index, item in enumerate(value):
            formatted = _format_scalar(item)
            if "]" in formatted:
                # The parser ends a multi-line list at the first ']' it sees.
                raise ValueError(f"LGF list items cannot contain ']': {item!r}")
            comma = "," if index < len(value) - 1 else ""
            lines.append(f"{indent}  {formatted}{comma}")
        lines.append(f"{indent}]")
        return lines
    return [f"{indent}{key} = {_format_scalar(value)}"]


def _check_token(token: str, what: str) -> None:
    if not token or any(ch.isspace() for ch in token):
        raise ValueError(f"LGF {what} must be non-empty and contain no whitespace: {token!r}")


def to_lgf(
    graph: Vertex,
    order: Iterable[str] | None = None,
    banners: Mapping[str, str] | None = None,
    header: str | None = None,
) -> str:
    """Serialize a :class:`Vertex` graph to LGF text.

    The output is the inverse of :func:`parse_lgf`: parsing it again yields a
    graph with the same nodes, labels, attributes and edges.

    Parameters
    ----------
    graph:
        The graph to serialize.
    order:
        Node IDs in the order they should be written. Nodes not listed are
        appended in ``graph.keys()`` order.
    banners:
        Optional comment banners, keyed by the node ID they precede.
    header:
        Optional comment written at the top of the file (may span lines).

    Returns
    -------
    str
        The LGF text.

    Raises
    ------
    ValueError
        If a node ID, label, edge type or value cannot be represented in LGF.
    """
    banners = banners or {}
    ordered: list[str] = []
    seen: set[str] = set()
    for node_id in list(order or []) + list(graph.keys()):
        if node_id not in seen and graph.has_node(node_id):
            seen.add(node_id)
            ordered.append(node_id)

    lines: list[str] = []
    if header:
        lines.extend(f"# {line}".rstrip() for line in header.splitlines())
        lines.append("")

    for node_id in ordered:
        node = graph.get_node(node_id)
        attrs = dict(node.attr)
        labels = list(attrs.pop("labels", []) or [])
        _check_token(node_id, "node IDs")
        for label in labels:
            _check_token(label, "labels")

        if node_id in banners:
            lines.append("# " + "=" * 76)
            lines.append(f"# {banners[node_id]}")
            lines.append("# " + "=" * 76)

        lines.append(" ".join([node_id, *labels]))
        for key, value in attrs.items():
            lines.extend(_format_attr(key, value, "  "))
        for edge in node.edges:
            edge_attrs = dict(edge.attr)
            edge_type = edge_attrs.pop("type", None)
            if not isinstance(edge_type, str):
                raise ValueError(f"LGF edges need a string 'type' (edge from {node_id!r})")
            _check_token(edge_type, "edge types")
            lines.append(f"  -{edge_type}-> {edge.to_node.id}")
            for key, value in edge_attrs.items():
                lines.extend(_format_attr(key, value, "    "))
        lines.append("")

    return "\n".join(lines)


def to_lgf_file(graph: Vertex, path: str, **kwargs: Any) -> None:
    """Write ``graph`` as LGF to ``path``. Keyword arguments go to :func:`to_lgf`."""
    with open(path, "w", encoding="utf-8") as f:
        f.write(to_lgf(graph, **kwargs))


__all__ = ["to_lgf", "to_lgf_file"]
