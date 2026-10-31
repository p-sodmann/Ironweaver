"""Native NodeView adjacency queries preserve directed multigraph semantics."""
import pytest

from ironweaver import Node, NodeView, Vertex


def assert_matches_snapshots(node):
    view = NodeView(node)
    outgoing = node.edges
    incoming = node.inverse_edges
    assert view.degree == len(outgoing)
    assert view.in_degree == len(incoming)
    assert view.neighbor_ids == {edge.to_node.id for edge in outgoing}
    for id in [*node.vertex.keys(), 'missing']:
        assert view.has_edge_to(id) == any(edge.to_node.id == id for edge in outgoing)
        assert view.has_edge_from(id) == any(edge.from_node.id == id for edge in incoming)


@pytest.mark.parametrize('seed', [None, 42])
def test_multigraph_mutations(seed):
    g = Vertex(shuffle_seed=seed)
    g.add_nodes(['a', 'b', 'c', 'isolated'])
    g.add_edges([('a', 'b'), ('a', 'b'), ('a', 'a'), ('c', 'a')])
    a, b = g['a'], g['b']
    for node in g:
        assert_matches_snapshots(node)
    view = NodeView(a)
    assert (view.degree, view.in_degree) == (3, 2)
    assert view.neighbor_ids == {'a', 'b'}
    assert not view.has_edge_to('c') and view.has_edge_from('c')
    snapshot = view.neighbor_ids
    snapshot.add('unrelated')
    assert view.neighbor_ids == {'a', 'b'}
    b.id = 'renamed'
    assert view.neighbor_ids == {'a', 'renamed'}
    assert not view.has_edge_to('b') and view.has_edge_to('renamed')
    g.remove_edge('a', 'renamed')
    assert_matches_snapshots(a)
    g.remove_node('c')
    g.add_node('replacement')  # exercise arena slot reuse
    assert not view.has_edge_from('replacement')
    assert_matches_snapshots(a)
    g.remove_node('a')
    g.add_node('a')
    for query in [lambda: view.degree, lambda: view.in_degree,
                  lambda: view.neighbor_ids, lambda: view.has_edge_to('missing'),
                  lambda: view.has_edge_from('a')]:
        with pytest.raises(RuntimeError, match='removed'):
            query()


def test_queries_do_not_materialize_handles(monkeypatch):
    g = Vertex()
    g.add_nodes(['a', 'b'])
    g.add_edges([('a', 'b'), ('b', 'a'), ('a', 'a')])
    view = NodeView(g['a'])

    def no_snapshot(self):
        raise AssertionError('adjacency query must not create edge snapshots')

    monkeypatch.setattr(Node, 'edges', property(no_snapshot))
    monkeypatch.setattr(Node, 'inverse_edges', property(no_snapshot))
    assert view.degree == view.in_degree == 2
    assert view.has_edge_to('b') and view.has_edge_from('b')
    assert view.neighbor_ids == {'a', 'b'}
    assert g.filter(lambda n: n.degree == 2).keys() == ['a']
