// algo/components.rs

use super::{groups, NONE};
use crate::Projection;

/// Weakly connected components: nodes joined by edges in either direction.
/// Groups are largest first; members in index order.
pub fn weakly_connected_components(p: &Projection) -> Vec<Vec<u32>> {
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            // Path halving
            let grand = parent[parent[x as usize] as usize];
            parent[x as usize] = grand;
            x = grand;
        }
        x
    }

    let n = p.node_count() as u32;
    let mut parent: Vec<u32> = (0..n).collect();
    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("weakly connected components", Some(n as u64));
    for u in 0..n {
        report.tick(u as usize, n as usize);
        if (u as usize).is_multiple_of(crate::cancel::TICK) && stop.requested() {
            break;
        }
        for &v in p.out_neighbors(u) {
            let (a, b) = (find(&mut parent, u), find(&mut parent, v));
            // The smaller index becomes the root: deterministic
            if a < b {
                parent[b as usize] = a;
            } else if b < a {
                parent[a as usize] = b;
            }
        }
    }
    let labels: Vec<u32> = (0..n).map(|u| find(&mut parent, u)).collect();
    groups(&labels)
}

/// Strongly connected components along the projection's edges (Tarjan's
/// algorithm, iterative). Groups are largest first; members in index order.
pub fn strongly_connected_components(p: &Projection) -> Vec<Vec<u32>> {
    struct Tarjan {
        index: Vec<u32>,
        low: Vec<u32>,
        on_stack: Vec<bool>,
        stack: Vec<u32>,
        /// (node, position of the next neighbour to look at)
        calls: Vec<(u32, usize)>,
        next_index: u32,
    }

    impl Tarjan {
        fn enter(&mut self, v: u32) {
            self.index[v as usize] = self.next_index;
            self.low[v as usize] = self.next_index;
            self.next_index += 1;
            self.stack.push(v);
            self.on_stack[v as usize] = true;
            self.calls.push((v, 0));
        }
    }

    let n = p.node_count();
    let mut t = Tarjan {
        index: vec![NONE; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        calls: Vec::new(),
        next_index: 0,
    };
    let mut component = vec![NONE; n];
    let mut components = 0u32;

    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("strongly connected components", Some(n as u64));
    for s in 0..n as u32 {
        report.tick(s as usize, n);
        if (s as usize).is_multiple_of(crate::cancel::TICK) && stop.requested() {
            return Vec::new(); // unfinished nodes have no component; `run` drops it anyway
        }
        if t.index[s as usize] != NONE {
            continue;
        }
        t.enter(s);
        while let Some(&(u, i)) = t.calls.last() {
            let neighbors = p.out_neighbors(u);
            if i < neighbors.len() {
                t.calls.last_mut().expect("not empty").1 += 1;
                let v = neighbors[i];
                if t.index[v as usize] == NONE {
                    t.enter(v);
                } else if t.on_stack[v as usize] {
                    t.low[u as usize] = t.low[u as usize].min(t.index[v as usize]);
                }
                continue;
            }
            t.calls.pop();
            if let Some(&(parent, _)) = t.calls.last() {
                t.low[parent as usize] = t.low[parent as usize].min(t.low[u as usize]);
            }
            if t.low[u as usize] == t.index[u as usize] {
                loop {
                    let w = t.stack.pop().expect("u is on the stack");
                    t.on_stack[w as usize] = false;
                    component[w as usize] = components;
                    if w == u {
                        break;
                    }
                }
                components += 1;
            }
        }
    }
    groups(&component)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::Direction;

    /// Reachability by BFS, for brute-force references.
    fn reach(p: &Projection, s: u32, undirected: bool) -> Vec<bool> {
        let mut seen = vec![false; p.node_count()];
        let mut todo = vec![s];
        seen[s as usize] = true;
        while let Some(u) = todo.pop() {
            let ins: &[u32] = if undirected { p.in_neighbors(u) } else { &[] };
            for &v in p.out_neighbors(u).iter().chain(ins) {
                if !seen[v as usize] {
                    seen[v as usize] = true;
                    todo.push(v);
                }
            }
        }
        seen
    }

    /// Reference: u and v share a component iff each reaches the other.
    fn brute(p: &Projection, undirected: bool) -> Vec<Vec<u32>> {
        let n = p.node_count() as u32;
        let r: Vec<Vec<bool>> = (0..n).map(|s| reach(p, s, undirected)).collect();
        let mut label = vec![NONE; n as usize];
        for u in 0..n {
            if label[u as usize] == NONE {
                for v in u..n {
                    if r[u as usize][v as usize] && r[v as usize][u as usize] {
                        label[v as usize] = u;
                    }
                }
            }
        }
        groups(&label)
    }

    #[test]
    fn match_brute_force() {
        for (seed, dir) in sweep() {
            let p = random(seed, 60, 30 + 6 * seed as usize, dir, false);
            assert_eq!(weakly_connected_components(&p), brute(&p, true), "wcc seed {seed} {dir:?}");
            assert_eq!(strongly_connected_components(&p), brute(&p, false), "scc seed {seed} {dir:?}");
        }
    }

    #[test]
    fn small_cases() {
        // 0 -> 1 -> 2 -> 0, 2 -> 3, 4 alone, 5 -> 5
        let p = from_edges(6, &[(0, 1), (1, 2), (2, 0), (2, 3), (5, 5)], Direction::Out);
        assert_eq!(strongly_connected_components(&p), [vec![0, 1, 2], vec![3], vec![4], vec![5]]);
        assert_eq!(weakly_connected_components(&p), [vec![0, 1, 2, 3], vec![4], vec![5]]);
        let empty = from_edges(0, &[], Direction::Out);
        assert!(strongly_connected_components(&empty).is_empty());
        // A long chain: no recursion, so no stack overflow
        let chain: Vec<(usize, usize)> = (0..200_000).map(|i| (i, i + 1)).collect();
        let p = from_edges(200_001, &chain, Direction::Out);
        assert_eq!(strongly_connected_components(&p).len(), 200_001);
        assert_eq!(weakly_connected_components(&p).len(), 1);
    }
}
