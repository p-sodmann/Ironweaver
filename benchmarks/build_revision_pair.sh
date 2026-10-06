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
benchmark_source=$(mktemp)
trap 'rm -f "$benchmark_source"' EXIT
git -C "$repo" show "$candidate_ref:crates/ironweaver-core/examples/bench_remove_node.rs" > "$benchmark_source"
for revision in baseline candidate; do
  ref=$baseline_ref
  [[ $revision != candidate ]] || ref=$candidate_ref
  source_dir="$artifact_root/source-$revision"
  mkdir "$source_dir" "$artifact_root/$revision" "$artifact_root/wheels-$revision"
  git -C "$repo" archive "$ref" | tar -x -C "$source_dir"
  cp "$repo/Cargo.lock" "$source_dir/Cargo.lock"
  mkdir -p "$source_dir/crates/ironweaver-core/examples"
  cp "$benchmark_source" "$source_dir/crates/ironweaver-core/examples/bench_remove_node.rs"
  (
    cd "$source_dir"
    maturin build --release --locked --interpreter "$(command -v python)" --out "$artifact_root/wheels-$revision"
    cargo build --release --locked -p ironweaver-core --example bench_remove_node
  )
  uv pip install --python "$(command -v python)" --no-deps --target "$artifact_root/$revision" "$artifact_root/wheels-$revision/"*.whl
  cp "$source_dir/target/release/examples/bench_remove_node" "$artifact_root/$revision/bench_remove_node"
done
python - "$artifact_root" <<'PY'
import hashlib
import json
from pathlib import Path
import subprocess
import sys

root = Path(sys.argv[1])
for revision in ('baseline', 'candidate'):
    source = root / f'source-{revision}'
    metadata = {
        'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
        'cargo_lock_sha256': hashlib.sha256((source / 'Cargo.lock').read_bytes()).hexdigest(),
        'flags': ['maturin build --release --locked', 'cargo build --release --locked'],
        'bindings_sha256': hashlib.sha256(b''.join(
            p.read_bytes() for directory in ('src', 'python')
            for p in sorted((source / directory).rglob('*')) if p.is_file())).hexdigest(),
        'graph_sha256': hashlib.sha256((source / 'crates/ironweaver-core/src/graph.rs').read_bytes()).hexdigest(),
    }
    (root / revision / 'build.json').write_text(json.dumps(metadata, indent=2) + '\n')
PY
