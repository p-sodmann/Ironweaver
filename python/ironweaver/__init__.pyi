"""
Public type stubs for the ironweaver package.

This file describes every symbol available after::

    from ironweaver import Vertex, Node, Edge, Path
    from ironweaver import NodeView, EdgeView
    from ironweaver import parse_lgf, parse_lgf_file
"""

from __future__ import annotations

from typing import Any, Callable, Iterator, final

# ---------------------------------------------------------------------------
# NodeView — proxy passed to Vertex.filter predicates
# ---------------------------------------------------------------------------

class NodeView:
    """Read-only proxy around a :class:`Node` used inside ``Vertex.filter`` predicates.

    Example::

        graph.filter(lambda n: (
            n.id.startswith("user_")
            and n.type == "Person"
            and n.attr("age", 0) >= 18
            and not n.has_attr("deleted")
        ))
    """

    def __init__(self, node: Node) -> None: ...

    @property
    def id(self) -> str:
        """The node's unique identifier."""
        ...
    @property
    def type(self) -> str | None:
        """Shortcut for ``node.attr.get("type")``. Returns None if not set."""
        ...
    @property
    def edges(self) -> list[Edge]:
        """Outgoing edges from this node."""
        ...
    @property
    def inverse_edges(self) -> list[Edge]:
        """Incoming edges to this node."""
        ...
    @property
    def meta(self) -> dict[str, Any]:
        """Node-level metadata dict."""
        ...
    @property
    def attrs(self) -> dict[str, Any]:
        """The full attribute dictionary (same object as ``node.attr``)."""
        ...
    @property
    def node(self) -> Node:
        """The underlying :class:`Node` object."""
        ...
    @property
    def neighbor_ids(self) -> set[str]:
        """Set of IDs reachable via outgoing edges."""
        ...
    @property
    def degree(self) -> int:
        """Number of outgoing edges."""
        ...
    @property
    def in_degree(self) -> int:
        """Number of incoming edges."""
        ...

    def attr(self, key: str, default: Any = ...) -> Any:
        """Return the value of attribute *key*, or *default* (None) if missing."""
        ...
    def has_attr(self, key: str) -> bool:
        """Return True if attribute *key* exists on this node."""
        ...
    def has_edge_to(self, target_id: str) -> bool:
        """Return True if there is an outgoing edge to the node with *target_id*."""
        ...
    def has_edge_from(self, source_id: str) -> bool:
        """Return True if there is an incoming edge from the node with *source_id*."""
        ...

# ---------------------------------------------------------------------------
# EdgeView — proxy passed to Node traversal edge-filter callables
# ---------------------------------------------------------------------------

class EdgeView:
    """Read-only proxy around an :class:`Edge` used in traversal filter callables.

    Example::

        node.traverse(depth=3, filter=lambda e: e.type == "knows")
        node.bfs(filter=lambda e: e.attr("weight", 0) > 0.5)
    """

    def __init__(self, edge: Edge) -> None: ...

    @property
    def type(self) -> str | None:
        """Shortcut for ``edge.attr.get("type")``. Returns None if not set."""
        ...
    @property
    def from_node(self) -> Node:
        """The source node of this edge."""
        ...
    @property
    def to_node(self) -> Node:
        """The target node of this edge."""
        ...
    @property
    def attrs(self) -> dict[str, Any]:
        """The full attribute dictionary (same object as ``edge.attr``)."""
        ...
    @property
    def edge(self) -> Edge:
        """The underlying :class:`Edge` object."""
        ...
    @property
    def id(self) -> str | None:
        """The edge's optional identifier."""
        ...

    def attr(self, key: str, default: Any = ...) -> Any:
        """Return the value of attribute *key*, or *default* (None) if missing."""
        ...
    def has_attr(self, key: str) -> bool:
        """Return True if attribute *key* exists on this edge."""
        ...

# ---------------------------------------------------------------------------
# ObservedDictionary  (PyO3 extension class — cannot be subclassed)
# ---------------------------------------------------------------------------

