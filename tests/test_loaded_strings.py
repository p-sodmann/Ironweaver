"""The load cache shares short strings without changing dictionaries or order."""
import pytest

from ironweaver import Vertex


@pytest.mark.parametrize('format', ['binary', 'json'])
def test_loaded_string_cache(format):
    g = Vertex()
    repeated = '日本語🙂 shared'
    attributes = {f'key_{i}': f'value_{i}' for i in range(100)}
    attributes['κλειδί'] = repeated
    g.add_node('a', attributes, labels=['Item'])
    g.add_node('b', attributes)
    g.add_edge('a', 'b', {'shared': repeated})
    g.add_edge('b', 'a', {'shared': repeated})
    g.meta['shared'] = repeated
    g.create_index('key_0')
    loaded = Vertex.from_binary(g.to_binary()) if format == 'binary' else Vertex.load_from_json(g.save_to_json())
    assert loaded['a'].attr == g['a'].attr
    assert loaded['b'].attr == g['b'].attr
    assert [e.to_node.id for e in loaded['a'].edges] == ['b']
    value = loaded['a'].attr_get('κλειδί')
    assert value is loaded['b'].attr_get('κλειδί')
    assert value is loaded['a'].edges[0].attr_get('shared')
    assert value is loaded.meta['shared']
    assert [n.id for n in loaded.find('key_0', 'value_0')] == ['a', 'b']
