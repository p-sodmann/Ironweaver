# Maintainability and dependency review, 10 October 2026

Base: Ironweaver main `5dc6f5dd26db03dfd46df0f48984774d7010939a`.

## Changes

- `find`, `find_range` and `filter(where=...)` now share one private Rust node selector. It uses property-index candidates when available, otherwise streams node slots, and always evaluates the full expression. Matching order, error propagation and the lifetime of the Python vertex borrow are preserved.
- Versions of the four dependencies shared by the core and Python extension live in `[workspace.dependencies]`. Feature flags remain with the crate that requires them; the core still has no PyO3 dependency.
- Upgrade `rand` from 0.9 to 0.10 and migrate its `Rng` extension trait to `RngExt`. No RNG algorithm or seed handling is changed explicitly.

## Direct crates

All 11 external direct dependencies resolve to their latest non-yanked stable releases after the change. Older version requirements such as `half = "2.2"` are compatible lower bounds, not exact pins. The only direct dependency that required a new compatibility range was `rand`.

| Crate | Before | After / latest stable |
| --- | --- | --- |
| [chrono](https://docs.rs/crate/chrono/latest) | 0.4.45 | 0.4.45 |
| [crc32fast](https://docs.rs/crate/crc32fast/latest) | 1.5.2 | 1.5.2 |
| [foldhash](https://docs.rs/crate/foldhash/latest) | 0.2.0 | 0.2.0 |
| [half](https://docs.rs/crate/half/latest) | 2.7.1 | 2.7.1 |
| [postcard](https://docs.rs/crate/postcard/latest) | 1.1.3 | 1.1.3 |
| [pyo3](https://docs.rs/crate/pyo3/latest) | 0.29.3 | 0.29.3 |
| [rand](https://docs.rs/crate/rand/latest) | 0.9.5 | 0.10.3 |
| [rayon](https://docs.rs/crate/rayon/latest) | 1.12.0 | 1.12.0 |
| [serde](https://docs.rs/crate/serde/latest) | 1.0.229 | 1.0.229 |
| [serde_json](https://docs.rs/crate/serde_json/latest) | 1.0.151 | 1.0.151 |
| [sonic-rs](https://docs.rs/crate/sonic-rs/latest) | 0.5.10 | 0.5.10 |

## Transitive crates

Checked all 103 resolved registry package versions (99 distinct crate names) against the registry index refreshed by Cargo. The complete inventory, including immediate dependants, is in [crate_versions.json](crate_versions.json). The fresh resolution also advances six compatible transitive versions from the local baseline: `cc` 1.6.0 → 1.7.0, `either` 1.18.0 → 1.19.0, `syn` 3.0.6 → 3.0.7, `uuid` 1.27.0 → 1.28.0, and `zerocopy` / `zerocopy-derive` 0.8.60 → 0.8.62.

Fifteen resolved versions remain below the latest stable release because upstream dependencies require their older compatibility ranges. The latest direct parents already impose these requirements; adding a direct dependency or forcing an override does not migrate their code. Some packages are only used on Windows, WASI or UEFI.

| Crate | Resolved | Latest stable | Immediate dependants |
| --- | --- | --- | --- |
| cobs | 0.3.0 | 0.5.1 | postcard 1.1.3 |
| embedded-io | 0.4.0 | 0.7.1 | postcard 1.1.3 |
| embedded-io | 0.6.1 | 0.7.1 | postcard 1.1.3 |
| getrandom | 0.3.4 | 0.4.3 | ahash 0.8.12 |
| r-efi | 5.3.0 | 7.1.0 | getrandom 0.3.4 |
| r-efi | 6.0.0 | 7.1.0 | getrandom 0.4.3 |
| syn | 2.0.119 | 3.0.7 | munge_macro 0.4.7, pyo3-macros 0.29.3, pyo3-macros-backend 0.29.3, windows-implement 0.60.2, windows-interface 0.59.3, zerocopy-derive 0.8.62 |
| wasip2 | 1.0.4+wasi-0.2.12 | 2.0.1+wasi-0.2.12 | getrandom 0.3.4 |
| windows-core | 0.62.2 | 0.100.0 | iana-time-zone 0.1.65 |
| windows-implement | 0.60.2 | 0.100.0 | windows-core 0.62.2 |
| windows-interface | 0.59.3 | 0.100.0 | windows-core 0.62.2 |
| windows-link | 0.2.1 | 0.100.0 | chrono 0.4.45, windows-core 0.62.2, windows-result 0.4.1, windows-strings 0.5.1 |
| windows-result | 0.4.1 | 0.100.0 | windows-core 0.62.2 |
| windows-strings | 0.5.1 | 0.100.0 | windows-core 0.62.2 |
| wit-bindgen | 0.57.1 | 0.62.0 | wasip2 1.0.4+wasi-0.2.12 |

This repository ignores `Cargo.lock`; the local lockfile was refreshed for testing, and the JSON records the tested resolution. CI and fresh builds resolve dependencies from the manifests. This is a point-in-time version review, not a guarantee that every transitive dependency can be upgraded independently or a substitute for the CI vulnerability audit.

## Validation

- Before and after: 164 Rust tests plus 6 doc tests pass.
- Python: 876 tests pass before; the two new order/filter cases also pass against the old extension. The rebuilt extension passes 878 tests after.
- The new cases compare `find`, `find_range` and expression filtering with and without an index after deletion, slot reuse and attribute changes. A combined property/label expression checks that index candidates still receive the full predicate.
- Formatting and Clippy with warnings denied pass. Rustdoc with warnings denied and `cargo package -p ironweaver-core --allow-dirty` both pass; the latter verifies the published crate with inherited workspace dependencies. Core dependency isolation passes.
- [seeded_check.py](seeded_check.py) compares sampled betweenness, Leiden, FastRP, node2vec, ordinary random walks and stratified random walks on a deterministic 40-node/120-edge graph. Each workload uses seeds 0, 1, 7 and 42, in separate processes with one and three Rayon threads. All four JSON outputs (old/new × one/three threads) are byte-identical, SHA-256 `348b885bb6be33fa48c0ad582ef22c7922c71d0c6f51762ddbfc8a84f7ff1e09`. This verifies these cases; it does not prove equivalence for every possible graph.

```sh
cargo update
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p ironweaver-core
maturin develop --release
pytest -q
RAYON_NUM_THREADS=1 python maintenance_results/2026-10-10/seeded_check.py > seeded-1.json
RAYON_NUM_THREADS=3 python maintenance_results/2026-10-10/seeded_check.py > seeded-3.json
cmp seeded-1.json seeded-3.json
```

Run the seed script against both the baseline and rebuilt extensions to compare versions. Save the baseline output before rebuilding.

## Sources

- [crates.io registry index](https://index.crates.io/config.json): highest non-yanked stable version for each resolved crate, excluding prereleases.
- [Rand 0.10 migration guide](https://rust-random.github.io/book/update-0.10.html): trait rename, RNG dependency changes and reproducibility notes.
- Direct crate links in the table point to their published release documentation.
