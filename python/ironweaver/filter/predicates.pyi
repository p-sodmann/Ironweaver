"""
Type stubs for ironweaver.filter.predicates.

These helpers build callables compatible with :meth:`Vertex.filter`.
All functions return a predicate — a callable that accepts a
:class:`~ironweaver.NodeView` (or raw :class:`~ironweaver.Node`) and returns bool.

Example::

    from ironweaver.filter.predicates import attr_contains, attr_equals, p_or

    # Match nodes whose "Labels" list contains "Field" OR "Selector"
    pred = p_or(
        attr_contains("Labels", "Field"),
        attr_contains("Labels", "Selector"),
    )
    result = graph.filter(pred)
"""

from __future__ import annotations

from typing import Any, Callable, Union

from ironweaver import Node, NodeView

# Predicate type alias
Predicate = Callable[[Union[Node, NodeView]], bool]

def attr_equals(key: str, value: Any) -> Predicate:
    """Return a predicate that matches nodes where ``node.attr(key) == value``."""
    ...

def attr_contains(key: str, member: Any) -> Predicate:
    """Return a predicate that matches nodes where ``member in node.attr(key)``."""
    ...

def p_and(*predicates: Predicate) -> Predicate:
    """Return a predicate that is True only when ALL *predicates* are True."""
    ...

def p_or(*predicates: Predicate) -> Predicate:
    """Return a predicate that is True when ANY of *predicates* is True."""
    ...

def p_not(predicate: Predicate) -> Predicate:
    """Return a predicate that negates *predicate*."""
    ...

__all__ = ["attr_equals", "attr_contains", "p_and", "p_or", "p_not"]
