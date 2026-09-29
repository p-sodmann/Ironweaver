// algo/ksp.rs
//
// The k shortest loopless paths between two nodes (Yen, 1971): after the
// shortest path, each next path deviates from an earlier one at some "spur"
// node, found by a search that may not reuse the earlier path's prefix nodes
// nor the edges earlier paths took from the same prefix. Parallel edges
// count as one edge with the lightest weight.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashSet, VecDeque};

use super::check_nodes;
use crate::batch::DensePath;
use crate::{GraphError, Projection};

/// Cost of the edge `u -> v`: the lightest parallel edge (1 if unweighted).
fn edge_cost(p: &Projection, u: u32, v: u32, weighted: bool) -> f64 {
    let row = p.out_neighbors(u);
    let first = row.partition_point(|&x| x < v);
    let last = row.partition_point(|&x| x <= v);
    match p.out_weights(u).filter(|_| weighted) {
        Some(w) => w[first..last].iter().copied().fold(f64::INFINITY, f64::min),
        None => 1.0,
    }
}

/// Searches with some nodes and edges banned.
struct Spur {
    dist: Vec<f64>,
    parent: Vec<u32>,
    banned: Vec<bool>,
    touched: Vec<u32>,
}

impl Spur {
    fn new(n: usize) -> Self {
        Spur { dist: vec![f64::INFINITY; n], parent: vec![u32::MAX; n], banned: vec![false; n], touched: Vec::new() }
    }

    /// Shortest path from `s` to `t` avoiding banned nodes and edges, with
    /// the number of nodes settled.
    fn search(
        &mut self,
        p: &Projection,
        s: u32,
        t: u32,
        weighted: bool,
        banned_edges: &HashSet<(u32, u32)>,
    ) -> Option<DensePath> {
        for &u in &self.touched {
            self.dist[u as usize] = f64::INFINITY;
            self.parent[u as usize] = u32::MAX;
        }
        self.touched.clear();
        self.dist[s as usize] = 0.0;
        self.touched.push(s);
        let mut settled = 0;
        let mut heap = BinaryHeap::new();
        let mut queue = VecDeque::new();
        if weighted {
            heap.push((Reverse(0f64.to_bits()), s));
        } else {
            queue.push_back(s);
        }
        loop {
            let u = if weighted {
                let Some((Reverse(bits), u)) = heap.pop() else { break };
                if f64::from_bits(bits) > self.dist[u as usize] {
                    continue;
                }
                u
            } else {
                let Some(u) = queue.pop_front() else { break };
                u
            };
            settled += 1;
            if u == t {
                break;
            }
            let du = self.dist[u as usize];
            let weights = p.out_weights(u).filter(|_| weighted);
            for (i, &v) in p.out_neighbors(u).iter().enumerate() {
                if v == u || self.banned[v as usize] || banned_edges.contains(&(u, v)) {
                    continue;
                }
                let nd = du + weights.map_or(1.0, |w| w[i]);
                if nd < self.dist[v as usize] {
                    if self.dist[v as usize].is_infinite() {
                        self.touched.push(v);
                    }
                    self.dist[v as usize] = nd;
                    self.parent[v as usize] = u;
                    if weighted {
                        heap.push((Reverse(nd.to_bits()), v));
                    } else {
                        queue.push_back(v);
                    }
                }
            }
        }
        if self.dist[t as usize].is_infinite() {
            return None;
        }
        let mut nodes = vec![t];
        while *nodes.last().expect("non-empty") != s {
            nodes.push(self.parent[*nodes.last().expect("non-empty") as usize]);
        }
        nodes.reverse();
        Some(DensePath { nodes, cost: self.dist[t as usize], settled })
    }
}

