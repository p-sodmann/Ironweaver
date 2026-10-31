"""View attribute queries match snapshots without materializing dictionaries."""
import pytest

from ironweaver import Edge, EdgeView, Node, NodeView, Vertex
from ironweaver.filter.predicates import _get_attr


@pytest.mark.parametrize('reserved', [False, True])
def test_view_attribute_snapshot_semantics(reserved):
    g = Vertex()
    values = {'none': None, 'false': False, 'zero': 0, 'empty': '', 'items': []}
    node = g.add_node('a', {**values, 'labels': 7}, labels=['Item'] if reserved else None)
    g.add_node('b')
    edge = g.add_edge('a', 'b', {**values, 'type': 7}, type='next' if reserved else None)
    default = object()
    for item, view in [(node, NodeView(node)), (edge, EdgeView(edge))]:
        snapshot = item.attr
        for key in [*values, 'labels', 'type', 'missing']:
            assert view.attr(key, default) == snapshot.get(key, default)
            assert view.has_attr(key) == (key in snapshot)
        assert view.attr('none', default) is None
        assert view.attr('missing', default) is default
        assert view.attr('items') is values['items']
    assert _get_attr(node, 'missing', default) is default
    assert _get_attr(node, 'none', default) is None


def test_missing_reserved_attributes_and_mutation():
    g = Vertex()
    a = g.add_node('a')
    g.add_node('b')
    e = g.add_edge('a', 'b')
    nv, ev = NodeView(a), EdgeView(e)
    default = object()
    assert nv.attr('labels', default) is default
    assert not nv.has_attr('labels')
    assert ev.attr('type', default) is default
    assert not ev.has_attr('type')
    a.add_label('Item')
    e.type = 'next'
    assert nv.attr('labels') == ['Item'] and nv.has_attr('labels')
    assert ev.attr('type') == 'next' and ev.has_attr('type')
    a.remove_label('Item')
    e.type = None
    assert not nv.has_attr('labels') and not ev.has_attr('type')


def test_queries_do_not_materialize_snapshots(monkeypatch):
    g = Vertex()
    a = g.add_node('a', {'value': 1, 'none': None})
    g.add_node('b')
    e = g.add_edge('a', 'b', {'value': 2, 'none': None})

    def no_snapshot(self):
        raise AssertionError('single-key queries must not copy attribute dictionaries')

    monkeypatch.setattr(Node, 'attr', property(no_snapshot))
    monkeypatch.setattr(Edge, 'attr', property(no_snapshot))
    for view in [NodeView(a), EdgeView(e)]:
        assert view.attr('value') in [1, 2]
        assert view.has_attr('none')
        assert not view.has_attr('missing')
    assert _get_attr(a, 'value') == 1
    assert g.filter(lambda n: n.attr('value') == 1).keys() == ['a']
    g.remove_node('a')
    for view in [NodeView(a), EdgeView(e)]:
        with pytest.raises(RuntimeError, match='removed'):
            view.attr('value')
        with pytest.raises(RuntimeError, match='removed'):
            view.has_attr('value')
