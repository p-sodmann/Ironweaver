// algo/mod.rs
//
// Graph analytics on a [`Projection`]. Every algorithm takes `&Projection`
// and works with dense node indices (`0..p.node_count()`); results are
// `Vec`s indexed by node, or groups of nodes. None touches the graph, so the
// bindings run them with the GIL released, and several run in parallel
// (rayon).
//
// Directed algorithms (strongly connected components, topological order,
// cycles, PageRank, BFS) follow the projection's edges (`out_neighbors`).
// Undirected ones (triangles, clustering, k-core, label propagation) use
// `Undirected`: the simple graph with an edge between two distinct nodes
// joined by at least one projected edge, in either direction (like
// networkx's `nx.Graph(G)` without self-loops).
//
// Adding an algorithm: a function here taking `&Projection` (plus options),
// validating options into a `GraphError`, tests against a brute-force
// reference in the same file, then a method on the Python `Projection`
// (src/projection.rs), stubs, docs and a networkx comparison test.

pub mod bfs;
pub mod centrality;
pub mod community;
pub mod components;
pub mod dag;
pub mod structure;

pub use bfs::bfs_levels;
pub use centrality::{degree_centrality, pagerank, PageRank};
pub use community::label_propagation;
pub use components::{strongly_connected_components, weakly_connected_components};
pub use dag::{find_cycle, topological_sort};
pub use structure::{clustering, core_number, triangles};

use rayon::prelude::*;

use crate::{Direction, GraphError, Projection};

/// Marks "no node" / "unreached" in `u32` arrays.
pub const NONE: u32 = u32::MAX;

/// The simple undirected graph underlying a projection: for each node, the
/// sorted distinct nodes joined to it by an edge in either direction,
/// excluding itself.
pub struct Undirected {
    start: Vec<usize>,
    to: Vec<u32>,
}

impl Undirected {
    pub fn of(p: &Projection) -> Self {
        let both = p.direction() == Direction::Both;
        let rows: Vec<Vec<u32>> = (0..p.node_count() as u32)
            .into_par_iter()
            .with_min_len(256)
            .map(|u| merge_distinct(p.out_neighbors(u), if both { &[] } else { p.in_neighbors(u) }, u))
            .collect();
        let mut start = Vec::with_capacity(rows.len() + 1);
        start.push(0);
        let mut to = Vec::with_capacity(rows.iter().map(Vec::len).sum());
        for row in rows {
            to.extend_from_slice(&row);
            start.push(to.len());
        }
        Undirected { start, to }
    }

    pub fn len(&self) -> usize {
        self.start.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn neighbors(&self, u: u32) -> &[u32] {
        &self.to[self.start[u as usize]..self.start[u as usize + 1]]
    }

    pub fn degree(&self, u: u32) -> usize {
        self.start[u as usize + 1] - self.start[u as usize]
    }
}

/// Sorted union of two sorted lists, without duplicates or `skip`.
fn merge_distinct(a: &[u32], b: &[u32], skip: u32) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let v = if j == b.len() || (i < a.len() && a[i] <= b[j]) {
            i += 1;
            a[i - 1]
        } else {
            j += 1;
            b[j - 1]
        };
        if v != skip && out.last() != Some(&v) {
            out.push(v);
        }
    }
    out
}

/// Number of common elements of two sorted, distinct lists.
fn intersection_size(a: &[u32], b: &[u32]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
        }
    }
    n
}

/// Nodes grouped by label: members in index order, groups largest first
/// (ties: the group with the smallest member first).
pub fn groups(labels: &[u32]) -> Vec<Vec<u32>> {
    let mut slot = vec![NONE; labels.len()];
    let mut out: Vec<Vec<u32>> = Vec::new();
    for (u, &label) in labels.iter().enumerate() {
        let s = &mut slot[label as usize];
        if *s == NONE {
            *s = out.len() as u32;
            out.push(Vec::new());
        }
        out[*s as usize].push(u as u32);
    }
    // Stable: equal sizes keep first-member order
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

/// Fail for node indices outside the projection.
fn check_nodes(p: &Projection, nodes: &[u32]) -> Result<(), GraphError> {
    match nodes.iter().find(|&&u| u as usize >= p.node_count()) {
        Some(u) => Err(GraphError::InvalidArgument(format!("node index {} out of range", u))),
        None => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Deterministic pseudo-random projections for the algorithm tests.
    use crate::pathfinding::EdgeCost;
    use crate::{Direction, Graph, GraphError, Projection, Record, Value};

    pub struct Lcg(u64);

    impl Lcg {
        pub fn new(seed: u64) -> Self {
            Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
        }

        pub fn below(&mut self, m: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) % m
        }
    }

    /// A random multigraph (self-loops and parallel edges included) with
    /// weights in 0.5..5.
    pub fn random(seed: u64, n: usize, m: usize, direction: Direction, weighted: bool) -> Projection {
        let mut rng = Lcg::new(seed);
        let mut g: Graph<Record, Record> = Graph::new();
        let ix: Vec<_> = (0..n).map(|i| g.add_node(format!("n{i}"), Record::default()).unwrap()).collect();
        for _ in 0..m {
            let (a, b) = (rng.below(n as u64) as usize, rng.below(n as u64) as usize);
            let w = 0.5 + rng.below(90) as f64 / 20.0;
            g.add_edge(ix[a], ix[b], Record::with_attr([("weight", Value::from(w))])).unwrap();
        }
        let cost = if weighted { EdgeCost::weighted(None, None) } else { EdgeCost::Unit };
        Projection::build::<_, _, GraphError>(&g, direction, &cost).unwrap()
    }

    /// A projection with the given edges between nodes `0..n`.
    pub fn from_edges(n: usize, edges: &[(usize, usize)], direction: Direction) -> Projection {
        let mut g: Graph<Record, Record> = Graph::new();
        let ix: Vec<_> = (0..n).map(|i| g.add_node(format!("n{i}"), Record::default()).unwrap()).collect();
        for &(a, b) in edges {
            g.add_edge(ix[a], ix[b], Record::default()).unwrap();
        }
        Projection::build::<_, _, GraphError>(&g, direction, &EdgeCost::Unit).unwrap()
    }

    /// Every (graph, direction) combination the tests sweep.
    pub fn sweep() -> impl Iterator<Item = (u64, Direction)> {
        (0..12u64).flat_map(|seed| [Direction::Out, Direction::In, Direction::Both].map(move |d| (seed, d)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(merge_distinct(&[1, 3, 3, 5], &[0, 3, 4, 5], 4), [0, 1, 3, 5]);
        assert_eq!(intersection_size(&[1, 2, 5, 9], &[2, 3, 5, 10]), 2);
        assert_eq!(groups(&[3, 1, 3, 1, 4, 3]), [vec![0, 2, 5], vec![1, 3], vec![4]]);
        let p = testing::from_edges(4, &[(0, 1), (1, 0), (1, 1), (2, 1)], Direction::Out);
        let u = Undirected::of(&p);
        assert_eq!((u.neighbors(1), u.degree(3)), (&[0, 2][..], 0));
    }
}
