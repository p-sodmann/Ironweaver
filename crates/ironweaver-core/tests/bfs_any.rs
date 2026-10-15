use std::collections::{HashMap, VecDeque};

use ironweaver_core::traversal::{
    bidirectional_bfs, bidirectional_bfs_any, bidirectional_bfs_any_limited, bidirectional_bfs_limited,
};
use ironweaver_core::{cancel, Budget, Direction, Edge, EdgeIx, Graph, GraphError};

fn all(_: EdgeIx, _: &Edge<bool>) -> Result<bool, GraphError> {
    Ok(true)
}

#[test]
fn any_paths_match_one_sided_bfs_on_multigraphs() {
    let mut seed = 123_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed
    };
    for _ in 0..12 {
        let mut g = Graph::new();
        let mut nodes: Vec<_> = (0..24).map(|i| g.add_node(i.to_string(), ()).unwrap()).collect();
        g.remove_node(nodes[3]).unwrap();
        nodes[3] = g.add_node("replacement", ()).unwrap();
        for _ in 0..100 {
            let a = nodes[(random() >> 32) as usize % nodes.len()];
            let b = nodes[(random() >> 32) as usize % nodes.len()];
            g.add_edge(a, b, random() & 4 != 0).unwrap();
        }
        for direction in [Direction::Out, Direction::In, Direction::Both] {
            for &source in &nodes {
                let mut distance = HashMap::from([(source, 0)]);
                let mut queue = VecDeque::from([source]);
                while let Some(u) = queue.pop_front() {
                    for (e, v) in g.neighbors(u, direction) {
                        if g.edge(e).unwrap().data && !distance.contains_key(&v) {
                            distance.insert(v, distance[&u] + 1);
                            queue.push_back(v);
                        }
                    }
                }
                for &target in &nodes {
                    let allow = |_: EdgeIx, e: &Edge<bool>| Ok::<_, GraphError>(e.data);
                    for limit in [None, Some(0), Some(2), Some(5)] {
                        let path = bidirectional_bfs_any(&g, source, target, limit, direction, allow).unwrap();
                        let expected = distance.get(&target).copied().filter(|d| limit.is_none_or(|max| *d <= max));
                        assert_eq!(path.as_ref().map(|p| p.len() - 1), expected);
                        if let Some(path) = path {
                            assert_eq!(path.first(), Some(&source));
                            assert_eq!(path.last(), Some(&target));
                            for pair in path.windows(2) {
                                assert!(g
                                    .neighbors(pair[0], direction)
                                    .any(|(e, v)| v == pair[1] && g.edge(e).unwrap().data));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn skips_canonical_work_and_preserves_existing_default() {
    let mut g = Graph::new();
    let n: Vec<_> = ["s", "low", "high", "t"].map(|id| g.add_node(id, ()).unwrap()).into();
    for (a, b) in [(0, 2), (0, 1), (2, 3), (1, 3)] {
        g.add_edge(n[a], n[b], true).unwrap();
    }
    let canonical = bidirectional_bfs_limited(&g, n[0], n[3], None, Direction::Out, Budget::UNLIMITED, all).unwrap();
    let any = bidirectional_bfs_any_limited(&g, n[0], n[3], None, Direction::Out, Budget::UNLIMITED, all).unwrap();
    assert_eq!(canonical.value, Some(vec![n[0], n[1], n[3]]));
    assert_eq!(any.value, Some(vec![n[0], n[2], n[3]]));
    assert!(any.edges < canonical.edges);
    let budget = Budget::UNLIMITED.max_edges(any.edges).truncate();
    assert!(!bidirectional_bfs_any_limited(&g, n[0], n[3], None, Direction::Out, budget, all).unwrap().truncated);
    assert!(bidirectional_bfs_limited(&g, n[0], n[3], None, Direction::Out, budget, all).unwrap().truncated);
    assert_eq!(bidirectional_bfs(&g, n[0], n[3], None, Direction::Out, all).unwrap(), canonical.value);
    for budget in
        [Budget::UNLIMITED.max_edges(any.edges - 1), Budget::UNLIMITED.max_visited(0), Budget::UNLIMITED.max_results(0)]
    {
        assert!(bidirectional_bfs_any_limited(&g, n[0], n[3], None, Direction::Out, budget, all).is_err());
        let result =
            bidirectional_bfs_any_limited(&g, n[0], n[3], None, Direction::Out, budget.truncate(), all).unwrap();
        assert!(result.truncated);
        assert!(result.value.is_none());
    }
}

#[test]
fn reconstruction_errors_and_cancellation_propagate() {
    let mut g = Graph::new();
    let n: Vec<_> = (0..4).map(|i| g.add_node(i.to_string(), ()).unwrap()).collect();
    for pair in n.windows(2) {
        g.add_edge(pair[0], pair[1], true).unwrap();
    }
    let mut calls = 0;
    let result = bidirectional_bfs_any(&g, n[0], n[3], None, Direction::Out, |_, _| {
        calls += 1;
        if calls > 3 {
            Err("reconstruction failed")
        } else {
            Ok(true)
        }
    });
    assert_eq!(result, Err("reconstruction failed"));
    let token = cancel::Token::new();
    let mut calls = 0;
    let result = cancel::run(&token, || {
        bidirectional_bfs_any(&g, n[0], n[3], None, Direction::Out, |_, _| {
            calls += 1;
            if calls == 3 {
                token.cancel();
            }
            Ok::<_, GraphError>(true)
        })
    });
    assert!(matches!(result, Err(GraphError::Interrupted)));
}

#[test]
fn long_path_reconstruction_is_iterative() {
    let mut g = Graph::new();
    let n: Vec<_> = (0..20_000).map(|i| g.add_node(i.to_string(), ()).unwrap()).collect();
    for pair in n.windows(2) {
        g.add_edge(pair[0], pair[1], true).unwrap();
    }
    assert_eq!(bidirectional_bfs_any(&g, n[0], n[n.len() - 1], None, Direction::Out, all).unwrap(), Some(n));
}
