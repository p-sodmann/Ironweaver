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


def _parse_list_item(item: str):
    """Parse a single list item, handling quoted strings and other types."""
    item = item.strip().rstrip(",")
    return _parse_value(item)


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
    
    # State for multi-line list parsing
    list_key = None
    list_items = []
    list_indent = 0
    in_list = False

    for raw_line in text.splitlines():
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(raw_line) - len(raw_line.lstrip())

        # Handle multi-line list continuation
        if in_list:
            # Check if this line closes the list
            if "]" in stripped:
                # Extract any items before the closing bracket
                before_bracket = stripped[:stripped.index("]")]
                if before_bracket.strip():
                    list_items.append(_parse_list_item(before_bracket))
                
                # Save the completed list
                if current_edge is not None and list_indent > edge_indent:
                    attrs = dict(current_edge.attr)
                    attrs[list_key] = list_items
                    current_edge.attr = attrs
                else:
                    current_node.attr_set(list_key, list_items)
                
                # Reset list state
                in_list = False
                list_key = None
                list_items = []
                list_indent = 0
                continue
            else:
                # Add item to list (strip commas)
                if stripped:
                    list_items.append(_parse_list_item(stripped))
                continue

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

        # Handle new forward syntax: -relationship-> target
        if stripped.startswith("-") and "->" in stripped:
            # Extract relationship and target from -relationship-> target format
            arrow_pos = stripped.find("->")
            if arrow_pos > 1:  # Must have at least one character for relationship
                relationship_part = stripped[1:arrow_pos]  # Remove leading '-'
                target_part = stripped[arrow_pos + 2:].strip()  # Remove '->' and strip
                
                # Remove trailing dash if present (for -relationship-> format)
                if relationship_part.endswith("-"):
                    relationship = relationship_part[:-1]  # Remove trailing '-'
                else:
                    relationship = relationship_part  # Use as-is for -relationship-> format
                
                target = target_part
                
                if relationship and target:  # Ensure both are non-empty
                    if not graph.has_node(target):
                        graph.add_node(target, {})
                    current_edge = graph.add_edge(current_node.id, target, {"type": relationship})
                    edge_indent = indent
                    continue

        # Handle new inverse syntax: <-relationship- target
        if stripped.startswith("<-") and stripped.count("-") >= 2:
            # Extract relationship and target from <-relationship- target format
            rest = stripped[2:]  # Remove '<-'
            if "-" in rest:
                dash_pos = rest.rfind("-")  # Find the last dash
                if dash_pos > 0:  # Must have at least one character for relationship
                    relationship = rest[:dash_pos]
                    target = rest[dash_pos + 1:].strip()
                    
                    if target and relationship:
                        if not graph.has_node(target):
                            graph.add_node(target, {})
                        # Create edge from target to current_node (inverse direction)
                        current_edge = graph.add_edge(target, current_node.id, {"type": relationship})
                        edge_indent = indent
                        continue

        key, _, value = stripped.partition("=")
        key = key.strip()
        value_str = value.strip()

        # Check if this is the start of a multi-line list
        if value_str.startswith("["):
            if value_str.endswith("]"):
                # Single-line list: [item1, item2, ...]
                inner = value_str[1:-1]
                if inner.strip():
                    # Parse comma-separated items
                    items = []
                    for item in inner.split(","):
                        item = item.strip()
                        if item:
                            items.append(_parse_value(item))
                    value = items
                else:
                    value = []
            else:
                # Multi-line list starting
                in_list = True
                list_key = key
                list_items = []
                list_indent = indent
                # Check if there are items on the opening line after '['
                after_bracket = value_str[1:].strip()
                if after_bracket:
                    list_items.append(_parse_list_item(after_bracket))
                continue
        else:
            value = _parse_value(value_str)

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
