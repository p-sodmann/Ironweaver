"""
Type stubs for the ironweaver Rust extension module (_ironweaver.so).

These stubs mirror the exact PyO3-generated signatures. All five classes are
@final (PyO3 extension types cannot be subclassed). Constructors use __new__
because that is the slot PyO3 populates; at runtime __init__ takes no args.

Note: Vertex.filter, Node.traverse, Node.bfs, and Node.bfs_search reflect the
Python-level wrappers applied in ironweaver/__init__.py at import time.
"""

from __future__ import annotations

from typing import Any, Callable, Iterator, final

@final
class ObservedDictionary:
    """A dict-like container that fires per-key callbacks on value changes."""

    def __new__(
        cls,
        node: Any | None,
        callbacks: dict[str, list[Callable[..., Any]]] | None,
    ) -> ObservedDictionary: ...
    def __setitem__(self, key: str, value: Any, /) -> None: ...
    def __getitem__(self, key: str, /) -> Any: ...

@final
class Edge:
    """A directed, attributed edge between two nodes."""

    id: str | None
    from_node: Node
    to_node: Node
    attr: dict[str, Any]
    watched_by: list[Any]
    meta: dict[str, Any]
    on_meta_change_callbacks: list[Callable[..., Any]]
    on_update_callbacks: list[Callable[[Vertex | None, Edge, str, Any, Any | None], bool]]
    vertex: Vertex | None

    def __new__(
        cls,
        from_node: Node,
        to_node: Node,
        attr: dict[str, Any] | None,
        id: str | None,
    ) -> Edge: ...
    def __repr__(self) -> str: ...
    def toJSON(self) -> dict[str, Any]: ...
    def attr_set(self, key: str, value: Any) -> None:
        """Set attr[key] = value and fire on_update_callbacks if the value changed."""
        ...
    def attr_get(self, key: str) -> Any | None:
        """Return attr[key], or None if the key does not exist."""
        ...

@final
class Node:
    """A graph node with a string ID, an attribute dict, and directed edge lists."""

    id: str
    attr: dict[str, Any]
    edges: list[Edge]
    inverse_edges: list[Edge]
    meta: dict[str, Any]
    on_edge_add_callbacks: list[Callable[..., Any]]
    on_update_callbacks: list[Callable[[Vertex | None, Node, str, Any, Any | None], bool]]
    vertex: Vertex | None

    def __new__(
        cls,
        id: str,
        attr: dict[str, Any] | None,
        edges: list[Edge] | None,
    ) -> Node: ...
    def __repr__(self) -> str: ...
    def traverse(
        self,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[Any], bool] | None = ...,
        edge_filter: Callable[[Any], bool] | None = ...,
    ) -> Vertex:
        """DFS traversal. filter/edge_filter receive an EdgeView from the Python wrapper."""
        ...
    def bfs(
        self,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[Any], bool] | None = ...,
        edge_filter: Callable[[Any], bool] | None = ...,
    ) -> Vertex:
        """BFS traversal. filter/edge_filter receive an EdgeView from the Python wrapper."""
        ...
    def bfs_search(
        self,
        target_id: str,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[Any], bool] | None = ...,
        edge_filter: Callable[[Any], bool] | None = ...,
    ) -> Node | None:
        """BFS search for target_id. Returns the Node if found, None otherwise."""
        ...
    def attr_get(self, key: str) -> Any | None: ...
    def attr_set(self, key: str, value: Any) -> None: ...
    def attr_list_append(self, key: str, value: Any) -> None: ...

@final
class Path:
    """An ordered sequence of nodes representing a traversal path."""

    nodes: list[Node]

    def __new__(cls, nodes: list[Node] | None) -> Path: ...
    def __repr__(self) -> str: ...
    def toJSON(self) -> list[str]: ...

@final
class Vertex:
    """A directed property graph backed by a Rust HashMap."""

    nodes: dict[str, Node]
    meta: dict[str, Any]
    on_node_add_callbacks: list[Callable[[Vertex, Node], bool]]
    on_edge_add_callbacks: list[Callable[[Vertex, Edge], bool]]
    on_node_update_callbacks: list[Callable[[Vertex | None, Node, str, Any, Any | None], bool]]
    on_edge_update_callbacks: list[Callable[[Vertex | None, Edge, str, Any, Any | None], bool]]

    def __new__(cls) -> Vertex: ...
    def __getitem__(self, key: str, /) -> Node: ...
    def __iter__(self) -> Iterator[Node]: ...
    def __len__(self) -> int: ...
    def __contains__(self, key: str | Node, /) -> bool:
        """True if the node ID (or Node) exists. Added by the Python wrapper."""
        ...
    def __repr__(self) -> str: ...
    def keys(self) -> list[str]: ...
    def toJSON(self) -> dict[str, Any]: ...
    def has_node(self, id: str) -> bool: ...
    def node_count(self) -> int: ...
    def add_node(self, id: str, attr: dict[str, Any] | None) -> Node: ...
    def add_edge(self, from_id: str, to_id: str, attr: dict[str, Any] | None) -> Edge: ...
    def get_node(self, id: str) -> Node: ...
    def save_to_json(self, file_path: str | None = ...) -> str | None: ...
    def save_to_binary(self, file_path: str) -> None: ...
    def save_to_binary_f16(self, file_path: str) -> None: ...
    @staticmethod
    def load_from_json(source: str | dict[str, Any]) -> Vertex:
        """Load from a file path, a raw JSON string, or a plain dict."""
        ...
    @staticmethod
    def load_from_binary(file_path: str) -> Vertex: ...
    @staticmethod
    def from_nodes(nodes: dict[str, Node]) -> Vertex: ...
    @staticmethod
    def from_nodes_with_path(nodes: dict[str, Node], nodelist: list[str]) -> Vertex: ...
    def get_metadata(self) -> dict[str, Any]: ...
    def to_networkx(self) -> Any: ...
    def shortest_path_bfs(
        self,
        root_node_id: str,
        target_node_id: str,
        max_depth: int | None = ...,
    ) -> Vertex:
        """Ordered path is in ``result.meta["nodelist"]``. Raises ValueError if unreachable."""
        ...
    def expand(self, source_vertex: Vertex, depth: int | None = ...) -> Vertex: ...
    def filter(
        self,
        predicate: Callable[[Any], bool] | None = ...,
        *,
        ids: list[str] | None = ...,
        id: str | None = ...,
        **kwargs: Any,
    ) -> Vertex:
        """Patched at import time by ironweaver/__init__.py to accept a predicate callable."""
        ...
    def prune(self) -> int: ...
    def random_walks(
        self,
        start_node_id: str,
        max_length: int,
        num_attempts: int,
        min_length: int | None = ...,
        allow_revisit: bool | None = ...,
        include_edge_types: bool | None = ...,
        edge_type_field: str | None = ...,
    ) -> list[list[str]]: ...

__all__ = ["ObservedDictionary", "Edge", "Node", "Path", "Vertex"]
