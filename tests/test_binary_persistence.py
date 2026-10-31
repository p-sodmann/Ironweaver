"""Integration with reopened, synchronous WAL-backed BLOB storage."""
import importlib.util
from pathlib import Path
import sqlite3

import pytest

_PATH = Path(__file__).resolve().parents[1] / 'benchmarks' / 'persist_binary.py'
_SPEC = importlib.util.spec_from_file_location('persist_binary', _PATH)
_BENCH = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_BENCH)


@pytest.mark.parametrize('name', ['raw', 'gzip', 'lz4', 'zstd'])
def test_persisted_graph_roundtrip_after_reopen(tmp_path, name):
    codecs = {'raw': _BENCH.codec('raw'), name: _BENCH.codec(name)}
    path = tmp_path / 'users.sqlite'
    alice, bob = _BENCH.make_graph(10), _BENCH.make_graph(1)
    alice.meta['owner'] = 'alice'
    bob.meta['owner'] = 'bob'
    alice['0'].meta = {'note': 'node metadata'}
    alice.create_index('score')
    conn = _BENCH.open_store(path)
    try:
        _BENCH.persist(conn, [('alice', alice), ('bob', bob)], name, codecs[name][0])
        # Stored codec tags, rather than a global setting, select each decoder.
        _BENCH.persist(conn, [('bob', bob)], 'raw', codecs['raw'][0])
    finally:
        conn.close()
    conn = _BENCH.open_store(path)
    try:
        restored = _BENCH.restore(conn, ['alice', 'bob'], codecs)
        for expected, actual in zip([alice, bob], restored):
            assert actual.meta == expected.meta
            assert set(actual.keys()) == set(expected.keys())
            for id in expected.keys():
                assert actual[id].attr == expected[id].attr
                assert actual[id].meta == expected[id].meta
                assert [(e.id, e.to_node.id, e.attr) for e in actual[id].edges] == [
                    (e.id, e.to_node.id, e.attr) for e in expected[id].edges]
        assert [n.id for n in restored[0].find('score', 0)] == ['0']
        # An update to one user must survive another reopen and leave the other intact.
        restored[0]['0'].attr_set('name', 'changed')
        _BENCH.persist(conn, [('alice', restored[0])], name, codecs[name][0])
    finally:
        conn.close()
    conn = _BENCH.open_store(path)
    try:
        a, b = _BENCH.restore(conn, ['alice', 'bob'], codecs)
        assert a['0'].attr_get('name') == 'changed'
        assert b['0'].attr_get('name') == 'Node 0'
    finally:
        conn.close()


def test_failed_batch_rolls_back(tmp_path):
    conn = _BENCH.open_store(tmp_path / 'graphs.sqlite')
    try:
        conn.execute("CREATE TRIGGER reject_bad BEFORE INSERT ON graphs WHEN NEW.id='bad' "
                     "BEGIN SELECT RAISE(ABORT, 'rejected'); END")
        graph = _BENCH.make_graph(1)
        with pytest.raises(sqlite3.IntegrityError, match='rejected'):
            _BENCH.persist(conn, [('good', graph), ('bad', graph)], 'raw', lambda b: b)
        assert conn.execute('SELECT count(*) FROM graphs').fetchone()[0] == 0
    finally:
        conn.close()
