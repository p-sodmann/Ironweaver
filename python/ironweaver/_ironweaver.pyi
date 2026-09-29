"""
Type stubs for the ironweaver Rust extension module (_ironweaver.so).

These stubs mirror the exact PyO3-generated signatures. All six classes are
@final (PyO3 extension types cannot be subclassed). Constructors use __new__
because that is the slot PyO3 populates; at runtime __init__ takes no args.

Note: Vertex.filter, Vertex.project, Node.traverse, Node.bfs, and Node.bfs_search reflect the
Python-level wrappers applied in ironweaver/__init__.py at import time.
"""

from __future__ import annotations

from typing import Any, Callable, Iterator, Literal, final

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
    """A directed, attributed edge between two nodes (a handle into its vertex)."""

    id: str | None
    attr: dict[str, Any]
    watched_by: list[Any]
    meta: dict[str, Any]
    on_meta_change_callbacks: list[Callable[..., Any]]
    @property
    def from_node(self) -> Node: ...
    @property
    def to_node(self) -> Node: ...
    @property
    def on_update_callbacks(self) -> list[Callable[[Vertex, Edge, str, Any, Any | None], bool]]: ...
    @property
    def vertex(self) -> Vertex: ...

    def __new__(cls, *args: Any, **kwargs: Any) -> Edge:
        """Always raises TypeError: create edges with Vertex.add_edge."""
        ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
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
    meta: dict[str, Any]
    on_edge_add_callbacks: list[Callable[..., Any]]
    @property
    def edges(self) -> list[Edge]: ...
    @property
    def inverse_edges(self) -> list[Edge]: ...
    @property
    def on_update_callbacks(self) -> list[Callable[[Vertex, Node, str, Any, Any | None], bool]]: ...
    @property
    def vertex(self) -> Vertex: ...

    def __new__(
        cls,
        id: str,
        attr: dict[str, Any] | None = ...,
        edges: list[Edge] | None = ...,
    ) -> Node:
        """A node in its own new one-node Vertex; *edges* must be empty."""
        ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
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
class Projection:
    """Compact read-only copy of a graph for analytics (see Vertex.project)."""

    @property
    def direction(self) -> Literal["out", "in", "both"]: ...
    @property
    def weighted(self) -> bool: ...
    def node_count(self) -> int: ...
    def edge_count(self) -> int: ...
    def __len__(self) -> int: ...
    def __contains__(self, id: str, /) -> bool: ...
    def __repr__(self) -> str: ...
    def ids(self) -> list[str]: ...
    def neighbors(self, id: str, direction: Literal["out", "in"] | None = ...) -> list[str]: ...
    def degree(self, id: str, direction: Literal["out", "in"] | None = ...) -> int: ...
    def memory_usage(self) -> int: ...
    def shortest_paths(
        self,
        pairs: list[tuple[str, str]],
        method: Literal["bfs", "dijkstra"] | None = ...,
        *,
        max_cost: float | None = ...,
    ) -> list[dict[str, Any] | None]: ...
    def distances(
        self,
        sources: list[str],
        targets: list[str] | None = ...,
        method: Literal["bfs", "dijkstra"] | None = ...,
        *,
        max_cost: float | None = ...,
    ) -> dict[str, dict[str, float]]: ...
    def weakly_connected_components(self) -> list[list[str]]: ...
    def strongly_connected_components(self) -> list[list[str]]: ...
    def topological_sort(self) -> list[str]: ...
    def find_cycle(self) -> list[str] | None: ...
    def degree_centrality(self, direction: Literal["out", "in"] | None = ...) -> dict[str, float]: ...
    def pagerank(
        self,
        alpha: float = ...,
        *,
        personalization: dict[str, float] | None = ...,
        max_iter: int = ...,
        tol: float = ...,
    ) -> dict[str, float]: ...
    def triangles(self) -> dict[str, int]: ...
    def clustering(self) -> dict[str, float]: ...
    def core_number(self) -> dict[str, int]: ...
    def label_propagation(self, max_iter: int = ...) -> list[list[str]]: ...
    def bfs_levels(self, sources: list[str], max_depth: int | None = ...) -> dict[str, int]: ...