@final
class ObservedDictionary:
    """A dict-like container that fires per-key callbacks when values change."""

    def __new__(
        cls,
        node: Any | None,
        callbacks: dict[str, list[Callable[..., Any]]] | None,
    ) -> ObservedDictionary: ...
    def __getitem__(self, key: str, /) -> Any: ...
    def __setitem__(self, key: str, value: Any, /) -> None: ...

# ---------------------------------------------------------------------------
# Edge  (PyO3 extension class — cannot be subclassed)
# ---------------------------------------------------------------------------

@final
class Edge:
    """A directed, attributed edge between two nodes.

    Attributes are stored in ``attr``. Use ``attr_set`` / ``attr_get`` instead
    of direct dict access when you need update callbacks to fire.

    Callback signature for on_update_callbacks::

        def cb(vertex, edge, key, new_value, old_value) -> bool: ...
    """

    id: str | None
    """Optional edge identifier."""
    from_node: Node
    """Source node."""
    to_node: Node
    """Target node."""
    attr: dict[str, Any]
    """Edge attributes, e.g. {"type": "knows", "since": 2020}."""
    watched_by: list[Any]
    meta: dict[str, Any]
    on_meta_change_callbacks: list[Callable[..., Any]]
    on_update_callbacks: list[Callable[[Vertex | None, Edge, str, Any, Any | None], bool]]
    """Fires when attr_set changes a value. Shared with Vertex.on_edge_update_callbacks."""
    vertex: Vertex | None
    """Back-reference to the owning Vertex (set automatically by add_edge)."""

    def __new__(
        cls,
        from_node: Node,
        to_node: Node,
        attr: dict[str, Any] | None,
        id: str | None,
    ) -> Edge: ...
    def __repr__(self) -> str: ...
    def toJSON(self) -> dict[str, Any]:
        """Return the attr dict as a plain Python dict."""
        ...
    def attr_set(self, key: str, value: Any) -> None:
        """Set attr[key] = value and fire on_update_callbacks if the value changed."""
        ...
    def attr_get(self, key: str) -> Any | None:
        """Return attr[key], or None if the key does not exist."""
        ...

# ---------------------------------------------------------------------------
# Node  (PyO3 extension class — cannot be subclassed)
# ---------------------------------------------------------------------------

@final
class Node:
    """A graph node with a string ID, an attribute dict, and directed edge lists.

    Callback signature for on_update_callbacks::

        def cb(vertex, node, key, new_value, old_value) -> bool: ...

    Use ``attr_set`` / ``attr_get`` when you need update callbacks to fire.
    Direct assignment to ``node.attr["key"] = value`` bypasses callbacks.
    """

    id: str
    """Unique node identifier."""
    attr: dict[str, Any]
    """Node attributes, e.g. {"type": "Person", "age": 30}."""
    edges: list[Edge]
    """Outgoing edges."""
    inverse_edges: list[Edge]
    """Incoming edges."""
    meta: dict[str, Any]
    on_edge_add_callbacks: list[Callable[..., Any]]
    on_update_callbacks: list[Callable[[Vertex | None, Node, str, Any, Any | None], bool]]
    """Fires when attr_set changes a value. Shared with Vertex.on_node_update_callbacks."""
    vertex: Vertex | None
    """Back-reference to the owning Vertex (set automatically by add_node)."""

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
        filter: dict[str, Any] | Callable[[EdgeView], bool] | None = ...,
        edge_filter: Callable[[EdgeView], bool] | None = ...,
    ) -> Vertex:
        """DFS traversal from this node.

        Parameters
        ----------
        depth:
            Maximum traversal depth. None means unlimited.
        filter:
            If a dict, only edges whose ``attr`` contains every key/value pair
            are followed (e.g. ``{"type": "broader"}``).
            If a callable, it receives an :class:`EdgeView` and must return True
            for edges that should be followed. Cannot be combined with edge_filter.
        edge_filter:
            Explicit callable edge filter (same semantics as a callable *filter*).

        Returns a :class:`Vertex` whose ``meta["nodelist"]`` contains node IDs
        in DFS visit order.
        """
        ...
    def bfs(
        self,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[EdgeView], bool] | None = ...,
        edge_filter: Callable[[EdgeView], bool] | None = ...,
    ) -> Vertex:
        """BFS traversal from this node.

        Same parameters as :meth:`traverse`. Returns a :class:`Vertex` whose
        ``meta["nodelist"]`` contains node IDs in BFS discovery order.
        """
        ...
    def bfs_search(
        self,
        target_id: str,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[EdgeView], bool] | None = ...,
        edge_filter: Callable[[EdgeView], bool] | None = ...,
    ) -> Node | None:
        """Search for *target_id* using BFS. Returns the Node if found, None otherwise."""
        ...
    def attr_get(self, key: str) -> Any | None:
        """Return attr[key], or None if the key does not exist."""
        ...
    def attr_set(self, key: str, value: Any) -> None:
        """Set attr[key] = value and fire on_update_callbacks if the value changed."""
        ...
    def attr_list_append(self, key: str, value: Any) -> None:
        """Append *value* to the list stored at attr[key], creating it if missing."""
        ...

