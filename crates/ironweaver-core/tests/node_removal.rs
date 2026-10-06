//! Node deletion must preserve surviving adjacency order and invalidate every
//! removed handle/id, including parallel edges, loops and reused arena slots.
use std::cell::Cell;
use std::rc::Rc;

use ironweaver_core::{EdgeId, Graph, NodeIx};
use rand::{Rng, SeedableRng};

fn check_removal(g: &mut Graph<(), usize>, removed: NodeIx) {
    let edges: Vec<_> = g.edges().map(|(ix, e)| (ix, e.id(), e.source(), e.target(), e.data)).collect();
    let lists: Vec<_> = g
        .nodes()
        .filter(|(ix, _)| *ix != removed)
        .map(|(ix, n)| (ix, n.out_edges().to_vec(), n.in_edges().to_vec()))
        .collect();
    let survives = |ix| edges.iter().any(|&(e, _, from, to, _)| e == ix && from != removed && to != removed);
    g.remove_node(removed).unwrap();
    assert!(g.node(removed).is_none());
    assert!(g.remove_node(removed).is_none());
    let mut expected_edges = 0;
    for &(ix, id, from, to, data) in &edges {
        if from == removed || to == removed {
            assert!(g.edge(ix).is_none());
            assert!(g.edge_ix(id).is_none());
        } else {
            expected_edges += 1;
            let e = g.edge(ix).unwrap();
            assert_eq!((e.source(), e.target(), e.id(), e.data), (from, to, id, data));
            assert_eq!(g.edge_ix(id), Some(ix));
        }
    }
    assert_eq!(g.edge_count(), expected_edges);
    for (ix, out, inc) in lists {
        assert_eq!(g.node(ix).unwrap().out_edges(), out.into_iter().filter(|&e| survives(e)).collect::<Vec<_>>());
        assert_eq!(g.node(ix).unwrap().in_edges(), inc.into_iter().filter(|&e| survives(e)).collect::<Vec<_>>());
    }
    // Refill freed slots; stale handles and IDs must remain stale.
    let replacement = g.add_node("replacement", ()).unwrap();
    for &(ix, id, from, to, _) in &edges {
        if from == removed || to == removed {
            g.add_edge(replacement, replacement, 0).unwrap();
            assert!(g.edge(ix).is_none());
            assert!(g.edge_ix(id).is_none());
        }
    }
    assert!(g.node(removed).is_none());
    g.remove_node(replacement).unwrap();
}

#[test]
fn parallel_edges_mixed_with_survivors_keep_order_and_ids() {
    for sparse_ids in [false, true] {
        let mut g = Graph::new();
        let nodes: Vec<_> = (0..4).map(|i| g.add_node(i.to_string(), ()).unwrap()).collect();
        for i in 0..300 {
            for (from, to) in [(0, 1), (2, 1), (1, 0), (1, 3), (0, 0), (0, 2), (3, 0), (1, 1)] {
                let id = sparse_ids.then_some(EdgeId(1_000_000 + g.edge_count() as u64));
                g.insert_edge(nodes[from], nodes[to], id, Some("link"), i).unwrap();
            }
        }
        check_removal(&mut g, nodes[0]);
    }
}

#[test]
fn repeated_removal_matches_an_edge_list_reference() {
    for seed in 0..12 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut g = Graph::new();
        let mut nodes: Vec<_> = (0..12).map(|i| g.add_node(i.to_string(), ()).unwrap()).collect();
        for round in 0..12 {
            for i in 0..100 {
                let from = nodes[rng.random_range(0..nodes.len())];
                let to = nodes[rng.random_range(0..nodes.len())];
                g.add_edge(from, to, i).unwrap();
            }
            let at = rng.random_range(0..nodes.len());
            check_removal(&mut g, nodes[at]);
            nodes[at] = g.add_node(format!("round{round}"), ()).unwrap();
        }
    }
}

#[test]
fn parallel_edge_payloads_are_dropped_exactly_once() {
    struct Payload(Rc<Cell<usize>>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Rc::new(Cell::new(0));
    let mut g = Graph::new();
    let a = g.add_node("a", ()).unwrap();
    let b = g.add_node("b", ()).unwrap();
    for _ in 0..50 {
        for (from, to) in [(a, b), (b, a), (a, a), (b, b)] {
            g.add_edge(from, to, Payload(drops.clone())).unwrap();
        }
    }
    g.remove_node(a).unwrap();
    assert_eq!(drops.get(), 150);
    assert_eq!(g.edge_count(), 50);
    drop(g);
    assert_eq!(drops.get(), 200);
}

#[test]
fn batched_cleanup_preserves_drop_and_slot_reuse_order() {
    use std::cell::RefCell;
    struct Payload(usize, Rc<RefCell<Vec<usize>>>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let drops = Rc::new(RefCell::new(Vec::new()));
    let mut g = Graph::new();
    let a = g.add_node("a", ()).unwrap();
    let b = g.add_node("b", ()).unwrap();
    let c = g.add_node("c", ()).unwrap();
    for i in 0..1200 {
        let (from, to) = match i % 4 {
            0 => (a, b),
            1 => (c, a),
            2 => (a, c),
            _ => (a, a),
        };
        g.add_edge(from, to, Payload(i, drops.clone())).unwrap();
    }
    let node = g.node(a).unwrap();
    let outgoing = node.out_edges().to_vec();
    let incoming = node.in_edges().iter().copied().filter(|e| !outgoing.contains(e)).collect::<Vec<_>>();
    let order = outgoing.into_iter().chain(incoming).collect::<Vec<_>>();
    let expected = order.iter().map(|&e| g.edge(e).unwrap().data.0).collect::<Vec<_>>();
    g.remove_node(a).unwrap();
    assert_eq!(*drops.borrow(), expected);
    for old in order.into_iter().rev() {
        let new = g.add_edge(b, c, Payload(0, drops.clone())).unwrap();
        assert_eq!(new.slot(), old.slot());
        assert!(g.edge(old).is_none());
    }
}
