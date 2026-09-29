"""Fail loudly when the compiled extension isn't installed.

Several test modules skip themselves when `import ironweaver` fails (so the
pure-Python helpers can be tested alone); without this check a broken build
would show up as skipped tests instead of an error.
"""

import ironweaver  # noqa: F401  (raises if the extension module is missing)