# ---------------------------------------------------------------------------
# Path  (PyO3 extension class — cannot be subclassed)
# ---------------------------------------------------------------------------

@final
class Path:
    """An ordered sequence of nodes.

    .. note::
        No current public API method returns a ``Path`` object directly.
        ``shortest_path_bfs`` and the traversal methods return a
        :class:`Vertex` subgraph; use ``result.meta["nodelist"]`` for the
        ordered node-ID list. ``Path`` is reserved for future use.
    """

    nodes: list[Node]

    def __new__(cls, nodes: list[Node] | None) -> Path: ...
    def __repr__(self) -> str: ...
    def toJSON(self) -> list[str]:
        """Return the list of node IDs along this path."""
        ...

# ---------------------------------------------------------------------------
# Vertex — main graph class  (PyO3 extension class — cannot be subclassed)
# ---------------------------------------------------------------------------

@final
class Vertex:
    """A directed property graph with Rust-powered performance.

    Quick start::

        g = Vertex()
        g.add_node("alice", {"type": "Person", "age": 30})
        g.add_node("bob",   {"type": "Person", "age": 25})
        g.add_edge("alice", "bob", {"type": "knows", "since": 2020})

        # Iterate nodes
        for node in g:
            print(node.id, node.attr)

        # Filter to a subgraph
        people = g.filter(lambda n: n.type == "Person")

        # Traverse from a node
        subgraph = g["alice"].bfs(depth=2)

    Callback conventions
    --------------------
    on_node_add_callbacks   – ``(vertex: Vertex, node: Node) -> bool``
    on_edge_add_callbacks   – ``(vertex: Vertex, edge: Edge) -> bool``
    on_node_update_callbacks – ``(vertex, node, key, new_val, old_val) -> bool``
    on_edge_update_callbacks – ``(vertex, edge, key, new_val, old_val) -> bool``

    Return ``False`` from any callback to stop further callbacks in that chain.
    The node or edge is **always added** regardless of the return value —
    returning ``False`` only prevents subsequent callbacks from running.
    """

    nodes: dict[str, Node]
    """Maps node ID → Node for all nodes in the graph."""
    meta: dict[str, Any]
    """Arbitrary graph-level metadata. Traversal methods may populate meta["nodelist"]."""
    on_node_add_callbacks: list[Callable[[Vertex, Node], bool]]
    on_edge_add_callbacks: list[Callable[[Vertex, Edge], bool]]
    on_node_update_callbacks: list[Callable[[Vertex | None, Node, str, Any, Any | None], bool]]
    on_edge_update_callbacks: list[Callable[[Vertex | None, Edge, str, Any, Any | None], bool]]

    def __new__(cls) -> Vertex: ...
    def __getitem__(self, key: str, /) -> Node:
        """Return the node with the given ID. Raises KeyError if not found."""
        ...
    def __iter__(self) -> Iterator[Node]:
        """Iterate over all nodes (values) in the graph."""
        ...
    def __len__(self) -> int:
        """Return the number of nodes."""
        ...
    def __repr__(self) -> str: ...
    def keys(self) -> list[str]:
        """Return all node IDs."""
        ...
    def toJSON(self) -> dict[str, Any]: ...

    # ------------------------------------------------------------------
    # Existence / introspection
    # ------------------------------------------------------------------

    def has_node(self, id: str) -> bool: ...
    def node_count(self) -> int: ...
    def get_metadata(self) -> dict[str, Any]:
        """Return summary metadata about the graph.

        Returned keys:

        ==================  =================================================
        ``node_count``      Number of nodes (int)
        ``edge_count``      Number of edges (int)
        ``average_degree``  Mean number of outgoing edges per node (float)
        ``node_ids``        List of all node ID strings
        ==================  =================================================
        """
        ...

    # ------------------------------------------------------------------
    # Mutation
    # ------------------------------------------------------------------

    def add_node(self, id: str, attr: dict[str, Any] | None) -> Node:
        """Add a node and return it. Raises ValueError if *id* already exists."""
        ...
    def add_edge(self, from_id: str, to_id: str, attr: dict[str, Any] | None) -> Edge:
        """Add a directed edge and return it. Raises ValueError if either node is missing."""
        ...
    def get_node(self, id: str) -> Node:
        """Return the node. Raises KeyError if not found."""
        ...
    def prune(self) -> int:
        """Remove dangling edges (edges pointing to nodes not in this vertex).

        Returns the number of edges removed. Useful after filtering or subsetting.
        """
        ...

    # ------------------------------------------------------------------
    # Persistence
    # ------------------------------------------------------------------

    def save_to_json(self, file_path: str | None = ...) -> str | None:
        """Serialize to JSON.

        If *file_path* is given, writes to that path and returns None.
        If *file_path* is None, returns the JSON string.
        """
        ...
    def save_to_binary(self, file_path: str) -> None:
        """Serialize to a compact binary format."""
        ...
    def save_to_binary_f16(self, file_path: str) -> None:
        """Like save_to_binary but stores floats as f16 to reduce file size."""
        ...
    @staticmethod
    def load_from_json(source: str | dict[str, Any]) -> Vertex:
        """Load from a file path, a raw JSON string, or a plain dict.

        Example::

            loaded = Vertex.load_from_json("my_graph.json")   # file path
            loaded = Vertex.load_from_json(json_string)        # raw JSON string
            loaded = Vertex.load_from_json({"nodes": {...}})   # plain dict
        """
        ...
    @staticmethod
    def load_from_binary(file_path: str) -> Vertex: ...
    @staticmethod
    def from_nodes(nodes: dict[str, Node]) -> Vertex:
        """Construct a Vertex directly from an existing node mapping."""
        ...
    @staticmethod
    def from_nodes_with_path(nodes: dict[str, Node], nodelist: list[str]) -> Vertex:
        """Like from_nodes but also stores *nodelist* in ``meta["nodelist"]``."""
        ...

    # ------------------------------------------------------------------
    # Conversion
    # ------------------------------------------------------------------

    def to_networkx(self) -> Any:
        """Convert to a ``networkx.DiGraph``. Requires networkx to be installed."""
        ...

    # ------------------------------------------------------------------
    # Algorithms
    # ------------------------------------------------------------------

    def shortest_path_bfs(
        self,
        root_node_id: str,
        target_node_id: str,
        max_depth: int | None = ...,
    ) -> Vertex:
        """Return a new Vertex containing only the nodes on the shortest BFS path.

        The ordered sequence of node IDs is in ``result.meta["nodelist"]``.
        Raises ValueError if either node is missing or the target is unreachable.
        """
        ...
    def expand(self, source_vertex: Vertex, depth: int | None = ...) -> Vertex:
        """Expand this subgraph by pulling neighbour nodes from *source_vertex*.

        *depth* defaults to 1 (one hop).

        Only **outgoing** edges are followed during expansion; nodes that point
        *into* the seed nodes are not included.

        Example::

            seed = graph.filter(id="ckd")
            expanded = seed.expand(graph, depth=1)
            # expanded now contains ckd + all nodes ckd has outgoing edges to
        """
        ...
    def filter(
        self,
        predicate: Callable[[NodeView], bool] | None = ...,
        *,
        ids: list[str] | None = ...,
        id: str | None = ...,
        **kwargs: Any,
    ) -> Vertex:
        """Return a new Vertex containing only matching nodes and their shared edges.

        Exactly one filtering mode must be used:

        **Predicate (lambda) mode** — most expressive::

            result = g.filter(lambda n: (
                n.id.startswith("user_")
                and n.type == "Person"
                and n.attr("age", 0) >= 18
                and not n.has_attr("deleted")
                and n.has_edge_to("org_1")
            ))

        **ID list mode**::

            result = g.filter(ids=["alice", "bob"])
            result = g.filter(id="alice")

        **Attribute equality mode** (keyword arguments)::

            result = g.filter(type="Person")
            result = g.filter(status="active", role="admin")  # multiple kwargs are ANDed

        The predicate receives a :class:`NodeView` which exposes:

        ==================  =====================================================
        ``n.id``            node ID (str)
        ``n.type``          shortcut for ``n.attr("type")``
        ``n.attr(key)``     attribute value, or None
        ``n.attr(key, d)``  attribute value with default *d*
        ``n.has_attr(key)`` True if key present
        ``n.attrs``         full attribute dict
        ``n.edges``         list of outgoing :class:`Edge` objects
        ``n.inverse_edges`` list of incoming :class:`Edge` objects
        ``n.degree``        number of outgoing edges
        ``n.in_degree``     number of incoming edges
        ``n.has_edge_to(id)``   True if outgoing edge to *id* exists
        ``n.has_edge_from(id)`` True if incoming edge from *id* exists
        ``n.neighbor_ids``  set of outgoing neighbour IDs
        ``n.node``          the underlying :class:`Node` object
        ==================  =====================================================

        Raises :exc:`ValueError` if called with no arguments — exactly one
        filtering mode must be used.
        """
        ...
    def random_walks(
        self,
        start_node_id: str,
        max_length: int,
        num_attempts: int,
        min_length: int | None,
        allow_revisit: bool | None,
        include_edge_types: bool | None,
        edge_type_field: str | None,
    ) -> list[list[str]]:
        """Perform random walks from *start_node_id*.

        Parameters
        ----------
        start_node_id:
            ID of the starting node.
        max_length:
            Maximum number of hops per walk.
        num_attempts:
            Number of walk attempts. Duplicate walks are removed automatically.
        min_length:
            Minimum walk length to include. Pass None to disable.
        allow_revisit:
            Allow visiting the same node twice. Pass None to use default (False).
        include_edge_types:
            If True, each walk alternates between node IDs and edge-type strings,
            e.g. ["alice", "knows", "bob"]. Pass None to use default (False).
        edge_type_field:
            Attribute key used to read the edge type. Pass None to use "type".

        Returns a list of walks; each walk is a list of strings.

        .. important::
            All seven arguments are **positional and required**. Pass ``None``
            for optional parameters to use their defaults::

                walks = graph.random_walks("node1", 5, 20, 2, None, True, None)
        """
        ...

