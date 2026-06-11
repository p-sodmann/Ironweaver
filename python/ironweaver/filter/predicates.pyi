"""
Type stubs for ironweaver.filter.predicates.

These helpers build callables compatible with :meth:`Vertex.filter`.
All functions return a predicate — a callable that accepts a
:class:`~ironweaver.NodeView` (or raw :class:`~ironweaver.Node`) and returns bool.

Example::

    from ironweaver.filter.predicates import attr_contains, attr_equals, p_or

    pred = p_or(
        attr_contains("Labels", "Field"),
        attr_contains("Labels", "Selector"),
    )
    result = graph.filter(pred)
"""

from __future__ import annotations

from typing import Any, Callable, Union

from ironweaver import Node, NodeView

def attr_equals(key: str, value: Any) -> Callable[[Union[Node, NodeView]], bool]:
    """Return a predicate that matches nodes where ``node.attr(key) == value``."""
    ...

def attr_contains(key: str, member: Any) -> Callable[[Union[Node, NodeView]], bool]:
    """Return a predicate that matches nodes where ``member in node.attr(key)``."""
    ...

def p_and(*predicates: Callable[[Union[Node, NodeView]], bool]) -> Callable[[Union[Node, NodeView]], bool]:
    """Return a predicate that is True only when ALL *predicates* are True."""
    ...

def p_or(*predicates: Callable[[Union[Node, NodeView]], bool]) -> Callable[[Union[Node, NodeView]], bool]:
    """Return a predicate that is True when ANY of *predicates* is True."""
    ...

def p_not(predicate: Callable[[Union[Node, NodeView]], bool]) -> Callable[[Union[Node, NodeView]], bool]:
    """Return a predicate that negates *predicate*."""
    ...
