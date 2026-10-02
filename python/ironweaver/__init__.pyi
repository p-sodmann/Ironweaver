"""
Public type stubs for the ironweaver package.

This file describes every symbol available after::

    from ironweaver import Vertex, Node, Edge, Path
    from ironweaver import NodeView, EdgeView
    from ironweaver import parse_lgf, parse_lgf_file
"""

from __future__ import annotations

from typing import Any, Callable, Iterable, Iterator, Literal, Sequence, final, overload

from typing_extensions import deprecated

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
    def labels(self) -> list[str]:
        """The node's labels."""
        ...
    def has_label(self, label: str) -> bool: ...
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
        """The edge's type (``edge.type``), or None."""
        ...
    @property
    def id(self) -> int:
        """The edge's persistent id."""
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

    An ``Edge`` is a handle to an edge stored in its ``vertex`` (the graph
    data lives in Rust): two handles to the same edge compare equal (``==``)
    but need not be the same object. Edges are created with
    ``Vertex.add_edge``; ``Edge(...)`` raises TypeError. Using a handle to a
    removed edge raises RuntimeError.

    Attributes are stored in ``attr``. Use ``attr_set`` / ``attr_get`` instead
    of direct dict access when you need update callbacks to fire.

    Callback signature for on_update_callbacks::

        def cb(vertex, edge, key, new_value, old_value) -> bool: ...
    """

    @property
    def id(self) -> int:
        """Persistent id: unique in its graph, never reused, kept by save/load
        and derived graphs (``Vertex.get_edge(id)``)."""
        ...
    type: str | None
    """The edge's type (settable; None clears it)."""
    attr: dict[str, Any]
    """Edge attributes (a copy), e.g. {"since": 2020, "type": "knows"}. The type
    shows up under "type"; assigning a dict with a str "type" sets it."""
    watched_by: list[Any]
    meta: dict[str, Any]
    on_meta_change_callbacks: list[Callable[..., Any]]
    @property
    def from_node(self) -> Node:
        """Source node."""
        ...
    @property
    def to_node(self) -> Node:
        """Target node."""
        ...
    @property
    def on_update_callbacks(self) -> list[Callable[[Vertex, Edge, str, Any, Any | None], bool]]:
        """The owning Vertex's on_edge_update_callbacks (fired by attr_set)."""
        ...
    @property
    def vertex(self) -> Vertex:
        """The Vertex this edge belongs to."""
        ...

    def __new__(cls, *args: Any, **kwargs: Any) -> Edge:
        """Always raises TypeError: create edges with Vertex.add_edge."""
        ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool:
        """True if both handles refer to the same edge of the same Vertex."""
        ...
    def __hash__(self) -> int: ...
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

    A ``Node`` is a handle to a node stored in its ``vertex`` (the graph data
    lives in Rust): ``g["a"] == g["a"]``, but the two need not be the same
    object, so compare with ``==``. Using a handle to a removed node raises
    RuntimeError.

    Callback signature for on_update_callbacks::

        def cb(vertex, node, key, new_value, old_value) -> bool: ...

    Use ``attr_set`` / ``attr_get`` when you need update callbacks to fire.
    Direct assignment to ``node.attr["key"] = value`` bypasses callbacks.
    """

    id: str
    """Unique node identifier (assigning renames the node; ValueError if taken)."""
    labels: list[str]
    """The node's labels (a copy; assigning replaces them)."""
    attr: dict[str, Any]
    """Node attributes (a copy), e.g. {"type": "Person", "age": 30}. Labels show
    up under "labels"; assigning a dict with a list of str "labels" sets them."""
    def add_label(self, label: str) -> bool:
        """Add a label; False if the node already had it."""
        ...
    def remove_label(self, label: str) -> bool:
        """Remove a label; False if the node didn't have it."""
        ...
    def has_label(self, label: str) -> bool: ...
    meta: dict[str, Any]
    on_edge_add_callbacks: list[Callable[..., Any]]
    @property
    def edges(self) -> list[Edge]:
        """Outgoing edges (a new list)."""
        ...
    @property
    def inverse_edges(self) -> list[Edge]:
        """Incoming edges (a new list)."""
        ...
    @property
    def on_update_callbacks(self) -> list[Callable[[Vertex, Node, str, Any, Any | None], bool]]:
        """The owning Vertex's on_node_update_callbacks (fired by attr_set)."""
        ...
    @property
    def vertex(self) -> Vertex:
        """The Vertex this node belongs to."""
        ...

    def __new__(
        cls,
        id: str,
        attr: dict[str, Any] | None = ...,
        edges: list[Edge] | None = ...,
    ) -> Node:
        """A standalone node in its own new one-node Vertex (``node.vertex``).

        *edges* must be empty: edges are created with ``Vertex.add_edge``.
        """
        ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool:
        """True if both handles refer to the same node of the same Vertex."""
        ...
    def __hash__(self) -> int: ...
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

        Returns a new :class:`Vertex` with copies of the reached nodes (and
        the edges between them) whose ``meta["nodelist"]`` contains node IDs
        in DFS visit order. A callable filter must not change the graph
        (that can raise RuntimeError).
        """
        ...
    def bfs(
        self,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[EdgeView], bool] | None = ...,
        edge_filter: Callable[[EdgeView], bool] | None = ...,
    ) -> Vertex:
        """BFS traversal from this node.

        Same parameters as :meth:`traverse`. Returns a new :class:`Vertex`
        with copies of the reached nodes whose ``meta["nodelist"]`` contains
        node IDs in BFS discovery order.
        """
        ...
    def bfs_search(
        self,
        target_id: str,
        depth: int | None = ...,
        filter: dict[str, Any] | Callable[[EdgeView], bool] | None = ...,
        edge_filter: Callable[[EdgeView], bool] | None = ...,
    ) -> Node | None:
        """Search for *target_id* using BFS. Returns the Node if found, None otherwise.

        The search runs from both ends at once (bidirectional BFS), which is
        much faster on large graphs.
        *filter* / *edge_filter* are applied to every edge either side
        follows; *depth* bounds the path length.
        """
        ...
    def paths(
        self,
        min_hops: int = ...,
        max_hops: int | None = ...,
        *,
        direction: Literal["out", "in", "both"] | None = ...,
        types: str | list[str] | None = ...,
        where: Expr | None = ...,
        uniqueness: Literal["trail", "path", "walk"] = ...,
        limit: int | None = ...,
    ) -> list[Path]:
        """Every path from this node with *min_hops*..*max_hops* edges, depth
        first. *uniqueness*: "trail" (no edge twice; default), "path" (no
        node twice) or "walk" (needs *max_hops*). *types* / *where* restrict
        the edges followed; *limit* caps the number of paths.
        """
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
    """A path: its nodes in order and the edges between them.

    Returned by :meth:`Node.paths`. ``len(path)`` is the number of edges.
    """

    nodes: list[Node]
    edges: list[Edge]

    def __new__(cls, nodes: list[Node] | None = ..., edges: list[Edge] | None = ...) -> Path: ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...
    def ids(self) -> list[str]:
        """The node ids in order."""
        ...
    def toJSON(self) -> list[str]:
        """Return the list of node IDs along this path."""
        ...

# ---------------------------------------------------------------------------
# Projection — compact read-only copy for analytics  (PyO3 extension class)
# ---------------------------------------------------------------------------

@final
class Projection:
    """A compact, read-only copy of (part of) a graph for analytics, made by
    :meth:`Vertex.project`.

    Holds the structure (sorted neighbour lists in both directions), at most
    one weight per edge and the node ids, but no attributes. Queries run on
    all cores with the GIL released. A snapshot: later changes to the graph
    don't affect it.
    """

    @property
    def direction(self) -> Literal["out", "in", "both"]:
        """How edges were followed: "in" reversed them, "both" made them undirected."""
        ...
    @property
    def weighted(self) -> bool: ...
    def node_count(self) -> int: ...
    def edge_count(self) -> int:
        """Number of graph edges in the projection."""
        ...
    def __len__(self) -> int: ...
    def __contains__(self, id: str, /) -> bool: ...
    def __repr__(self) -> str: ...
    def ids(self) -> list[str]:
        """Node ids, in the projection's order."""
        ...
    def neighbors(self, id: str, direction: Literal["out", "in"] | None = ...) -> list[str]:
        """Ids one edge away, sorted by projection order; parallel edges repeat."""
        ...
    def degree(self, id: str, direction: Literal["out", "in"] | None = ...) -> int: ...
    def memory_usage(self) -> int:
        """Approximate memory used, in bytes."""
        ...
    def shortest_paths(
        self,
        pairs: list[tuple[str, str]],
        method: Literal["bfs", "dijkstra"] | None = ...,
        *,
        max_cost: float | None = ...,
    ) -> list[dict[str, Any] | None]:
        """Like :meth:`Vertex.shortest_paths`; ``method=None`` picks "dijkstra"
        on a weighted projection, "bfs" otherwise."""
        ...
    def distances(
        self,
        sources: list[str],
        targets: list[str] | None = ...,
        method: Literal["bfs", "dijkstra"] | None = ...,
        *,
        max_cost: float | None = ...,
    ) -> dict[str, dict[str, float]]:
        """Like :meth:`Vertex.distances`, on this projection."""
        ...
    def weakly_connected_components(self) -> list[list[str]]:
        """Components ignoring edge direction: lists of ids, largest first."""
        ...
    def strongly_connected_components(self) -> list[list[str]]:
        """Strongly connected components along the edges: lists of ids, largest first."""
        ...
    def topological_sort(self) -> list[str]:
        """Ids with every edge pointing forwards (ties in projection order).

        Raises ValueError if the projection has a cycle.
        """
        ...
    def find_cycle(self) -> list[str] | None:
        """One cycle (the last id has an edge back to the first), or None."""
        ...
    def degree_centrality(self, direction: Literal["out", "in"] | None = ...) -> dict[str, float]:
        """``{id: degree / (n - 1)}``, one per edge."""
        ...
    def pagerank(
        self,
        alpha: float = ...,
        *,
        personalization: dict[str, float] | None = ...,
        max_iter: int = ...,
        tol: float = ...,
    ) -> dict[str, float]:
        """``{id: rank}``, like ``networkx.pagerank``; uses the projection's weights.

        *personalization* biases the random jumps (personalized PageRank).
        Raises ValueError for invalid options or if it does not converge;
        ``tol=0`` runs exactly *max_iter* iterations instead.
        """
        ...
    def triangles(self) -> dict[str, int]:
        """``{id: triangles through the node}`` (edges as undirected)."""
        ...
    def clustering(self, *, directed: bool = ...) -> dict[str, float]:
        """``{id: local clustering coefficient}``: edges as undirected (like
        networkx), or with ``directed=True`` the LDBC Graphalytics definition."""
        ...
    def core_number(self) -> dict[str, int]:
        """``{id: k-core number}`` (edges as undirected)."""
        ...
    def label_propagation(self, max_iter: int = ...) -> list[list[str]]:
        """Communities by synchronous label propagation (the LDBC CDLP rule;
        deterministic), largest first."""
        ...
    def bfs_levels(self, sources: list[str], max_depth: int | None = ...) -> dict[str, int]:
        """``{id: hops from the nearest source}`` for every node reached."""
        ...
    def betweenness_centrality(
        self,
        k: int | None = ...,
        *,
        normalized: bool = ...,
        endpoints: bool = ...,
        weighted: bool | None = ...,
        seed: int | None = ...,
    ) -> dict[str, float]:
        """Betweenness centrality (Brandes; `k` samples sources), like networkx."""
        ...
    def closeness_centrality(self, *, wf_improved: bool = ..., weighted: bool | None = ...) -> dict[str, float]:
        """Closeness centrality from the distances *to* each node, like networkx."""
        ...
    def harmonic_centrality(self, *, weighted: bool | None = ...) -> dict[str, float]:
        """Sum of ``1 / distance`` from every node that reaches each node."""
        ...
    def similarity(
        self,
        pairs: list[tuple[str, str]],
        metric: Literal[
            "jaccard", "overlap", "common_neighbors", "adamic_adar", "resource_allocation", "preferential_attachment"
        ] = ...,
    ) -> list[float]:
        """Neighbourhood similarity of each pair (edges as undirected)."""
        ...
    @overload
    def most_similar(
        self,
        ids: str,
        k: int = ...,
        *,
        metric: Literal["jaccard", "overlap", "common_neighbors", "adamic_adar", "resource_allocation"] = ...,
        min_score: float = ...,
    ) -> list[tuple[str, float]]: ...
    @overload
    def most_similar(
        self,
        ids: list[str] | None = ...,
        k: int = ...,
        *,
        metric: Literal["jaccard", "overlap", "common_neighbors", "adamic_adar", "resource_allocation"] = ...,
        min_score: float = ...,
    ) -> dict[str, list[tuple[str, float]]]:
        """The ``k`` most similar nodes of each node, best first."""
        ...
    def leiden(
        self,
        resolution: float = ...,
        *,
        randomness: float = ...,
        max_iter: int = ...,
        seed: int | None = ...,
    ) -> list[list[str]]:
        """Communities by the Leiden algorithm (connected, maximising modularity), largest first."""
        ...
    def modularity(self, communities: Iterable[Iterable[str]], resolution: float = ...) -> float:
        """Modularity of a partition of the nodes, like networkx.community.modularity."""
        ...
    def minimum_spanning_tree(self, *, maximum: bool = ...) -> list[tuple[str, str, float]]:
        """Edges ``(id, id, weight)`` of a minimum (or maximum) spanning forest."""
        ...
    def fastrp(
        self,
        dimension: int = ...,
        *,
        iteration_weights: list[float] = ...,
        self_influence: float = ...,
        normalization_strength: float = ...,
        seed: int | None = ...,
    ) -> dict[str, list[float]]:
        """``{id: embedding}`` by FastRP (fast random projection)."""
        ...
    def node2vec_walks(
        self,
        walk_length: int = ...,
        walks_per_node: int = ...,
        *,
        p: float = ...,
        q: float = ...,
        sources: list[str] | None = ...,
        seed: int | None = ...,
    ) -> list[list[str]]:
        """node2vec (second-order biased) random walks."""
        ...
    def k_shortest_paths(
        self,
        source: str,
        target: str,
        k: int,
        method: Literal["bfs", "dijkstra"] | None = ...,
    ) -> list[dict[str, Any]]:
        """Up to ``k`` shortest loopless paths, cheapest first (Yen)."""
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

    @property
    def nodes(self) -> dict[str, Node]:
        """A new dict mapping node ID → Node for all nodes in the graph."""
        ...
    meta: dict[str, Any]
    """Arbitrary graph-level metadata. Traversal methods may populate meta["nodelist"]."""
    on_node_add_callbacks: list[Callable[[Vertex, Node], bool]]
    on_edge_add_callbacks: list[Callable[[Vertex, Edge], bool]]
    on_node_update_callbacks: list[Callable[[Vertex, Node, str, Any, Any | None], bool]]
    on_edge_update_callbacks: list[Callable[[Vertex, Edge, str, Any, Any | None], bool]]

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
    def __contains__(self, key: str | Node, /) -> bool:
        """Return True if a node ID (str) or Node is in the graph.

        Example::

            if "alice" in graph: ...
        """
        ...
    def __repr__(self) -> str: ...
    def keys(self) -> list[str]:
        """Return all node IDs (graph order: insertion order until nodes are removed)."""
        ...
    def toJSON(self) -> dict[str, Any]: ...

    # ------------------------------------------------------------------
    # Existence / introspection
    # ------------------------------------------------------------------

    def has_node(self, id: str) -> bool: ...
    def node_count(self) -> int: ...
    def memory_usage(self, *, deep: bool = ...) -> int:
        """Approximate bytes used by the graph structure (nodes, edges, ids,
        adjacency, labels, indexes).

        With *deep*, also the attribute / meta dicts and their values
        (``sys.getsizeof``, containers recursively, shared objects once).
        """
        ...
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

    def add_node(
        self, id: str, attr: dict[str, Any] | None = ..., labels: list[str] | None = ...
    ) -> Node:
        """Add a node and return it. Raises ValueError if *id* already exists.

        An ``attr["labels"]`` list of str becomes the node's labels.
        """
        ...
    def add_edge(
        self, from_id: str, to_id: str, attr: dict[str, Any] | None = ..., type: str | None = ...
    ) -> Edge:
        """Add a directed edge and return it. Raises ValueError if either node is missing.

        An ``attr["type"]`` str becomes the edge's type. The edge gets a new
        persistent ``id``.
        """
        ...
    def add_nodes(
        self,
        nodes: Iterable[str | tuple[str, dict[str, Any]] | list[Any]],
        *,
        labels: list[str] | None = ...,
        attrs: dict[str, Sequence[Any]] | None = ...,
    ) -> int:
        """Add many nodes in one call; each item is an id or ``(id, attrs)``.

        *labels* go on every node. *attrs* gives attributes as columns,
        ``{name: [one value per node]}`` (None: not set; a ``"labels"``
        column sets labels). Every item is checked first: on an error nothing
        is added. Returns the number of nodes added.
        """
        ...
    def add_edges(
        self,
        edges: Iterable[tuple[str, str] | tuple[str, str, dict[str, Any]] | list[Any]],
        *,
        type: str | None = ...,
        attrs: dict[str, Sequence[Any]] | None = ...,
    ) -> int:
        """Add many edges in one call; each item is ``(from_id, to_id)`` or
        ``(from_id, to_id, attrs)``.

        *type* applies to every edge. *attrs* gives attributes as columns,
        ``{name: [one value per edge]}`` (None: not set; a ``"type"`` column
        sets types) — the fastest way to load weights. Every item is checked
        first: on an error nothing is added. Returns the number of edges added.
        """
        ...
    def get_edge(self, id: int) -> Edge:
        """The edge with this persistent id. Raises KeyError if there is none."""
        ...
    def nodes_with_label(self, label: str) -> list[Node]:
        """Nodes carrying *label*, in graph order (label index)."""
        ...
    def create_index(self, name: str) -> bool:
        """Index node attribute *name* for fast :meth:`find` / :meth:`find_range`.

        ``filter(where=...)`` and :meth:`match` use indexes automatically.
        The index follows every change made through the graph's methods, and
        is saved with the graph (its definition; loading rebuilds it). Only
        scalar values are indexed (numbers, strings, bools, bytes, dates,
        datetimes). Returns False if the index already existed.
        """
        ...
    def drop_index(self, name: str) -> bool:
        """Drop the index on *name*; returns whether there was one."""
        ...
    @property
    def indexes(self) -> list[str]:
        """Names of the indexed node attributes, in creation order."""
        ...
    def index_stats(self, name: str) -> dict[str, int] | None:
        """Size of the index on *name*, or None if *name* isn't indexed. O(1).

        ``entries`` is the number of indexed nodes (those with a scalar value),
        ``distinct_keys`` the number of distinct values (1 and 1.0 are one),
        ``memory_bytes`` the approximate bytes the index uses (part of
        :meth:`memory_usage`), and ``dirty`` the number of nodes whose entries
        may be stale; the counts are exact when it is 0, as after changes
        made through the graph's methods.
        """
        ...
    def find(self, name: str, value: Any) -> list[Node]:
        """Nodes whose attribute *name* equals *value* (1 == 1.0), in graph order.

        Uses the index on *name* if there is one, else scans every node.
        """
        ...
    def find_range(
        self,
        name: str,
        low: Any = ...,
        high: Any = ...,
        *,
        inclusive: Literal["both", "left", "right", "neither"] = ...,
    ) -> list[Node]:
        """Nodes whose attribute *name* lies between *low* and *high*, in graph order.

        Either bound may be None (open). Bounds are numbers, strings, bools,
        bytes, dates or datetimes, of one kind; values of another kind are
        never in range. Uses the index on *name* if there is one.
        """
        ...
    def match(
        self,
        pattern: str,
        *,
        where: dict[str, Expr] | None = ...,
        ids: dict[str, str | list[str]] | None = ...,
        limit: int | None = ...,
    ) -> list[dict[str, Node | Edge | list[Edge]]]:
        """Every occurrence of a Cypher-like pattern, e.g.
        ``"(a:Person {name: 'Ann'})-[k:KNOWS*1..3]->(b)"``: one dict per match
        from variable name to Node, Edge, or list of Edges (variable-length).
        Each pattern edge binds a different edge; nodes may repeat.
        *where* adds Expr conditions per variable, *ids* fixes node
        variables, *limit* caps the matches. See docs/patterns.md.
        """
        ...
    def remove_node(self, id: str) -> Node:
        """Remove a node and every edge attached to it.

        Returns a detached copy of the node (same id, attr and meta, no edges)
        in a new one-node Vertex; existing handles to the removed node raise
        RuntimeError when used. Raises KeyError if the node does not exist.
        """
        ...
    def remove_edge(self, from_id: str, to_id: str, attr: dict[str, Any] | None = ...) -> int:
        """Remove edges from *from_id* to *to_id* and return how many were removed.

        If *attr* is given, only edges whose attributes match every key/value
        pair are removed, e.g. ``g.remove_edge("a", "b", {"type": "knows"})``.
        Raises ValueError if either node does not exist.
        """
        ...
    def get_node(self, id: str) -> Node:
        """Return the node. Raises KeyError if not found."""
        ...
    def prune(self) -> int:
        """Kept for compatibility; always returns 0.

        Edges always connect two nodes of their own graph (results of
        filter/traversals/from_nodes only copy the edges between their nodes),
        so there are never dangling edges to remove.
        """
        ...

    # ------------------------------------------------------------------
    # Persistence
    # ------------------------------------------------------------------

    def save_to_json(self, file_path: str | None = ..., pretty: bool = ...) -> str | None:
        """Serialize to JSON.

        If *file_path* is given, writes to that path and returns None.
        If *file_path* is None, returns the JSON string.
        Output is compact by default; pass ``pretty=True`` for indented JSON.
        Raises RuntimeError if a value cannot be serialized or the file
        cannot be written.
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
        """A new Vertex with copies of the given nodes (keyed by the dict keys)
        and of the edges between them."""
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
        """Find a shortest path with the chosen algorithm (see ``path_methods()``).

        *method*: ``"bfs"`` (fewest edges), ``"dijkstra"`` (cheapest by the
        *weight* edge attribute) or ``"astar"`` (cheapest, guided by an
        estimate of the remaining cost). ``None`` picks ``"astar"`` if
        *heuristic*, *coords* or *distances* is given, ``"dijkstra"`` if
        *weight* is given, else ``"bfs"``.

        Shared options: *weight* (default ``"weight"``), *default_weight*
        (cost of edges without it, default 1.0), *max_cost* (for ``"bfs"``: a
        limit on the number of edges), *direction*.

        Method options (``TypeError`` if the method does not take one):

        * ``bfs``: *max_depth*.
        * ``astar``: *heuristic* (``"euclidean"``, the default, or
          ``"manhattan"``) with *coords*, where node coordinates live: a list
          of attribute paths, one per dimension (default ``["x", "y"]``;
          ``"pos.lat"`` reads ``attr["pos"]["lat"]``), or one attribute holding
          a sequence (``"pos"``). Or *distances*: a ``vertex.meta`` key holding
          ``{node_id: estimate}`` or ``{node_id: {target_id: estimate}}``.
          Nodes without coordinates/estimates count as 0. Estimates must not
          overestimate the remaining cost, or the path may not be the cheapest.

        The result holds copies of the path's nodes and the edges between
        them; ``meta`` has ``nodelist`` (in order), ``cost`` (number of edges
        for ``"bfs"``), ``method`` and, for Dijkstra/A*, ``expanded`` (nodes
        settled).

        Raises ValueError for an unknown method, a missing node, an
        unreachable target, a negative weight or a target without
        coordinates; TypeError for a non-numeric weight, coordinate or
        estimate.

        Example::

            graph.shortest_path("a", "z")                        # bfs
            graph.shortest_path("a", "z", weight="distance")     # dijkstra
            graph.shortest_path("a", "z", coords="pos")          # astar
            graph.meta["est"] = {"a": 3.0, "b": 1.5}
            graph.shortest_path("a", "z", distances="est")       # astar
        """
        ...
    @staticmethod
    def path_methods() -> dict[str, str]:
        """The available ``shortest_path`` methods as ``{name: description}``."""
        ...
    def project(
        self,
        weight: str | None = ...,
        default_weight: float | None = ...,
        *,
        direction: Literal["out", "in", "both"] | None = ...,
        nodes: list[str] | None = ...,
        node_filter: dict[str, Any] | Callable[[NodeView], bool] | None = ...,
        edge_filter: dict[str, Any] | Callable[[EdgeView], bool] | None = ...,
    ) -> Projection:
        """A compact, read-only copy of (part of) the graph for analytics.

        Unweighted unless *weight* or *default_weight* is given. *direction*
        "in" reverses edges, "both" makes them undirected. *nodes* limits the
        projection to those ids; *node_filter* / *edge_filter* take a dict
        (attribute equality) or a callable receiving a NodeView / EdgeView.
        Build once, then run many queries on it.
        """
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
    ) -> list[dict[str, Any] | None]:
        """Shortest paths for many (source, target) pairs, in parallel.

        The graph and its edge costs are copied into a projection once, then
        all queries run on every core with the GIL released (use
        :meth:`project` to reuse one for several batches). Returns one
        entry per pair: ``{"nodelist": [...], "cost": ...}`` (cost is the number
        of edges for "bfs"), or None if the target is unreachable. Methods
        "bfs" and "dijkstra" only. Edge weights of the whole graph are validated
        when the projection is built. For a single query use ``shortest_path``.
        """
        ...
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
    ) -> dict[str, dict[str, float]]:
        """``{source: {node: cost}}`` for every node each source reaches within
        *max_cost* (only *targets*, if given), computed in parallel like
        ``shortest_paths``."""
        ...
    @deprecated("Use shortest_path(root, target, method=\"bfs\")")
    def shortest_path_bfs(
        self,
        root_node_id: str,
        target_node_id: str,
        max_depth: int | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> Vertex:
        """Deprecated: use ``shortest_path(..., method="bfs")``. The path with the fewest edges.

        The ordered sequence of node IDs is in ``result.meta["nodelist"]``.
        *direction* selects which edges are followed: ``"out"`` (default),
        ``"in"`` (walk edges backwards) or ``"both"`` (ignore direction).
        The search runs from both ends at once (bidirectional BFS). If several
        shortest paths exist, which one is returned is not specified.
        Raises ValueError if either node is missing or the target is unreachable.
        """
        ...
    @deprecated("Use shortest_path(root, target, method=\"dijkstra\")")
    def shortest_path_dijkstra(
        self,
        root_node_id: str,
        target_node_id: str,
        weight: str | None = ...,
        default_weight: float | None = ...,
        max_cost: float | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> Vertex:
        """Deprecated: use ``shortest_path(..., method="dijkstra")``. The cheapest path.

        Edge costs are read from the *weight* attribute (default ``"weight"``);
        edges without it cost *default_weight* (default 1.0). Paths costing
        more than *max_cost* are ignored. The ordered node IDs are in
        ``result.meta["nodelist"]`` and the total cost in ``result.meta["cost"]``.

        Raises ValueError if a node is missing, the target is unreachable, or a
        weight is negative; TypeError if a weight is not a number.

        Example::

            path = graph.shortest_path("a", "z", method="dijkstra", weight="distance")
            path.meta["nodelist"], path.meta["cost"]
        """
        ...
    def expand(
        self,
        source_vertex: Vertex,
        depth: int | None = ...,
        direction: Literal["out", "in", "both"] | None = ...,
    ) -> Vertex:
        """Expand this subgraph by pulling neighbour nodes from *source_vertex*.

        *depth* defaults to 1 (one hop).

        By default only **outgoing** edges are followed; nodes that point
        *into* the seed nodes are not included. Pass ``direction="in"`` to
        follow incoming edges instead, or ``direction="both"`` for both.

        Example::

            seed = graph.filter(id="ckd")
            expanded = seed.expand(graph, depth=1)
            # expanded now contains ckd + all nodes ckd has outgoing edges to
        """
        ...
    def filter(
        self,
        predicate: Callable[[NodeView], bool] | Expr | None = ...,
        *,
        ids: list[str] | None = ...,
        id: str | None = ...,
        where: Expr | None = ...,
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

        Raises :exc:`ValueError` if called with no arguments, or with both a
        predicate and keyword arguments — exactly one filtering mode must be
        used.
        """
        ...
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
    ) -> list[list[str]]:
        """Perform random walks from *start_node_id*.

        Parameters
        ----------
        start_node_id:
            ID of the starting node. May be None only when ``stratified=True``,
            in which case each walk's start node is sampled across the whole
            graph (favouring least-visited nodes).
        max_length:
            Maximum number of hops per walk.
        num_attempts:
            Number of walk attempts. Duplicate walks are removed automatically.
        min_length:
            Minimum walk length to include. Defaults to no minimum.
        allow_revisit:
            Allow visiting the same node twice. Defaults to False.
        include_edge_types:
            If True, each walk alternates between node IDs and edge-type strings,
            e.g. ["alice", "knows", "bob"]. Defaults to False.
        edge_type_field:
            Attribute key used to read the edge type. Defaults to "type".
        stratified:
            If True, aim for equal node visit frequencies: every choice (the
            start node when *start_node_id* is None, and each step) is weighted
            by ``1 / (1 + times_visited)``, steering walks towards the
            least-visited nodes. Visit counts persist across all attempts of
            one call. Defaults to False.
        seed:
            Seed for the random number generator. The same seed and arguments
            always produce the same walks. Defaults to a random seed.

        Walks run in native code without holding the GIL; non-stratified walks
        are spread over all CPU cores.

        Returns a list of walks; each walk is a list of strings.

        Example::

            walks = graph.random_walks("node1", 5, 20)
            walks = graph.random_walks("node1", 5, 20, include_edge_types=True)
            walks = graph.random_walks(None, 5, 50, stratified=True)
            walks = graph.random_walks("node1", 5, 20, seed=42)  # reproducible
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

    .. note::
        The node-type label (``Person``, ``Company`` …) is stored in
        ``attr["labels"]`` as a **list** (labels from repeated declarations
        are merged), not in ``attr["type"]``. Use
        ``graph.filter(attr_contains("labels", "Person"))`` or
        ``lambda n: "Person" in n.attr("labels", [])`` to filter parsed
        nodes — ``filter(type=...)`` and ``NodeView.type`` will not match.

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
# Expressions — filters evaluated in Rust
# ---------------------------------------------------------------------------

@final
class Expr:
    """A filter over a node or an edge, evaluated in Rust. Build with
    :func:`attr`, :func:`label`, :func:`edge_type`; combine with ``&``, ``|``,
    ``~`` (parenthesize comparisons: ``(attr("a") > 1) & label("L")``).
    A missing attribute makes every comparison false.
    """

    def __and__(self, other: Expr) -> Expr: ...
    def __or__(self, other: Expr) -> Expr: ...
    def __invert__(self) -> Expr: ...
    def __bool__(self) -> bool:
        """Always raises TypeError (use & | ~, not and / or / not)."""
        ...
    def __repr__(self) -> str: ...

@final
class Attr:
    """An attribute reference made by :func:`attr`; comparing it gives an :class:`Expr`."""

    def __eq__(self, other: object) -> Expr: ...  # type: ignore[override]
    def __ne__(self, other: object) -> Expr: ...  # type: ignore[override]
    def __lt__(self, other: Any) -> Expr: ...
    def __le__(self, other: Any) -> Expr: ...
    def __gt__(self, other: Any) -> Expr: ...
    def __ge__(self, other: Any) -> Expr: ...
    def is_in(self, values: list[Any]) -> Expr:
        """True if the attribute equals one of *values*."""
        ...
    def exists(self) -> Expr:
        """True if the attribute exists and is not None."""
        ...

def attr(path: str | list[str]) -> Attr:
    """An attribute of the node / edge: ``attr("age") > 30``; ``attr("pos.lat")``
    reaches into nested dicts, ``attr(["a.b"])`` for names containing dots."""
    ...

def label(name: str) -> Expr:
    """Nodes carrying this label (always False for edges)."""
    ...

def edge_type(name: str) -> Expr:
    """Edges of this type (always False for nodes)."""
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
    "Projection",
    "ObservedDictionary",
    "Expr",
    "Attr",
    "attr",
    "label",
    "edge_type",
    "parse_lgf",
    "parse_lgf_file",
]
