# Import the Rust extension module classes
from typing import Callable, Iterable

from ._ironweaver import Vertex, Node, Edge, Path, ObservedDictionary

# Import the Python LGF parser
from .lgf_parser import parse_lgf, parse_lgf_file


def _filter(self, predicate: Callable[[Node], bool]) -> Iterable[Node]:
    """Filter nodes using ``predicate``.

    Parameters
    ----------
    predicate:
        Callable that receives a :class:`Node` and returns ``True`` for nodes
        that should be included.

    Returns
    -------
    Iterable[Node]
        Nodes that satisfy the predicate.

    Example
    -------
    >>> g = Vertex()
    >>> g.add_node("n1", {"type": "keep"})
    >>> g.add_node("n2", {"type": "discard"})
    >>> list(g.filter(lambda n: n.attr.get("type") == "keep"))
    [g["n1"]]
    """

    return [node for node in self.nodes.values() if predicate(node)]


# Expose the Python-level filter on the Vertex class
setattr(Vertex, "filter", _filter)

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
