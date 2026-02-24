"""Predicate helpers for :meth:`Vertex.filter`.

This module embraces a tiny, function-based approach to filtering graph nodes.
Rather than implementing a custom query language, :meth:`Vertex.filter`
accepts any callable that receives a :class:`~ironweaver.NodeView` and returns
a boolean.  The utilities below simply build and compose such callables.

Example
-------
>>> from ironweaver import Vertex
>>> from ironweaver.filter.predicates import attr_contains, p_or
>>> g = Vertex()
>>> g.add_node("n1", {"Labels": ["Field"]})
>>> g.add_node("n2", {"Labels": ["Selector"]})
>>> predicate = p_or(attr_contains("Labels", "Field"),
...                  attr_contains("Labels", "Selector"))
>>> [n.id for n in g.filter(predicate)]
['n1', 'n2']
"""

from __future__ import annotations

from typing import Any, Callable, Union

from .. import Node, NodeView


def _get_attr(node: Union[Node, NodeView], key: str, default=None):
    """Get attribute value from either a Node or NodeView."""
    if isinstance(node, NodeView):
        return node.attr(key, default)
    return node.attr.get(key, default)


def attr_equals(key: str, value: Any) -> Callable[[Union[Node, NodeView]], bool]:
    """Match nodes where ``node.attr(key) == value``.

    Example
    -------
    >>> pred = attr_equals("type", "Field")
    >>> graph.filter(pred)  # doctest: +SKIP
    """

    def _predicate(node: Union[Node, NodeView]) -> bool:
        return _get_attr(node, key) == value

    return _predicate


def attr_contains(key: str, member: Any) -> Callable[[Union[Node, NodeView]], bool]:
    """Match nodes where ``member`` is found in ``node.attr(key)``.

    Example
    -------
    >>> pred = attr_contains("Labels", "Field")
    >>> graph.filter(pred)  # doctest: +SKIP
    """

    def _predicate(node: Union[Node, NodeView]) -> bool:
        value = _get_attr(node, key)
        if value is None:
            return False
        try:
            return member in value
        except TypeError:
            return False

    return _predicate


def p_and(*predicates: Callable[[Node], bool]) -> Callable[[Node], bool]:
    """Logical AND of multiple predicates.

    Example
    -------
    >>> pred = p_and(attr_equals("type", "Field"),
    ...              attr_contains("Labels", "Field"))
    >>> graph.filter(pred)  # doctest: +SKIP
    """

    def _predicate(node: Node) -> bool:
        return all(p(node) for p in predicates)

    return _predicate


def p_or(*predicates: Callable[[Node], bool]) -> Callable[[Node], bool]:
    """Logical OR of multiple predicates.

    Example
    -------
    >>> pred = p_or(attr_contains("Labels", "Field"),
    ...             attr_contains("Labels", "Selector"))
    >>> graph.filter(pred)  # doctest: +SKIP
    """

    def _predicate(node: Node) -> bool:
        return any(p(node) for p in predicates)

    return _predicate


def p_not(predicate: Callable[[Node], bool]) -> Callable[[Node], bool]:
    """Negate a predicate.

    Example
    -------
    >>> pred = p_not(attr_equals("type", "Field"))
    >>> graph.filter(pred)  # doctest: +SKIP
    """

    def _predicate(node: Node) -> bool:
        return not predicate(node)

    return _predicate
