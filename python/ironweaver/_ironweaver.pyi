"""
Type stubs for the ironweaver Rust extension module.

These stubs mirror the PyO3 class definitions in src/. The public Python API
is re-exported through ironweaver/__init__.pyi, which includes the Python-level
wrappers (NodeView, EdgeView, the patched Vertex.filter, etc.).
"""

from __future__ import annotations

from typing import Any, Callable, Iterator

class ObservedDictionary:
    """A dict-like container that fires per-key callbacks on value changes."""

    def __init__(
        self,
        node: Any | None = ...,
        callbacks: dict[str, list[Callable[..., Any]]] | None = ...,
    ) -> None: ...
    def __setitem__(self, key: str, value: Any) -> None: ...
    def __getitem__(self, key: str) -> Any: ...

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

    def __init__(
        self,
        from_node: Node,
        to_node: Node,
        attr: dict[str, Any] | None = ...,
        id: str | None = ...,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def toJSON(self) -> dict[str, Any]: ...
    def attr_set(self, key: str, value: Any) -> None:
        """Set attr[key] = value and fire on_update_callbacks if the value changed."""
        ...
    def attr_get(self, key: str) -> Any | None:
        """Return attr[key], or None if the key does not exist."""
        ...

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

    def __init__(
        self,
        id: str,
        attr: dict[str, Any] | None = ...,
        edges: list[Edge] | None = ...,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def traverse(
        self,
        depth: int | None = ...,
        filter: dict[str, Any] | None = ...,
        edge_filter: Callable[[Edge], bool] | None = ...,
    ) -> Vertex:
        """DFS traversal from this node. Returns a Vertex; meta['nodelist'] holds visit order."""
        ...
    def bfs(
        self,
        depth: int | None = ...,
        filter: dict[str, Any] | None = ...,
        edge_filter: Callable[[Edge], bool] | None = ...,
    ) -> Vertex:
        """BFS traversal from this node. Returns a Vertex; meta['nodelist'] holds BFS order."""
        ...
    def bfs_search(
        self,
        target_id: str,
        depth: int | None = ...,
        filter: dict[str, Any] | None = ...,
        edge_filter: Callable[[Edge], bool] | None = ...,
    ) -> Node | None:
        """BFS search for target_id. Returns the Node if found, None otherwise."""
        ...
    def attr_get(self, key: str) -> Any | None:
        """Return attr[key], or None if the key does not exist."""
        ...
    def attr_set(self, key: str, value: Any) -> None:
        """Set attr[key] = value and fire on_update_callbacks if the value changed."""
        ...
    def attr_list_append(self, key: str, value: Any) -> None:
        """Append value to the list at attr[key], creating the list if necessary."""
        ...

class Path:
    """An ordered sequence of nodes representing a traversal path."""

    nodes: list[Node]

    def __init__(self, nodes: list[Node] | None = ...) -> None: ...
    def __repr__(self) -> str: ...
    def toJSON(self) -> list[str]:
        """Return the list of node IDs along this path."""
        ...

class Vertex:
    """A directed property graph backed by a Rust HashMap.

    Typical usage::

        g = Vertex()
        g.add_node("alice", {"type": "Person", "age": 30})
        g.add_node("bob",   {"type": "Person", "age": 25})
        g.add_edge("alice", "bob", {"type": "knows", "since": 2020})
    """

    nodes: dict[str, Node]
    meta: dict[str, Any]
    on_node_add_callbacks: list[Callable[[Vertex, Node], bool]]
    on_edge_add_callbacks: list[Callable[[Vertex, Edge], bool]]
    on_node_update_callbacks: list[Callable[[Vertex | None, Node, str, Any, Any | None], bool]]
    on_edge_update_callbacks: list[Callable[[Vertex | None, Edge, str, Any, Any | None], bool]]

    def __init__(self) -> None: ...
    def __getitem__(self, key: str) -> Node: ...
    def __iter__(self) -> Iterator[Node]: ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...
    def keys(self) -> list[str]:
        """Return all node IDs in the graph."""
        ...
    def toJSON(self) -> dict[str, Any]: ...
    def has_node(self, id: str) -> bool: ...
    def node_count(self) -> int: ...
    def add_node(self, id: str, attr: dict[str, Any] | None = ...) -> Node:
        """Add a node; raises ValueError if a node with id already exists."""
        ...
    def add_edge(self, from_id: str, to_id: str, attr: dict[str, Any] | None = ...) -> Edge:
        """Add a directed edge; raises ValueError if either node does not exist."""
        ...
    def get_node(self, id: str) -> Node:
        """Return the node; raises KeyError if not found."""
        ...
    def save_to_json(self, file_path: str | None = ...) -> str | None:
        """Serialize to JSON. Writes to file_path if given, otherwise returns JSON string."""
        ...
    def save_to_binary(self, file_path: str) -> None: ...
    def save_to_binary_f16(self, file_path: str) -> None:
        """Like save_to_binary but stores floats as f16 to reduce file size."""
        ...
    @staticmethod
    def load_from_json(source: str | dict[str, Any]) -> Vertex:
        """Load from a file path, JSON string, or dict."""
        ...
    @staticmethod
    def load_from_binary(file_path: str) -> Vertex: ...
    @staticmethod
    def from_nodes(nodes: dict[str, Node]) -> Vertex:
        """Construct a Vertex from an existing node dict (no callbacks wired)."""
        ...
    @staticmethod
    def from_nodes_with_path(nodes: dict[str, Node], nodelist: list[str]) -> Vertex:
        """Like from_nodes but also stores nodelist in meta['nodelist']."""
        ...
    def get_metadata(self) -> dict[str, Any]:
        """Return summary metadata: node_count, edge_count, etc."""
        ...
    def to_networkx(self) -> Any:
        """Convert to a networkx.DiGraph. Requires networkx to be installed."""
        ...
    def shortest_path_bfs(
        self,
        root_node_id: str,
        target_node_id: str,
        max_depth: int | None = ...,
    ) -> Vertex:
        """Return a new Vertex containing only the shortest BFS path nodes."""
        ...
    def expand(self, source_vertex: Vertex, depth: int | None = ...) -> Vertex:
        """Expand this subgraph by pulling neighbour nodes from source_vertex."""
        ...
    def filter(self, **kwargs: Any) -> Vertex:
        """Return a subgraph of matching nodes.

        Keyword arguments:
          ids  – list of node IDs to keep
          id   – single node ID to keep
          **   – attribute equality filters, e.g. type="Person"

        See ironweaver.__init__.pyi for the full Python-level signature which
        also accepts a predicate callable.
        """
        ...
    def prune(self) -> int:
        """Remove edges that point to nodes absent from this vertex. Returns count removed."""
        ...
    def random_walks(
        self,
        start_node_id: str,
        max_length: int,
        num_attempts: int,
        min_length: int | None = ...,
        allow_revisit: bool | None = ...,
        include_edge_types: bool | None = ...,
        edge_type_field: str | None = ...,
    ) -> list[list[str]]:
        """Run random walks. Returns list of walks; each walk is a list of node IDs
        (or alternating node IDs / edge types when include_edge_types=True)."""
        ...
