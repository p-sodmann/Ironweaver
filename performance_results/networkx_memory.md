# ironweaver vs networkx: memory

Generated 2026-09-26 10:18 by `benchmarks/compare_networkx_memory.py`.

| | |
|---|---|
| Python | 3.11.15 (CPython) |
| networkx | 3.6.1 (`MultiDiGraph`) |
| Platform | Linux-6.18.44-fc-v37-x86_64-with-glibc2.39 |
| Repeats | median of 3, each in a fresh process |
| Seed | 42 |

Both libraries hold the same random directed multigraph as in
[networkx_comparison.md](networkx_comparison.md). Memory is the growth of the
process's resident set size (RSS), measured in a fresh interpreter per
measurement, because most of ironweaver's memory lives in Rust allocations
that `tracemalloc` cannot see. Peaks use the kernel's high-water mark
(`VmHWM`) and are only available on Linux. "ironweaver is" compares
ironweaver with networkx (networkx ÷ ironweaver).

## 10,000 nodes, 59,999 edges

### In memory

| Measurement | Description | ironweaver | networkx | ironweaver is | Notes |
|---|---|---:|---:|---:|---|
| Resident graph | nodes (`index`, `group`) and edges (`type`, `weight`) | 35.8 MiB | 41.0 MiB | **1.1× smaller** | ironweaver: 3.7 KiB, networkx: 4.2 KiB per node + 6 edges |
| Resident graph, no attributes | structure only | 19.0 MiB | 31.6 MiB | **1.7× smaller** | ironweaver: 1.9 KiB, networkx: 3.2 KiB per node + 6 edges |
| Peak while loading JSON | graph + parser buffers, from a file | 85.4 MiB | 58.4 MiB | 1.5× larger | `load_from_json` vs `json.load` + `node_link_graph` |
| Extra peak while saving JSON | on top of the resident graph | 21.6 MiB | 13.7 MiB | 1.6× larger | `save_to_json` vs `node_link_data` + `json.dump` |

### On disk

| Measurement | Description | ironweaver | networkx | ironweaver is | Notes |
|---|---|---:|---:|---:|---|
| JSON file | compact JSON | 13.7 MiB | 5.2 MiB | 2.6× larger | `save_to_json` vs `node_link_data` + `json.dump` |
| Binary file | native binary format | 14.0 MiB | 3.4 MiB | 4.1× larger | `save_to_binary` (bincode) vs `pickle` (highest protocol) |

## 100,000 nodes, 599,999 edges

### In memory

| Measurement | Description | ironweaver | networkx | ironweaver is | Notes |
|---|---|---:|---:|---:|---|
| Resident graph | nodes (`index`, `group`) and edges (`type`, `weight`) | 357.7 MiB | 413.6 MiB | **1.2× smaller** | ironweaver: 3.7 KiB, networkx: 4.2 KiB per node + 6 edges |
| Resident graph, no attributes | structure only | 190.0 MiB | 319.8 MiB | **1.7× smaller** | ironweaver: 1.9 KiB, networkx: 3.3 KiB per node + 6 edges |
| Peak while loading JSON | graph + parser buffers, from a file | 858.6 MiB | 587.7 MiB | 1.5× larger | `load_from_json` vs `json.load` + `node_link_graph` |
| Extra peak while saving JSON | on top of the resident graph | 228.9 MiB | 137.4 MiB | 1.7× larger | `save_to_json` vs `node_link_data` + `json.dump` |

### On disk

| Measurement | Description | ironweaver | networkx | ironweaver is | Notes |
|---|---|---:|---:|---:|---|
| JSON file | compact JSON | 145.8 MiB | 53.4 MiB | 2.7× larger | `save_to_json` vs `node_link_data` + `json.dump` |
| Binary file | native binary format | 148.7 MiB | 35.8 MiB | 4.2× larger | `save_to_binary` (bincode) vs `pickle` (highest protocol) |