# ---------------------------------------------------------------------------
# LGF parsing functions
# ---------------------------------------------------------------------------

def parse_lgf(
    text: str,
    graph: Vertex | None = ...,
    base_path: str | None = ...,
) -> Vertex:
    """Parse LGF (Labeled Graph Format) text into a :class:`Vertex`.

    LGF syntax::

        alice Person
          name = "Alice"
          age = 30
          -knows-> bob
            since = 2020
          -works_at-> corp_1

        bob Person
          name = "Bob"
          age = 25

        corp_1 Company
          name = "Tech Corp"
          <-founded_by- alice

    Parameters
    ----------
    text:
        LGF-formatted string.
    graph:
        Existing graph to add nodes and edges to. A new graph is created if None.
    base_path:
        Base directory used to resolve ``import(...)`` statements.
    """
    ...

def parse_lgf_file(
    path: str,
    graph: Vertex | None = ...,
) -> Vertex:
    """Parse an LGF file from *path* into a :class:`Vertex`."""
    ...

# ---------------------------------------------------------------------------
# Re-exports
# ---------------------------------------------------------------------------

__all__ = [
    "Vertex",
    "Node",
    "NodeView",
    "EdgeView",
    "Edge",
    "Path",
    "ObservedDictionary",
    "parse_lgf",
    "parse_lgf_file",
]
