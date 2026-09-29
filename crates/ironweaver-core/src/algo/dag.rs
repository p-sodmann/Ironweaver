// algo/dag.rs

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::{GraphError, Projection};

/// Nodes ordered so that every projected edge goes from an earlier to a
/// later node (Kahn's algorithm; among the nodes ready at each step the
/// lowest index comes first, so the order is deterministic). Fails if the
/// projection has a cycle (a self-loop counts); [`find_cycle`] shows one.
pub fn topological_sort(p: &Projection) -> Result<Vec<u32>, GraphError> {
    let n = p.node_count();
    let mut indegree = vec![0u32; n];
    for u in 0..n as u32 {
        for &v in p.out_neighbors(u) {
            indegree[v as usize] += 1;
        }
    }
    let mut ready: BinaryHeap<Reverse<u32>> =
        (0..n as u32).filter(|&u| indegree[u as usize] == 0).map(Reverse).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(Reverse(u)) = ready.pop() {
        order.push(u);
        for &v in p.out_neighbors(u) {
            indegree[v as usize] -= 1;
            if indegree[v as usize] == 0 {
                ready.push(Reverse(v));
            }
        }
    }
    if order.len() < n {
        return Err(GraphError::InvalidArgument(
            "the graph has a cycle, so it has no topological order (find_cycle shows one)".into(),
        ));
    }
    Ok(order)
}

/// One cycle along the projection's edges, as the nodes in order (the last
/// has an edge back to the first; a self-loop gives one node), or `None` if
/// there is none. Depth-first from the lowest index, iterative.
pub fn find_cycle(p: &Projection) -> Option<Vec<u32>> {
    const NEW: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let n = p.node_count();
    let mut state = vec![NEW; n];
    // The current DFS path: (node, position of the next neighbour)
    let mut path: Vec<(u32, usize)> = Vec::new();
    for s in 0..n as u32 {
        if state[s as usize] != NEW {
            continue;
        }
        state[s as usize] = OPEN;
        path.push((s, 0));
        while let Some(&(u, i)) = path.last() {
            let neighbors = p.out_neighbors(u);
            if i == neighbors.len() {
                state[u as usize] = DONE;
                path.pop();
                continue;
            }
            path.last_mut().expect("not empty").1 += 1;
            let v = neighbors[i];
            match state[v as usize] {
                NEW => {
                    state[v as usize] = OPEN;
                    path.push((v, 0));
                }
                OPEN => {
                    let at = path.iter().position(|&(w, _)| w == v).expect("open nodes are on the path");
                    return Some(path[at..].iter().map(|&(w, _)| w).collect());
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep, Lcg};
    use crate::Direction;

    fn is_cycle(p: &Projection, c: &[u32]) -> bool {
        !c.is_empty() && (0..c.len()).all(|i| p.out_neighbors(c[i]).binary_search(&c[(i + 1) % c.len()]).is_ok()) && {
            let mut d = c.to_vec();
            d.sort_unstable();
            d.dedup();
            d.len() == c.len()
        }
    }

    #[test]
    fn random_dags_and_cyclic_graphs() {
        for (seed, dir) in sweep() {
            // Edges only from lower to higher index: a DAG (reversed by `In`)
            let mut rng = Lcg::new(seed);
            let edges: Vec<(usize, usize)> = (0..150)
                .map(|_| {
                    let (a, b) = (rng.below(50) as usize, rng.below(50) as usize);
                    (a.min(b), a.max(b))
                })
                .filter(|(a, b)| a != b)
                .collect();
            let dag = from_edges(50, &edges, dir);
            if dir == Direction::Both {
                assert_eq!(find_cycle(&dag).is_some(), !edges.is_empty());
            } else {
                let order = topological_sort(&dag).unwrap();
                let mut pos = vec![0; 50];
                for (i, &u) in order.iter().enumerate() {
                    pos[u as usize] = i;
                }
                for u in 0..50u32 {
                    assert!(dag.out_neighbors(u).iter().all(|&v| pos[u as usize] < pos[v as usize]));
                }
                assert_eq!(find_cycle(&dag), None);
            }

            let p = random(seed, 40, 60, dir, false);
            match find_cycle(&p) {
                Some(c) => {
                    assert!(is_cycle(&p, &c), "{c:?}");
                    assert!(topological_sort(&p).is_err());
                }
                None => assert_eq!(topological_sort(&p).unwrap().len(), 40),
            }
        }
    }

    #[test]
    fn small_cases() {
        let p = from_edges(4, &[(2, 0), (3, 1), (0, 1)], Direction::Out);
        assert_eq!(topological_sort(&p).unwrap(), [2, 0, 3, 1]);
        let loop1 = from_edges(3, &[(0, 1), (2, 2)], Direction::Out);
        assert_eq!(find_cycle(&loop1), Some(vec![2]));
        let tri = from_edges(4, &[(3, 0), (0, 1), (1, 2), (2, 0)], Direction::Out);
        assert_eq!(find_cycle(&tri), Some(vec![0, 1, 2]));
        let chain: Vec<(usize, usize)> = (0..100_000).map(|i| (i, i + 1)).collect();
        let long = from_edges(100_001, &chain, Direction::Out);
        assert_eq!(find_cycle(&long), None);
    }
}
