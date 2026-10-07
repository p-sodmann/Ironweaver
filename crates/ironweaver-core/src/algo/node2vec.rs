// algo/node2vec.rs
//
// Second-order biased random walks (node2vec; Grover & Leskovec, 2016) on a
// projection, to feed a word2vec-style embedding model. From node `v`,
// having arrived from `t`, the walk moves to neighbour `x` with probability
// proportional to `weight(v, x) * bias`, where bias is `1/p` if `x == t`
// (going back), 1 if `x` is a neighbour of `t` (staying close), and `1/q`
// otherwise (moving outwards). `p = q = 1` is a plain weighted random walk.
//
// Sampling uses rejection (as in KnightKing): draw `x` by edge weight,
// accept it with probability `bias / max_bias`. It needs no per-edge tables,
// so memory stays at one prefix sum per edge (weighted projections only).
// Neighbour tests are binary searches in the sorted neighbour lists.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;

use super::{check_nodes, mix};
use crate::{GraphError, Projection};

/// node2vec walk options.
#[derive(Clone, Debug)]
pub struct Node2Vec {
    /// Nodes per walk (including the start); walks end early at nodes
    /// without outgoing edges.
    pub walk_length: usize,
    pub walks_per_node: usize,
    /// Return parameter: higher values make going back less likely.
    pub p: f64,
    /// In-out parameter: higher values keep the walk local (BFS-like),
    /// lower values push it outwards (DFS-like).
    pub q: f64,
    pub seed: u64,
}

impl Default for Node2Vec {
    fn default() -> Self {
        Node2Vec { walk_length: 80, walks_per_node: 10, p: 1.0, q: 1.0, seed: 0 }
    }
}

/// Picks a neighbour index of `u` in proportion to the edge weights.
struct Picker<'a> {
    p: &'a Projection,
    /// Per weighted row: cumulative weights (absent if unweighted).
    cumulative: Option<Vec<f64>>,
    row_start: Vec<usize>,
}

impl<'a> Picker<'a> {
    fn new(p: &'a Projection) -> Self {
        let n = p.node_count() as u32;
        let mut row_start = Vec::with_capacity(n as usize + 1);
        row_start.push(0);
        for u in 0..n {
            row_start.push(row_start[u as usize] + p.out_degree(u));
        }
        let cumulative = p.is_weighted().then(|| {
            let mut c = Vec::with_capacity(row_start[n as usize]);
            for u in 0..n {
                let mut sum = 0.0;
                for &w in p.out_weights(u).unwrap_or(&[]) {
                    sum += w;
                    c.push(sum);
                }
            }
            c
        });
        Picker { p, cumulative, row_start }
    }

    /// A neighbour of `u` (which has at least one) drawn by weight; `None`
    /// if all its edges weigh 0.
    fn pick(&self, u: u32, rng: &mut StdRng) -> Option<u32> {
        let row = self.p.out_neighbors(u);
        let i = match &self.cumulative {
            None => rng.random_range(0..row.len()),
            Some(c) => {
                let c = &c[self.row_start[u as usize]..self.row_start[u as usize + 1]];
                let total = *c.last()?;
                if total <= 0.0 {
                    return None;
                }
                let r = rng.random::<f64>() * total;
                c.partition_point(|&x| x <= r).min(c.len() - 1)
            }
        };
        Some(row[i])
    }
}

fn walk(p: &Projection, picker: &Picker, start: u32, opts: &Node2Vec, rng: &mut StdRng) -> Vec<u32> {
    let (back, out) = (1.0 / opts.p, 1.0 / opts.q);
    let max_bias = back.max(out).max(1.0);
    let mut path = Vec::with_capacity(opts.walk_length);
    path.push(start);
    while path.len() < opts.walk_length {
        let v = *path.last().expect("the walk has its start");
        if p.out_degree(v) == 0 {
            break;
        }
        let next = if path.len() == 1 {
            picker.pick(v, rng)
        } else {
            let t = path[path.len() - 2];
            let mut chosen = None;
            // Expected tries are at most max_bias / min_bias; the cap only
            // guards against pathological parameters
            for _ in 0..10_000 {
                let Some(x) = picker.pick(v, rng) else { break };
                let bias = if x == t {
                    back
                } else if p.out_neighbors(t).binary_search(&x).is_ok() {
                    1.0
                } else {
                    out
                };
                if rng.random::<f64>() * max_bias < bias {
                    chosen = Some(x);
                    break;
                }
            }
            chosen
        };
        match next {
            Some(x) => path.push(x),
            None => break,
        }
    }
    path
}