/// Up to `k` shortest loopless paths from `source` to `target`, cheapest
/// first (ties: by the node sequence), by Yen's algorithm: Dijkstra over
/// the projection's weights if `weighted`, BFS otherwise. Each path's
/// `settled` counts the nodes settled by the search that found it.
pub fn k_shortest_paths(
    p: &Projection,
    source: u32,
    target: u32,
    k: usize,
    weighted: bool,
) -> Result<Vec<DensePath>, GraphError> {
    check_nodes(p, &[source, target])?;
    if weighted && !p.is_weighted() {
        return Err(GraphError::InvalidArgument(
            "weighted queries need a projection with edge weights (project with weight=...)".into(),
        ));
    }
    let mut spur = Spur::new(p.node_count());
    let mut found: Vec<DensePath> = Vec::new();
    if k == 0 {
        return Ok(found);
    }
    match spur.search(p, source, target, weighted, &HashSet::new()) {
        Some(path) => found.push(path),
        None => return Ok(found),
    }
    // Candidates by (cost, nodes): ordered and deduplicated
    let mut candidates: BTreeSet<(u64, Vec<u32>, usize)> = BTreeSet::new();
    let mut banned_edges = HashSet::new();
    while found.len() < k {
        let last = found.last().expect("at least one path").nodes.clone();
        let mut root_cost = 0.0;
        for j in 0..last.len() - 1 {
            let root = &last[..=j];
            banned_edges.clear();
            for path in &found {
                if path.nodes.len() > j + 1 && path.nodes[..=j] == *root {
                    banned_edges.insert((path.nodes[j], path.nodes[j + 1]));
                }
            }
            for &u in &root[..j] {
                spur.banned[u as usize] = true;
            }
            if let Some(tail) = spur.search(p, last[j], target, weighted, &banned_edges) {
                let mut nodes = root[..j].to_vec();
                nodes.extend_from_slice(&tail.nodes);
                let cost = root_cost + tail.cost;
                candidates.insert((cost.to_bits(), nodes, tail.settled));
            }
            for &u in &root[..j] {
                spur.banned[u as usize] = false;
            }
            root_cost += edge_cost(p, last[j], last[j + 1], weighted);
        }
        // Drop candidates already found (they can be regenerated)
        let next = loop {
            let Some(c) = candidates.pop_first() else { break None };
            if !found.iter().any(|f| f.nodes == c.1) {
                break Some(c);
            }
        };
        match next {
            Some((bits, nodes, settled)) => found.push(DensePath { nodes, cost: f64::from_bits(bits), settled }),
            None => break,
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};

    /// Every simple path from s to t by DFS, with costs, sorted.
    fn all_simple(p: &Projection, s: u32, t: u32, weighted: bool) -> Vec<(f64, Vec<u32>)> {
        fn go(p: &Projection, path: &mut Vec<u32>, t: u32, w: bool, out: &mut Vec<(f64, Vec<u32>)>) {
            let u = *path.last().unwrap();
            if u == t {
                let cost = path.windows(2).map(|e| edge_cost(p, e[0], e[1], w)).sum();
                out.push((cost, path.clone()));
                return;
            }
            let mut next: Vec<u32> = p.out_neighbors(u).to_vec();
            next.dedup();
            for v in next {
                if !path.contains(&v) {
                    path.push(v);
                    go(p, path, t, w, out);
                    path.pop();
                }
            }
        }
        let mut out = Vec::new();
        go(p, &mut vec![s], t, weighted, &mut out);
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out
    }

    #[test]
    fn matches_enumeration() {
        for (seed, dir) in sweep() {
            for weighted in [false, true] {
                let p = random(seed, 9, 20, dir, weighted);
                for (s, t) in [(0, 1), (2, 7), (3, 3)] {
                    let want = all_simple(&p, s, t, weighted);
                    let got = k_shortest_paths(&p, s, t, 12, weighted).unwrap();
                    assert_eq!(got.len(), want.len().min(12), "seed {seed} {dir:?} {s}->{t}");
                    for (g, w) in got.iter().zip(&want) {
                        // Costs agree; paths agree unless costs tie
                        assert!((g.cost - w.0).abs() < 1e-9, "{:?} vs {:?}", got, want);
                        assert!(want.iter().any(|x| x.1 == g.nodes));
                    }
                    let mut seen = HashSet::new();
                    assert!(got.iter().all(|g| seen.insert(g.nodes.clone())));
                }
            }
        }
    }

    #[test]
    fn edge_cases() {
        let p = from_edges(3, &[(0, 1), (0, 1), (1, 2)], crate::Direction::Out);
        let got = k_shortest_paths(&p, 0, 2, 5, false).unwrap();
        assert_eq!(got.len(), 1); // parallel edges give one path
        assert_eq!((got[0].nodes.as_slice(), got[0].cost), (&[0, 1, 2][..], 2.0));
        assert!(k_shortest_paths(&p, 2, 0, 5, false).unwrap().is_empty());
        assert!(k_shortest_paths(&p, 0, 2, 0, false).unwrap().is_empty());
        assert!(k_shortest_paths(&p, 0, 2, 1, true).is_err());
        assert!(k_shortest_paths(&p, 0, 9, 1, false).is_err());
    }
}