@final
class Vertex:
    """A directed property graph backed by ironweaver_core::Graph (pure Rust)."""

    meta: dict[str, Any]
    on_node_add_callbacks: list[Callable[[Vertex, Node], bool]]
    on_edge_add_callbacks: list[Callable[[Vertex, Edge], bool]]
    on_node_update_callbacks: list[Callable[[Vertex, Node, str, Any, Any | None], bool]]
    on_edge_update_callbacks: list[Callable[[Vertex, Edge, str, Any, Any | None], bool]]

    @property
    def nodes(self) -> dict[str, Node]: ...
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
    def add_node(self, id: str, attr: dict[str, Any] | None = ...) -> Node: ...
    def add_edge(self, from_id: str, to_id: str, attr: dict[str, Any] | None = ...) -> Edge: ...
    def remove_node(self, id: str) -> Node: ...
    def remove_edge(self, from_id: str, to_id: str, attr: dict[str, Any] | None = ...) -> int: ...
    def get_node(self, id: str) -> Node: ...
    def save_to_json(self, file_path: str | None = ..., pretty: bool = ...) -> str | None: ...
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
    def shortest_path(
        self,
        source: str,
        target: str,
        method: Literal["bfs", "dijkstra", "astar"] | None = ...,
        *,
        weight: str | None = ...,
        default_weight: float | None = ...,
        max_cost: float | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
        max_depth: int | None = ...,
        heuristic: Literal["euclidean", "manhattan"] | None = ...,
        coords: str | list[str] | None = ...,
        distances: str | None = ...,
    ) -> Vertex:
        """Shortest path with method "bfs", "dijkstra" or "astar"; see ``path_methods()``."""
        ...
    @staticmethod
    def path_methods() -> dict[str, str]: ...
    def project(
        self,
        weight: str | None = ...,
        default_weight: float | None = ...,
        *,
        direction: Literal["out", "in", "both"] | None = ...,
        nodes: list[str] | None = ...,
        node_filter: dict[str, Any] | Callable[[Any], bool] | None = ...,
        edge_filter: dict[str, Any] | Callable[[Any], bool] | None = ...,
    ) -> Projection:
        """Patched in ironweaver/__init__.py: callables receive NodeView / EdgeView."""
        ...
    def shortest_paths(
        self,
        pairs: list[tuple[str, str]],
        method: Literal["bfs", "dijkstra"] | None = ...,
        *,
        weight: str | None = ...,
        default_weight: float | None = ...,
        max_cost: float | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> list[dict[str, Any] | None]: ...
    def distances(
        self,
        sources: list[str],
        targets: list[str] | None = ...,
        method: Literal["bfs", "dijkstra"] | None = ...,
        *,
        weight: str | None = ...,
        default_weight: float | None = ...,
        max_cost: float | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> dict[str, dict[str, float]]: ...
    def shortest_path_bfs(
        self,
        root_node_id: str,
        target_node_id: str,
        max_depth: int | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> Vertex:
        """Ordered path is in ``result.meta["nodelist"]``. Raises ValueError if unreachable."""
        ...
    def shortest_path_dijkstra(
        self,
        root_node_id: str,
        target_node_id: str,
        weight: str | None = ...,
        default_weight: float | None = ...,
        max_cost: float | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> Vertex:
        """Path in ``result.meta["nodelist"]``, total cost in ``result.meta["cost"]``."""
        ...
    def expand(
        self,
        source_vertex: Vertex,
        depth: int | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> Vertex: ...
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
        start_node_id: str | None,
        max_length: int,
        num_attempts: int,
        min_length: int | None = ...,
        allow_revisit: bool | None = ...,
        include_edge_types: bool | None = ...,
        edge_type_field: str | None = ...,
        stratified: bool | None = ...,
        seed: int | None = ...,
    ) -> list[list[str]]: ...

__all__ = ["ObservedDictionary", "Edge", "Node", "Path", "Projection", "Vertex"]
