"""Require the compiled extension before collecting any tests.

A missing or broken build must fail collection rather than skip tests, including
the pure-Python helpers and benchmarks.
"""

import ironweaver  # noqa: F401  (raises if the extension module is missing)
