#!/usr/bin/env bash
# Build frozen artifacts for a same-machine revision comparison.
set -euo pipefail
if [[ $# != 3 ]]; then
  echo "Usage: $0 BASELINE_REF CANDIDATE_REF OUTPUT_DIRECTORY" >&2
  exit 2
fi
repo=$(git rev-parse --show-toplevel)
baseline_ref=$1
candidate_ref=$2
mkdir -p "$3"
artifact_root=$(cd "$3" && pwd)
if [[ -e "$artifact_root/baseline" || -e "$artifact_root/candidate" ]]; then
  echo "Use a fresh output directory to avoid mixing artifacts." >&2
  exit 2
fi
test -f "$repo/Cargo.lock" || { echo "Generate Cargo.lock first." >&2; exit 2; }
benchmark_source=$(mktemp -d)
trap 'rm -rf "$benchmark_source"' EXIT
for example in bench_remove_node traversal_bench construction_bench; do
  git -C "$repo" show "$candidate_ref:crates/ironweaver-core/examples/$example.rs" > "$benchmark_source/$example.rs"
done
for revision in baseline candidate; do
  ref=$baseline_ref
  [[ $revision != candidate ]] || ref=$candidate_ref
  source_dir="$artifact_root/source-$revision"
  mkdir "$source_dir" "$artifact_root/$revision" "$artifact_root/wheels-$revision"
  git -C "$repo" archive "$ref" | tar -x -C "$source_dir"
  cp "$repo/Cargo.lock" "$source_dir/Cargo.lock"
  mkdir -p "$source_dir/crates/ironweaver-core/examples"
  cp "$benchmark_source/"*.rs "$source_dir/crates/ironweaver-core/examples/"
  # Archives retain commit timestamps. Force freshness when sharing a target
  # directory, otherwise Cargo can reuse a different revision's older artifact.
  python - "$source_dir" <<'STAMP'
from pathlib import Path
import sys
for path in Path(sys.argv[1]).rglob('*.rs'):
    path.touch()
STAMP
  (
    cd "$source_dir"
    # Keep dependency caches, but discard this package's examples and core
    # artifacts so a partial/shared-target rebuild cannot freeze stale binaries.
    cargo clean --release -p ironweaver-core
    maturin build --release --locked --interpreter "$(command -v python)" --out "$artifact_root/wheels-$revision"
    cargo build --release --locked -p ironweaver-core --examples
  )
  uv pip install --python "$(command -v python)" --no-deps --target "$artifact_root/$revision" "$artifact_root/wheels-$revision/"*.whl
  for example in bench_remove_node traversal_bench construction_bench; do
    cp "${CARGO_TARGET_DIR:-$source_dir/target}/release/examples/$example" "$artifact_root/$revision/$example"
  done
done
python - "$artifact_root" "$baseline_ref" "$candidate_ref" <<'PY'
import hashlib
import json
from pathlib import Path
import subprocess
import sys

root = Path(sys.argv[1])
for revision in ('baseline', 'candidate'):
    source = root / f'source-{revision}'
    metadata = {
        'revision_ref': subprocess.check_output(['git', 'rev-parse', sys.argv[2 if revision == 'baseline' else 3]], text=True).strip(),
        'clean_core_package_before_build': True,
        'binaries': {name: hashlib.sha256((root / revision / name).read_bytes()).hexdigest()
                     for name in ('bench_remove_node', 'traversal_bench', 'construction_bench')},
        'benchmark_sources': {name: hashlib.sha256((source / 'crates/ironweaver-core/examples' / (name + '.rs')).read_bytes()).hexdigest()
                              for name in ('bench_remove_node', 'traversal_bench', 'construction_bench')},
        'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
        'cargo_lock_sha256': hashlib.sha256((source / 'Cargo.lock').read_bytes()).hexdigest(),
        'flags': ['maturin build --release --locked', 'cargo build --release --locked'],
        'rustflags': __import__('os').environ.get('RUSTFLAGS', ''),
        'bindings_sha256': hashlib.sha256(b''.join(
            p.read_bytes() for directory in ('src', 'python')
            for p in sorted((source / directory).rglob('*')) if p.is_file())).hexdigest(),
        'core_sha256': hashlib.sha256(b''.join(p.read_bytes() for p in sorted((source / 'crates/ironweaver-core/src').rglob('*.rs')))).hexdigest(),
        'graph_sha256': hashlib.sha256((source / 'crates/ironweaver-core/src/graph.rs').read_bytes()).hexdigest(),
    }
    (root / revision / 'build.json').write_text(json.dumps(metadata, indent=2) + '\n')
PY
