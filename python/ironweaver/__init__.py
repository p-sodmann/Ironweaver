# Import the Rust extension module classes
from typing import Callable, Iterable

from ._ironweaver import Vertex, Node, Edge, Path, ObservedDictionary

# Import the Python LGF parser
from .lgf_parser import parse_lgf, parse_lgf_file


class NodeView:
    """Lightweight proxy around :class:`Node` for use in filter lambdas.

    Provides a clean, expressive API so that predicates read naturally::

        vertex.filter(lambda n: (
            n.id.startswith("test_")
            and n.type in {"A", "B", "C"}
            and n.attr("score") < 0.8
            and n.attr("status") != "archived"
        ))

    Attributes
    ----------
    id : str
        The node identifier.
    type : str or None
        Shortcut for ``node.attr["type"]``.
    edges : list[Edge]
        Outgoing edges.
    inverse_edges : list[Edge]
        Incoming edges.
    meta : dict
        Node metadata.
    attrs : dict
        The full attribute dictionary (same as ``node.attr``).
    node : Node
        The underlying :class:`Node` object.
    """

    __slots__ = ("_node",)

    def __init__(self, node: Node):
        object.__setattr__(self, "_node", node)

    # ---- core properties ----

    @property
    def id(self) -> str:
        return self._node.id

    @property
    def type(self):
        """Shortcut for ``node.attr.get("type")``."""
        return self._node.attr.get("type")

    @property
    def edges(self):
        return self._node.edges

    @property
    def inverse_edges(self):
        return self._node.inverse_edges

    @property
    def meta(self):
        return self._node.meta

    @property
    def attrs(self) -> dict:
        """The full attribute dictionary."""
        return self._node.attr

    @property
    def node(self) -> Node:
        """Access the underlying :class:`Node` object."""
        return self._node

    # ---- attribute access ----

    def attr(self, key: str, default=None):
        """Return the value of attribute *key*, or *default* if missing.

        Example
        -------
        >>> n.attr("score")       # returns the value or None
        >>> n.attr("score", 0.0)  # returns the value or 0.0
        """
        return self._node.attr.get(key, default)

    def has_attr(self, key: str) -> bool:
        """Return ``True`` if attribute *key* exists on this node."""
        return key in self._node.attr

    # ---- edge helpers ----

    def has_edge_to(self, target_id: str) -> bool:
        """Return ``True`` if this node has an outgoing edge to *target_id*."""
        for edge in self._node.edges:
            if edge.to_node.id == target_id:
                return True
        return False

    def has_edge_from(self, source_id: str) -> bool:
        """Return ``True`` if this node has an incoming edge from *source_id*."""
        for edge in self._node.inverse_edges:
            if edge.from_node.id == source_id:
                return True
        return False

    @property
    def neighbor_ids(self) -> set:
        """Set of IDs of directly connected outgoing neighbours."""
        return {edge.to_node.id for edge in self._node.edges}

    @property
    def degree(self) -> int:
        """Number of outgoing edges."""
        return len(self._node.edges)

    @property
    def in_degree(self) -> int:
        """Number of incoming edges."""
        return len(self._node.inverse_edges)

    # ---- dunder helpers ----

    def __repr__(self):
        return repr(self._node)

    def __eq__(self, other):
        if isinstance(other, NodeView):
            return self._node.id == other._node.id
        return NotImplemented

    def __hash__(self):
        return hash(self._node.id)


class FilterResult:
    """A result object that can behave as both a Vertex and an iterable of nodes."""
    
    def __init__(self, vertex, nodes):
        self._vertex = vertex
        self._nodes = nodes
    
    def __iter__(self):
        """For backwards compatibility with list(filter_result)"""
        return iter(self._nodes)
    
    def __getattr__(self, name):
        """Delegate all other attributes to the underlying Vertex"""
        return getattr(self._vertex, name)
    
    def to_networkx(self):
        """Delegate to the underlying Vertex"""
        return self._vertex.to_networkx()


def _filter(self, predicate=None, **kwargs):
    """Filter nodes using a predicate function or keyword arguments.

    Parameters
    ----------
    predicate : Callable[[NodeView], bool], optional
        A callable (typically a lambda) that receives a :class:`NodeView` and
        returns ``True`` for nodes that should be kept.  The ``NodeView``
        exposes a clean API::

            n.id                    # node id (str)
            n.type                  # shortcut for attr["type"]
            n.attr("key")           # attribute lookup with optional default
            n.attr("key", default)
            n.has_attr("key")       # check attribute existence
            n.attrs                 # full attribute dict
            n.edges                 # outgoing edges
            n.inverse_edges         # incoming edges
            n.degree                # number of outgoing edges
            n.in_degree             # number of incoming edges
            n.has_edge_to("id")     # check outgoing edge
            n.has_edge_from("id")   # check incoming edge
            n.neighbor_ids          # set of outgoing neighbour ids
            n.node                  # the underlying Node object

    **kwargs
        Keyword arguments passed to the Rust filter implementation
        (``ids``, ``id``, or attribute equality filters).

    Returns
    -------
    Vertex
        A new Vertex containing only the matching nodes and the edges
        between them.

    Examples
    --------
    >>> g = Vertex()
    >>> g.add_node("test_a", {"type": "A", "score": 0.5, "status": "active"})
    >>> g.add_node("test_b", {"type": "B", "score": 0.9, "status": "archived"})
    >>> g.add_node("other",  {"type": "C", "score": 0.3, "status": "active"})
    >>>
    >>> # Lambda / predicate filtering
    >>> result = g.filter(lambda n: (
    ...     n.id.startswith("test_")
    ...     and n.type in {"A", "B", "C"}
    ...     and n.attr("score") < 0.8
    ...     and n.attr("status") != "archived"
    ... ))
    >>>
    >>> # ID-based filtering
    >>> result = g.filter(ids=["test_a", "other"])
    """

    if predicate is not None:
        # Predicate-based filtering — wrap each node in a NodeView
        matching_ids = [
            node.id
            for node in self.nodes.values()
            if predicate(NodeView(node))
        ]

        if not matching_ids:
            return Vertex()

        return self._original_filter(ids=matching_ids)

    elif kwargs:
        return self._original_filter(**kwargs)

    else:
        raise ValueError("Must provide either a predicate function or keyword arguments")


# Store reference to original Rust filter method before overriding
def _setup_filter_method():
    # Get the original Rust filter method
    original_filter = Vertex.filter
    
    # Store it as a private method
    setattr(Vertex, "_original_filter", original_filter)
    
    # Replace with our enhanced version
    setattr(Vertex, "filter", _filter)

_setup_filter_method()


# Add iteration support to Vertex so list(v.filter(...)) works
def _vertex_iter(self):
    """Iterate over the nodes in this Vertex."""
    return iter(self.nodes.values())


def _vertex_len(self):
    """Return the number of nodes in this Vertex."""
    return len(self.nodes)


Vertex.__iter__ = _vertex_iter
Vertex.__len__ = _vertex_len


# Export all public components
__all__ = [
    "Vertex",
    "Node",
    "NodeView",
    "Edge",
    "Path",
    "ObservedDictionary",
    "parse_lgf",
    "parse_lgf_file",
]
