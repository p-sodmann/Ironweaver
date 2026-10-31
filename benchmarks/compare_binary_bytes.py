"""Compare binary bytes encoding with durable file saving for tiny graphs.

Run against an optimized build. Filesystem timings include atomic writes and
syncs; in-memory results still need storage-layer persistence for durability.
"""
from pathlib import Path
import hashlib
import importlib
import json
import platform
import statistics
import sys
import tempfile
import timeit

from ironweaver import Vertex


def measure(fn, number):
    fn()
    samples = [value / number * 1e6 for value in timeit.repeat(fn, number=number, repeat=7)]
    return {'median_us': statistics.median(samples), 'samples_us': samples}


def main():
    extension = Path(importlib.import_module('ironweaver._ironweaver').__file__)
    results = {'python': sys.version, 'platform': platform.platform(),
               'extension_sha256': hashlib.sha256(extension.read_bytes()).hexdigest(), 'cases': []}
    with tempfile.TemporaryDirectory() as directory:
        path = str(Path(directory) / 'graph.bin')
        for count in [0, 1, 10, 100]:
            g = Vertex()
            g.add_nodes([(str(i), {'name': f'Node {i}', 'score': i / 10, 'tags': ['a', 'b']})
                         for i in range(count)], labels=['Item'])
            g.add_edges([(str(i), str((i + offset) % count), {'weight': 1.25})
                         for i in range(count) for offset in [1, 2]], type='next')
            blob = g.to_binary()
            g.save_to_binary(path)
            assert Vertex.from_binary(blob).node_count() == count
            assert Vertex.load_from_binary(path).node_count() == count
            case = {'nodes': count, 'edges': count * 2, 'binary_bytes': len(blob),
                    'binary_encode_memory': measure(g.to_binary, 1000),
                    'binary_decode_memory': measure(lambda: Vertex.from_binary(blob), 1000),
                    'binary_save_atomic_file': measure(lambda: g.save_to_binary(path), 100),
                    'binary_load_warm_file': measure(lambda: Vertex.load_from_binary(path), 100),
                    'json_encode_memory': measure(g.save_to_json, 1000)}
            results['cases'].append(case)
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
