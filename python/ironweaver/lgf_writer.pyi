from __future__ import annotations

from typing import Any, Iterable, Mapping

from ironweaver import Vertex

def to_lgf(
    graph: Vertex,
    order: Iterable[str] | None = ...,
    banners: Mapping[str, str] | None = ...,
    header: str | None = ...,
) -> str:
    """Serialize *graph* to LGF text; the inverse of :func:`parse_lgf`."""
    ...

def to_lgf_file(graph: Vertex, path: str, **kwargs: Any) -> None:
    """Write *graph* as LGF to *path*; keyword arguments go to :func:`to_lgf`."""
    ...

__all__ = ["to_lgf", "to_lgf_file"]
