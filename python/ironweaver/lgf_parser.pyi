from __future__ import annotations

from ironweaver import Vertex

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

        corp_1 Company
          name = "Tech Corp"
          <-founded_by- alice

    Supports ``import(path/to/file.lgf)`` at indent level 0 to compose files.
    """
    ...

def parse_lgf_file(
    path: str,
    graph: Vertex | None = ...,
) -> Vertex:
    """Read *path* and parse its contents as LGF. Resolves imports relative to the file."""
    ...

__all__ = ["parse_lgf", "parse_lgf_file"]
