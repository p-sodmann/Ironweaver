# Import the Rust extension module classes
from typing import Callable, Iterable

from ._ironweaver import Vertex, Node, Edge, Path, ObservedDictionary

# Import the Python LGF parser
from .lgf_parser import parse_lgf, parse_lgf_file


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
    """Filter nodes using predicate or keyword arguments.

    Parameters
    ----------
    predicate : Callable[[Node], bool], optional
        Callable that receives a :class:`Node` and returns ``True`` for nodes
        that should be included. Returns a FilterResult that behaves as both
        a Vertex and an iterable of nodes.
    **kwargs
        Keyword arguments for filtering (ids, id, or attribute filters).
        Returns a new Vertex.

    Returns
    -------
    FilterResult or Vertex
        When predicate is provided: A FilterResult that can be used as both
        a Vertex (with .to_networkx(), etc.) and as an iterable of nodes.
        When kwargs are provided: A new Vertex containing filtered nodes.

    Example
    -------
    >>> g = Vertex()
    >>> g.add_node("n1", {"type": "keep"})
    >>> g.add_node("n2", {"type": "discard"})
    >>> 
    >>> # Predicate-based filtering
    >>> result = g.filter(lambda n: n.attr.get("type") == "keep")
    >>> result.to_networkx()  # Works as Vertex
    >>> list(result)          # Works as iterable (backwards compatibility)
    >>> 
    >>> # ID-based filtering (returns Vertex)
    >>> filtered_vertex = g.filter(ids=["n1"])
    """

    if predicate is not None:
        # Predicate-based filtering - collect matching nodes
        matching_nodes = [node for node in self.nodes.values() if predicate(node)]
        matching_ids = [node.id for node in matching_nodes]
        
        if not matching_ids:
            # Return FilterResult with empty vertex if no nodes match
            empty_vertex = Vertex()
            return FilterResult(empty_vertex, [])
        
        # Use the Rust implementation with the filtered IDs
        filtered_vertex = self._original_filter(ids=matching_ids)
        return filtered_vertex
    
    elif kwargs:
        # Keyword-based filtering - delegate to Rust implementation
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

# Export all public components
__all__ = [
    "Vertex",
    "Node", 
    "Edge",
    "Path",
    "ObservedDictionary",
    "parse_lgf",
    "parse_lgf_file",
]
