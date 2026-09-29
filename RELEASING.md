# Releasing

A release publishes the Python package `ironweaver` to PyPI (wheels for
Linux, macOS and Windows, Python 3.9–3.14, plus the sdist) and the Rust crate
`ironweaver-core` to crates.io, from one tag. `.github/workflows/release.yml`
does the work.

## One-time setup

1. **PyPI trusted publishing** (no API token stored anywhere): on
   <https://pypi.org/manage/account/publishing/>, add a pending publisher for
   the project `ironweaver`: owner `p-sodmann`, repository `Ironweaver`,
   workflow `release.yml`, environment `pypi`.
2. **crates.io token:** create an API token on
   <https://crates.io/settings/tokens> with the `publish-new` and
   `publish-update` scopes, and store it as the secret
   `CARGO_REGISTRY_TOKEN` of a GitHub environment named `crates-io`
   (Settings → Environments). After the first release, crates.io's trusted
   publishing can replace the token.
3. **Documentation site:** Settings → Pages → Source: *GitHub Actions*. The
   `Docs` workflow then publishes the site on every push to `main`.
4. Optionally, require a reviewer for the `pypi` and `crates-io`
   environments, so every publish waits for a click.

## Making a release

1. Pick the version (semantic versioning; while it is 0.x, a minor release
   may break the API).
2. Set it in `Cargo.toml` and `crates/ironweaver-core/Cargo.toml` (and the
   `version = "..."` of the `ironweaver-core` dependency in `Cargo.toml`).
   `pyproject.toml` takes it from `Cargo.toml`.
3. In `CHANGELOG.md`, turn the `## X.Y.Z — unreleased` heading into
   `## X.Y.Z — YYYY-MM-DD` and check the breaking changes are listed.
4. Merge that to `main` and wait for CI to pass.
5. Tag and push:

   ```text
   git tag -a vX.Y.Z -m "ironweaver X.Y.Z"
   git push origin vX.Y.Z
   ```

The workflow then:

- checks that the tag, both `Cargo.toml` versions and the changelog agree;
- builds wheels (manylinux 2.28 and musllinux 1.2 for x86_64 and aarch64,
  macOS Intel and Apple Silicon, Windows x64) with the `dist` profile (fat
  LTO), and the sdist;
- installs the wheels on every platform (Python 3.9 and 3.14) and runs the
  test suite against them, and builds and tests the sdist;
- publishes to PyPI and crates.io, then creates a GitHub release with the
  changelog section and the files.

To try all of it without publishing, run the workflow by hand
(Actions → Release → Run workflow); pull requests that change the packaging
files run it too.

## If something goes wrong

- A failed build or test publishes nothing: fix it, move the tag
  (`git tag -f`, `git push -f origin vX.Y.Z`) and the workflow runs again.
- A version can't be uploaded to PyPI or crates.io twice. If one of them
  already has it, release the fix as X.Y.Z+1.
- To withdraw a broken release, yank it (PyPI: the project's release page;
  crates.io: `cargo yank --version X.Y.Z ironweaver-core`).
