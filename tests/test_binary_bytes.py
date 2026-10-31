"""Binary bytes retain the existing file format and validation."""
import datetime as dt

import pytest

from ironweaver import Vertex


def test_binary_bytes_roundtrip_and_file_compatibility(tmp_path):
    g = Vertex()
    g.add_node('a', {'none': None, 'data': b'abc', 'day': dt.date(2026, 10, 31),
                     'nested': {'values': [1, False, 2.5]}}, labels=['Item'])
    g.add_node('b', {'score': 1})
    deleted = g.add_edge('b', 'a')
    deleted_id = deleted.id
    g.remove_edge('b', 'a')
    e = g.add_edge('a', 'b', {'weight': 1.25}, type='next')
    e.meta = {'note': 'edge'}
    g['a'].meta = {'note': 'node'}
    g.meta['title'] = 'tiny'
    g.create_index('score')
    blob = g.to_binary()
    assert isinstance(blob, bytes)
    memory = Vertex.from_binary(blob)
    path = tmp_path / 'graph.bin'
    path.write_bytes(blob)
    file = Vertex.load_from_binary(str(path))
    g.save_to_binary(str(path))
    from_file_bytes = Vertex.from_binary(path.read_bytes())
    for loaded in [memory, file, from_file_bytes]:
        assert loaded.keys() == g.keys()
        assert loaded.meta == g.meta
        assert loaded['a'].attr == g['a'].attr
        assert loaded['a'].meta == g['a'].meta
        assert loaded.get_edge(e.id).attr == e.attr
        assert loaded.get_edge(e.id).meta == e.meta
        assert loaded.indexes == ['score']
        assert [n.id for n in loaded.find('score', 1)] == ['b']
        with pytest.raises(KeyError):
            loaded.get_edge(deleted_id)
        assert loaded.add_edge('a', 'b').id > e.id


def test_empty_bytes_roundtrip():
    g = Vertex.from_binary(Vertex().to_binary())
    assert len(g) == 0 and g.meta == {}


@pytest.mark.parametrize('bad', [b'', b'unframed', b'IWGR'])
def test_invalid_binary_bytes(bad):
    with pytest.raises(RuntimeError):
        Vertex.from_binary(bad)


def test_corrupted_binary_bytes():
    blob = bytearray(Vertex().to_binary())
    blob[len(blob) // 2] ^= 1
    with pytest.raises(RuntimeError, match='checksum'):
        Vertex.from_binary(bytes(blob))
