// algo/sssp.rs
//
// Single-source shortest distances on a projection, reused across many
// sources (betweenness, closeness, harmonic centrality): a workspace with
// per-node arrays that is reset through the list of nodes a search touched.
// Searches see the simple graph: parallel edges collapse to the lightest,
// self-loops are skipped (like networkx on a multigraph).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::Projection;

/// The distinct neighbours of a node with the smallest weight among
/// parallel edges (1 if unweighted), skipping `skip` (the node itself).
pub(crate) struct SimpleRow<'a> {
    to: &'a [u32],
    weight: Option<&'a [f64]>,
    skip: u32,
    i: usize,
}

impl<'a> SimpleRow<'a> {
    /// Along the projection's edges (`reverse = false`) or against them.
    pub(crate) fn of(p: &'a Projection, u: u32, reverse: bool, weighted: bool) -> Self {
        let (to, weight) =
            if reverse { (p.in_neighbors(u), p.in_weights(u)) } else { (p.out_neighbors(u), p.out_weights(u)) };
        SimpleRow { to, weight: if weighted { weight } else { None }, skip: u, i: 0 }
    }
}

impl Iterator for SimpleRow<'_> {
    type Item = (u32, f64);

    fn next(&mut self) -> Option<(u32, f64)> {
        loop {
            let v = *self.to.get(self.i)?;
            let mut w = self.weight.map_or(1.0, |w| w[self.i]);
            self.i += 1;
            // Parallel edges are adjacent (rows are sorted)
            while self.to.get(self.i) == Some(&v) {
                if let Some(ws) = self.weight {
                    w = w.min(ws[self.i]);
                }
                self.i += 1;
            }
            if v != self.skip {
                return Some((v, w));
            }
        }
    }
}

/// Reusable single-source search state.
pub(crate) struct Search {
    pub dist: Vec<f64>,
    /// Nodes settled, in order of non-decreasing distance (the source first).
    pub order: Vec<u32>,
    done: Vec<bool>,
    touched: Vec<u32>,
    heap: BinaryHeap<(Reverse<u64>, u32)>,
    queue: VecDeque<u32>,
}

impl Search {
    pub fn new(n: usize) -> Self {
        Search {
            dist: vec![f64::INFINITY; n],
            order: Vec::new(),
            done: vec![false; n],
            touched: Vec::new(),
            heap: BinaryHeap::new(),
            queue: VecDeque::new(),
        }
    }

    fn reset(&mut self) {
        for &u in &self.touched {
            self.dist[u as usize] = f64::INFINITY;
            self.done[u as usize] = false;
        }
        self.touched.clear();
        self.order.clear();
        self.heap.clear();
        self.queue.clear();
    }

    /// Distances from `source` to every node it reaches, following edges
    /// forwards (or backwards if `reverse`): BFS hop counts, or Dijkstra over
    /// the weights if `weighted` (the projection must have weights). Clears
    /// the previous search first.
    pub fn run(&mut self, p: &Projection, source: u32, weighted: bool, reverse: bool) {
        self.reset();
        self.dist[source as usize] = 0.0;
        self.touched.push(source);
        if !weighted {
            self.queue.push_back(source);
            while let Some(u) = self.queue.pop_front() {
                self.order.push(u);
                let next = self.dist[u as usize] + 1.0;
                for (v, _) in SimpleRow::of(p, u, reverse, false) {
                    if self.dist[v as usize].is_infinite() {
                        self.dist[v as usize] = next;
                        self.touched.push(v);
                        self.queue.push_back(v);
                    }
                }
            }
            return;
        }
        // Costs are non-negative, so their bit patterns order like the values
        self.heap.push((Reverse(0f64.to_bits()), source));
        while let Some((Reverse(bits), u)) = self.heap.pop() {
            if self.done[u as usize] || f64::from_bits(bits) > self.dist[u as usize] {
                continue;
            }
            self.done[u as usize] = true;
            self.order.push(u);
            let du = self.dist[u as usize];
            for (v, w) in SimpleRow::of(p, u, reverse, true) {
                let nd = du + w;
                if nd < self.dist[v as usize] {
                    if self.dist[v as usize].is_infinite() {
                        self.touched.push(v);
                    }
                    self.dist[v as usize] = nd;
                    self.heap.push((Reverse(nd.to_bits()), v));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::from_edges;
    use crate::Direction;

    #[test]
    fn simple_rows_and_search() {
        let p = from_edges(4, &[(0, 1), (0, 1), (0, 0), (1, 2), (3, 2)], Direction::Out);
        assert_eq!(SimpleRow::of(&p, 0, false, false).collect::<Vec<_>>(), [(1, 1.0)]);
        assert_eq!(SimpleRow::of(&p, 2, true, false).collect::<Vec<_>>(), [(1, 1.0), (3, 1.0)]);
        let mut s = Search::new(4);
        s.run(&p, 0, false, false);
        assert_eq!((s.order.as_slice(), s.dist[2], s.dist[3]), (&[0, 1, 2][..], 2.0, f64::INFINITY));
        s.run(&p, 2, false, true);
        assert_eq!((s.order.len(), s.dist[0]), (4, 2.0));
    }
}
