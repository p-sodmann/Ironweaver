"""Durable SQLite BLOB integration benchmark; not an Ironweaver DB benchmark.

Run against optimized builds with optional lz4/zstandard benchmark packages.
All measured writes include graph encoding, compression and synchronous=FULL
WAL commits. Read times include fetching, decompression and graph reconstruction.
"""
import argparse
import gzip
import hashlib
import importlib
import json
from pathlib import Path
import platform
import sqlite3
import statistics
import subprocess
import tempfile
import time

from ironweaver import Vertex


def codec(name):
    if name == 'raw':
        return (lambda b: b), (lambda b: b)
    if name == 'gzip':
        return (lambda b: gzip.compress(b, compresslevel=1, mtime=0)), gzip.decompress
    if name == 'lz4':
        import lz4.frame
        return lz4.frame.compress, lz4.frame.decompress
    if name == 'zstd':
        import zstandard
        return zstandard.ZstdCompressor(level=1).compress, zstandard.ZstdDecompressor().decompress
    raise ValueError(f'unknown codec: {name}')


def open_store(path):
    conn = sqlite3.connect(path, isolation_level=None)
    assert conn.execute('PRAGMA journal_mode=WAL').fetchone()[0] == 'wal'
    conn.execute('PRAGMA synchronous=FULL')
    assert conn.execute('PRAGMA synchronous').fetchone()[0] == 2
    # Checkpoint separately, so benchmark commits consistently target the WAL.
    conn.execute('PRAGMA wal_autocheckpoint=0')
    conn.execute('CREATE TABLE IF NOT EXISTS graphs (id TEXT PRIMARY KEY, codec TEXT NOT NULL, payload BLOB NOT NULL)')
    return conn


def persist(conn, graphs, name, encode):
    # Encode outside the write transaction to avoid extending the writer lock.
    rows = [(id, name, encode(graph.to_binary())) for id, graph in graphs]
    conn.execute('BEGIN IMMEDIATE')
    try:
        conn.executemany('INSERT INTO graphs VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE '
                         'SET codec=excluded.codec, payload=excluded.payload', rows)
        conn.commit()
    except BaseException:
        conn.rollback()
        raise


def restore(conn, ids, codecs):
    graphs = []
    for id in ids:
        row = conn.execute('SELECT codec, payload FROM graphs WHERE id=?', (id,)).fetchone()
        if row is None:
            raise KeyError(id)
        name, payload = row
        graphs.append(Vertex.from_binary(codecs[name][1](payload)))
    return graphs


def make_graph(n):
    g = Vertex()
    g.add_nodes([(str(i), {'name': f'Node {i}', 'score': i/10, 'tags': ['a', 'b']})
                 for i in range(n)], labels=['Item'])
    g.add_edges([(str(i), str((i+d) % n), {'weight': 1.25})
                 for i in range(n) for d in [1, 2]], type='next')
    return g


def stats(values):
    return {'median_us': statistics.median(values), 'samples_us': values}


def run(directory, nodes, batch, names, repeats, update):
    graph = make_graph(nodes)
    inputs = [(f'user_{i}', graph) for i in range(batch)]
    ids = [id for id, _ in inputs]
    states = {}
    for name in names:
        path = directory / f'{update}-n{nodes}-b{batch}-{name}.sqlite'
        conn = open_store(path)
        codecs = {name: codec(name)}  # reused sequentially, never concurrently
        encode = codecs[name][0]
        persist(conn, inputs, name, encode)
        assert all(len(g) == nodes for g in restore(conn, ids, codecs))
        states[name] = {'conn': conn, 'path': path, 'codecs': codecs,
                        'writes': [], 'reads': [], 'wal_growth': []}
    try:
        for repeat in range(repeats):
            # Business mutation is outside persistence timing; every codec sees
            # the same changed graph, rather than merely a new save timestamp.
            for i in (range(nodes) if update == 'all_attributes' else [0]):
                graph[str(i)].attr_set('score', repeat + 1 + i/10)
            for name in (names if repeat % 2 == 0 else list(reversed(names))):
                state = states[name]
                wal = Path(str(state['path']) + '-wal')
                before = wal.stat().st_size
                start = time.perf_counter()
                persist(state['conn'], inputs, name, state['codecs'][name][0])
                state['writes'].append((time.perf_counter()-start)*1e6)
                state['wal_growth'].append(wal.stat().st_size-before)
                start = time.perf_counter()
                restored = restore(state['conn'], ids, state['codecs'])
                state['reads'].append((time.perf_counter()-start)*1e6)
                assert all(len(g) == nodes for g in restored)
                del restored  # destruction is outside read timing
        results = []
        for name, state in states.items():
            conn = state['conn']
            sizes = conn.execute('SELECT sum(length(payload)) FROM graphs').fetchone()[0]
            conn.execute('PRAGMA wal_checkpoint(TRUNCATE)')
            size = state['path'].stat().st_size
            conn.close()
            # Reopen and check actual data, rather than handles cached in memory.
            state['conn'] = open_store(state['path'])
            loaded = restore(state['conn'], ids, state['codecs'])
            assert all(g['0'].attr == graph['0'].attr for g in loaded)
            results.append({'codec': name, 'nodes_per_graph': nodes, 'edges_per_graph': nodes*2,
                            'graphs_per_commit': batch, 'update': update, 'payload_bytes_total': sizes,
                            'checkpointed_db_bytes': size, 'wal_growth_bytes': state['wal_growth'],
                            'persist_commit': stats(state['writes']), 'read_restore': stats(state['reads'])})
        return results
    finally:
        for state in states.values():
            state['conn'].close()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--directory', type=Path, required=True, help='Choose the filesystem to measure; avoid tmpfs for disk tests')
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--codecs', nargs='+', default=['raw', 'lz4', 'zstd', 'gzip'])
    p.add_argument('--repeats', type=int, default=15)
    p.add_argument('--updates', nargs='+', choices=['single_attribute', 'all_attributes'],
                   default=['single_attribute', 'all_attributes'])
    args = p.parse_args()
    if args.repeats < 1:
        p.error('repeats must be positive')
    args.directory.mkdir(parents=True, exist_ok=True)
    for name in args.codecs:
        codec(name)
    try:
        filesystem = subprocess.check_output(['findmnt','-T',str(args.directory),'-J','-o','TARGET,FSTYPE'],text=True)
    except (OSError, subprocess.CalledProcessError):
        filesystem = None
    ext = Path(importlib.import_module('ironweaver._ironweaver').__file__)
    result = {'platform': platform.platform(), 'sqlite': sqlite3.sqlite_version,
              'extension_sha256': hashlib.sha256(ext.read_bytes()).hexdigest(),
              'filesystem': filesystem, 'journal_mode': 'WAL', 'synchronous': 'FULL',
              'repeats': args.repeats, 'cases': []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.directory) as directory:
        for update in args.updates:
            for n in [1, 10, 100]:
                for batch in [1, 100]:
                    result['cases'].extend(run(Path(directory), n, batch, args.codecs, args.repeats, update))
                    args.output.write_text(json.dumps(result, indent=2))
                    print(f'{update}: {n} nodes, {batch} graphs/commit complete', flush=True)


if __name__ == '__main__':
    main()
