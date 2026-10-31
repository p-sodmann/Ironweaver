"""Measure compressing binary graph bytes: size, codec cost and full round trips.

Optional benchmark-only packages: zstandard, lz4. Run with an optimized build.
Compression is outside the binary format; decompress before Vertex.from_binary.
"""
import gzip
import hashlib
import importlib
import json
from pathlib import Path
import statistics
import sys
import timeit
import zlib

from ironweaver import Vertex


def measure(fn, number=300):
    fn()
    values = [v / number * 1e6 for v in timeit.repeat(fn, number=number, repeat=5)]
    return {'median_us': statistics.median(values), 'samples_us': values}


def codecs():
    result = [
        ('none', lambda b: b, lambda b: b),
        ('gzip_1', lambda b: gzip.compress(b, compresslevel=1, mtime=0), gzip.decompress),
        ('gzip_6', lambda b: gzip.compress(b, compresslevel=6, mtime=0), gzip.decompress),
        ('zlib_1', lambda b: zlib.compress(b, 1), zlib.decompress),
        ('zlib_6', lambda b: zlib.compress(b, 6), zlib.decompress),
        ('raw_deflate_1', lambda b: zlib.compress(b, 1, wbits=-15), lambda b: zlib.decompress(b, wbits=-15)),
    ]
    try:
        import zstandard as zstd
        for level in [1, 3]:
            compressor, decompressor = zstd.ZstdCompressor(level=level), zstd.ZstdDecompressor()
            result.append((f'zstd_{level}_reused', compressor.compress, decompressor.decompress))
        result.append(('zstd_1_fresh', lambda b: zstd.ZstdCompressor(level=1).compress(b),
                       lambda b: zstd.ZstdDecompressor().decompress(b)))
    except ImportError:
        pass
    try:
        import lz4.frame
        result.append(('lz4_frame', lz4.frame.compress, lz4.frame.decompress))
    except ImportError:
        pass
    return result


def main():
    extension = Path(importlib.import_module('ironweaver._ironweaver').__file__)
    results = {'python': sys.version,
               'extension_sha256': hashlib.sha256(extension.read_bytes()).hexdigest(),
               'zlib': zlib.ZLIB_RUNTIME_VERSION, 'cases': []}
    for count, profile in [(0, 'scalar'), (1, 'scalar'), (10, 'scalar'), (100, 'scalar'),
                           (1, 'embedding'), (10, 'embedding')]:
        g = Vertex()
        def attrs(i):
            value = {'name': f'Node {i}', 'score': i / 10, 'tags': ['a', 'b']}
            if profile == 'embedding':
                # Deterministic pseudo-random numbers, not repeated constant vectors.
                import random
                rng = random.Random(i)
                value['embedding'] = [rng.uniform(-1, 1) for _ in range(384)]
            return value
        g.add_nodes([(str(i), attrs(i)) for i in range(count)], labels=['Item'])
        g.add_edges([(str(i), str((i + offset) % count), {'weight': 1.25})
                     for i in range(count) for offset in [1, 2]], type='next')
        raw = g.to_binary()
        case = {'nodes': count, 'edges': count * 2, 'profile': profile,
                'raw_bytes': len(raw), 'codecs': []}
        for name, encode, decode in codecs():
            compressed = encode(raw)
            assert decode(compressed) == raw
            restored = Vertex.from_binary(decode(compressed))
            assert restored.node_count() == count
            if count:
                assert restored['0'].attr == g['0'].attr
            row = {'codec': name, 'bytes': len(compressed), 'ratio': len(compressed) / len(raw),
                   'compress': measure(lambda: encode(raw)),
                   'decompress': measure(lambda: decode(compressed)),
                   'graph_encode_and_compress': measure(lambda: encode(g.to_binary())),
                   'decompress_and_graph_decode': measure(lambda: Vertex.from_binary(decode(compressed)))}
            case['codecs'].append(row)
        results['cases'].append(case)
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