/// `walks_per_node` node2vec walks from each of `starts` (every node if
/// `None`): all first walks (in `starts` order), then all second walks,
/// and so on. Parallel; deterministic for a seed, whatever the number of
/// threads.
pub fn node2vec_walks(p: &Projection, starts: Option<&[u32]>, opts: &Node2Vec) -> Result<Vec<Vec<u32>>, GraphError> {
    if opts.walk_length == 0 {
        return Err(GraphError::InvalidArgument("walk_length must be at least 1".into()));
    }
    for (name, x) in [("p", opts.p), ("q", opts.q)] {
        if !(x > 0.0 && x.is_finite()) {
            return Err(GraphError::InvalidArgument(format!("{} must be a positive number, got {}", name, x)));
        }
    }
    let all: Vec<u32>;
    let starts = match starts {
        Some(s) => {
            check_nodes(p, s)?;
            s
        }
        None => {
            all = (0..p.node_count() as u32).collect();
            &all
        }
    };
    let picker = Picker::new(p);
    let total = starts.len() * opts.walks_per_node;
    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("node2vec", Some(total as u64));
    Ok((0..total)
        .into_par_iter()
        .with_min_len(64)
        .map(|i| {
            report.tick(i, total);
            if stop.requested() {
                return Vec::new();
            }
            let mut rng = StdRng::seed_from_u64(mix(opts.seed, i as u64));
            walk(p, &picker, starts[i % starts.len()], opts, &mut rng)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random};
    use crate::Direction;

    /// Transition frequencies from `v` having come from `t`, over many walks.
    fn second_steps(p: &Projection, opts: &Node2Vec, t: u32, v: u32) -> Vec<(u32, f64)> {
        let walks = node2vec_walks(p, Some(&[t]), opts).unwrap();
        let mut counts = std::collections::BTreeMap::new();
        let mut total = 0.0;
        for w in &walks {
            if w.len() >= 3 && w[1] == v {
                *counts.entry(w[2]).or_insert(0.0) += 1.0;
                total += 1.0;
            }
        }
        counts.into_iter().map(|(x, c)| (x, c / total)).collect()
    }

    #[test]
    fn biases_match_the_formula() {
        // 0 - 1, 1 - 2, 1 - 3, 0 - 2 (undirected); from 0 to 1, then:
        // back to 0 (1/p), to 2 (a neighbour of 0: 1), to 3 (1/q)
        let p = from_edges(4, &[(0, 1), (1, 2), (1, 3), (0, 2)], Direction::Both);
        let opts = Node2Vec { walk_length: 3, walks_per_node: 60_000, p: 0.5, q: 4.0, seed: 1 };
        let got = second_steps(&p, &opts, 0, 1);
        let raw = [(0, 2.0), (2, 1.0), (3, 0.25)];
        let sum: f64 = raw.iter().map(|x| x.1).sum();
        for ((x, f), (y, r)) in got.iter().zip(raw) {
            assert_eq!(*x, y);
            assert!((f - r / sum).abs() < 0.01, "{got:?}");
        }
    }

    #[test]
    fn weighted_first_step() {
        let p = random(1, 6, 0, Direction::Out, true);
        assert_eq!(p.edge_count(), 0);
        let walks = node2vec_walks(&p, None, &Node2Vec { walks_per_node: 2, ..Default::default() }).unwrap();
        assert_eq!(walks.len(), 12);
        assert!(walks.iter().all(|w| w.len() == 1));
        let p = random(2, 20, 80, Direction::Out, true);
        let opts = Node2Vec { walk_length: 10, walks_per_node: 3, p: 2.0, q: 0.5, seed: 9 };
        let walks = node2vec_walks(&p, None, &opts).unwrap();
        assert_eq!(walks.len(), 60);
        for (i, w) in walks.iter().enumerate() {
            assert_eq!(w[0] as usize, i % 20);
            for pair in w.windows(2) {
                assert!(p.out_neighbors(pair[0]).contains(&pair[1]));
            }
        }
        assert_eq!(walks, node2vec_walks(&p, None, &opts).unwrap());
        assert!(node2vec_walks(&p, None, &Node2Vec { q: 0.0, ..Default::default() }).is_err());
        assert!(node2vec_walks(&p, Some(&[99]), &Node2Vec::default()).is_err());
    }
}
