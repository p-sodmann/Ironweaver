// batch.rs
//
// Many shortest-path queries at once, in parallel, on a [`Projection`]. The
// projection owns plain data and no payloads, so the queries run on all
// cores without touching the graph (the Python bindings release the GIL).
// Each worker thread reuses one set of per-node arrays across its queries
// and resets only the entries a query touched. Nodes are the projection's
// dense indices.

use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::{GraphError, Projection};

const NONE: u32 = u32::MAX;

/// A path found by [`shortest_paths`].
#[derive(Clone, Debug, PartialEq)]
pub struct DensePath {
    /// Dense node indices, from source to target.
    pub nodes: Vec<u32>,
    /// Sum of the edge weights (edge count for hop queries).
    pub cost: f64,
    /// Nodes settled by the search (Dijkstra), or reached (BFS).
    pub settled: usize,
}

/// Check the shared options: weights must exist for weighted queries, node
/// indices must be in range, `max_cost` must be non-negative.
fn check(
    p: &Projection,
    nodes: impl IntoIterator<Item = u32>,
    weighted: bool,
    max_cost: Option<f64>,
) -> Result<(), GraphError> {
    crate::pathfinding::check_max_cost(max_cost)?;
    if weighted && !p.is_weighted() {
        return Err(GraphError::InvalidArgument(
            "weighted queries need a projection with edge weights (project with weight=...)".into(),
        ));
    }
    if let Some(u) = nodes.into_iter().find(|&u| u as usize >= p.node_count()) {
        return Err(GraphError::InvalidArgument(format!("node index {} out of range", u)));
    }
    Ok(())
}

/// Per-thread scratch space, sized to the snapshot and reset after each
/// query through the list of touched nodes.
struct Workspace {
    dist: Vec<f64>,
    parent: Vec<u32>,
    touched: Vec<u32>,
    heap: BinaryHeap<(Reverse<u64>, u32)>,
    queue: VecDeque<u32>,
}

impl Workspace {
    fn new(n: usize) -> Self {
        Workspace {
            dist: vec![f64::INFINITY; n],
            parent: vec![NONE; n],
            touched: Vec::new(),
            heap: BinaryHeap::new(),
            queue: VecDeque::new(),
        }
    }

    fn reset(&mut self) {
        for &u in &self.touched {
            self.dist[u as usize] = f64::INFINITY;
            self.parent[u as usize] = NONE;
        }
        self.touched.clear();
        self.heap.clear();
        self.queue.clear();
    }

    fn reach(&mut self, u: u32, d: f64, parent: u32) {
        if self.dist[u as usize].is_infinite() {
            self.touched.push(u);
        }
        self.dist[u as usize] = d;
        self.parent[u as usize] = parent;
    }

    /// Search from `source` until `target` is settled (or everything within
    /// `max_cost` is, without a target): Dijkstra if `weighted`, BFS
    /// otherwise. Returns the number of nodes settled. Afterwards `touched`
    /// lists the nodes reached, in order.
    fn run(
        &mut self,
        p: &Projection,
        weighted: bool,
        source: u32,
        target: Option<u32>,
        max_cost: Option<f64>,
    ) -> usize {
        self.reach(source, 0.0, NONE);
        if target == Some(source) {
            return 1;
        }
        match weighted {
            // Dijkstra. Costs are non-negative, so their bit patterns order
            // like the values themselves.
            true => {
                let mut settled = 0;
                self.heap.push((Reverse(0f64.to_bits()), source));
                while let Some((Reverse(bits), u)) = self.heap.pop() {
                    let du = f64::from_bits(bits);
                    if du > self.dist[u as usize] {
                        continue; // stale entry
                    }
                    settled += 1;
                    if Some(u) == target {
                        break;
                    }
                    let weights = p.out_weights(u).expect("checked: the projection is weighted");
                    for (&v, &w) in p.out_neighbors(u).iter().zip(weights) {
                        let nd = du + w;
                        if nd < self.dist[v as usize] && max_cost.is_none_or(|limit| nd <= limit) {
                            self.reach(v, nd, u);
                            self.heap.push((Reverse(nd.to_bits()), v));
                        }
                    }
                }
                settled
            }
            // BFS; max_cost limits the number of edges.
            false => {
                let limit = max_cost.map(|c| c.floor());
                self.queue.push_back(source);
                while let Some(u) = self.queue.pop_front() {
                    let next = self.dist[u as usize] + 1.0;
                    if limit.is_some_and(|l| next > l) {
                        continue;
                    }
                    for &v in p.out_neighbors(u) {
                        if self.dist[v as usize].is_infinite() {
                            self.reach(v, next, u);
                            if Some(v) == target {
                                return self.touched.len();
                            }
                            self.queue.push_back(v);
                        }
                    }
                }
                self.touched.len()
            }
        }
    }

    fn path_to(&self, target: u32) -> Vec<u32> {
        let mut path = Vec::new();
        let mut u = target;
        while u != NONE {
            path.push(u);
            u = self.parent[u as usize];
        }
        path.reverse();
        path
    }
}

/// Shortest path for every `(source, target)` pair, computed in parallel:
/// Dijkstra over the projection's weights if `weighted`, BFS (edge count)
/// otherwise. `None` for pairs whose target is not reachable within
/// `max_cost` (for BFS: edges). Results are in the order of `pairs`.
pub fn shortest_paths(
    p: &Projection,
    pairs: &[(u32, u32)],
    weighted: bool,
    max_cost: Option<f64>,
) -> Result<Vec<Option<DensePath>>, GraphError> {
    check(p, pairs.iter().flat_map(|&(a, b)| [a, b]), weighted, max_cost)?;
    let stop = crate::cancel::stop();
    Ok(pairs
        .par_iter()
        .map_init(
            || Workspace::new(p.node_count()),
            |ws, &(source, target)| {
                if stop.requested() {
                    return None;
                }
                let settled = ws.run(p, weighted, source, Some(target), max_cost);
                let result = ws.dist[target as usize].is_finite().then(|| DensePath {
                    nodes: ws.path_to(target),
                    cost: ws.dist[target as usize],
                    settled,
                });
                ws.reset();
                result
            },
        )
        .collect())
}

/// Every node reachable from each source within `max_cost`, with its cost
/// (edge count unless `weighted`), computed in parallel. One list per
/// source, in discovery order, starting with the source itself.
pub fn distances(
    p: &Projection,
    sources: &[u32],
    weighted: bool,
    max_cost: Option<f64>,
) -> Result<Vec<Vec<(u32, f64)>>, GraphError> {
    check(p, sources.iter().copied(), weighted, max_cost)?;
    let stop = crate::cancel::stop();
    Ok(sources
        .par_iter()
        .map_init(
            || Workspace::new(p.node_count()),
            |ws, &source| {
                if stop.requested() {
                    return Vec::new();
                }
                ws.run(p, weighted, source, None, max_cost);
                let out = ws.touched.iter().map(|&u| (u, ws.dist[u as usize])).collect();
                ws.reset();
                out
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathfinding::{find_path, EdgeCost, PathQuery};
    use crate::{Direction, Graph, NodeIx, Record, Value};

    fn weighted(w: f64) -> Record {
        Record::with_attr([("weight", Value::from(w))])
    }

    // a -> b (1) -> d (5), a -> c (2) -> d (1), d -> e (default 1), f isolated
    fn diamond() -> (Graph<Record, Record>, Vec<NodeIx>) {
        let mut g = Graph::new();
        let ix: Vec<NodeIx> =
            ["a", "b", "c", "d", "e", "f"].iter().map(|id| g.add_node(*id, Record::default()).unwrap()).collect();
        for (f, t, w) in [(0, 1, Some(1.0)), (1, 3, Some(5.0)), (0, 2, Some(2.0)), (2, 3, Some(1.0)), (3, 4, None)] {
            g.add_edge(ix[f], ix[t], w.map_or_else(Record::default, weighted)).unwrap();
        }
        (g, ix)
    }

    fn project(g: &Graph<Record, Record>, dir: Direction, weighted: bool) -> Projection {
        let cost = if weighted { EdgeCost::weighted(None, None) } else { EdgeCost::Unit };
        Projection::build::<_, _, GraphError>(g, dir, &cost).unwrap()
    }

    #[test]
    fn paths_match_find_path() {
        let (g, ix) = diamond();
        let p = project(&g, Direction::Out, true);
        // Dense indices follow slot order: a=0 .. f=5
        let r = shortest_paths(&p, &[(0, 4), (4, 0), (2, 2)], true, None).unwrap();
        let single = find_path::<_, _, GraphError>(&g, ix[0], ix[4], &mut PathQuery::dijkstra()).unwrap().unwrap();
        let path = r[0].as_ref().unwrap();
        let nodes: Vec<NodeIx> = path.nodes.iter().map(|&u| p.node(u)).collect();
        assert_eq!((nodes, path.cost), (single.nodes, single.cost));
        assert!(r[1].is_none());
        assert_eq!(r[2].as_ref().unwrap().nodes, [2]);

        // Hop counts on the same (weighted) projection
        let hops = shortest_paths(&p, &[(0, 4)], false, None).unwrap()[0].clone().unwrap();
        assert_eq!((hops.nodes.len(), hops.cost), (4, 3.0));
        assert!(shortest_paths(&p, &[(0, 4)], false, Some(2.5)).unwrap()[0].is_none());

        let back = project(&g, Direction::In, true);
        assert_eq!(shortest_paths(&back, &[(4, 0)], true, None).unwrap()[0].as_ref().unwrap().cost, 4.0);
    }

    #[test]
    fn distances_and_limits() {
        let (g, _) = diamond();
        let p = project(&g, Direction::Out, true);
        let d = distances(&p, &[0, 5], true, Some(3.0)).unwrap();
        let mut got = d[0].clone();
        got.sort_by_key(|&(n, _)| n);
        assert_eq!(got, [(0, 0.0), (1, 1.0), (2, 2.0), (3, 3.0)]);
        assert_eq!(d[1], [(5, 0.0)]);
        let both = project(&g, Direction::Both, false);
        assert_eq!(distances(&both, &[4], false, None).unwrap()[0].len(), 5);
        assert!(distances(&p, &[0], true, Some(-1.0)).is_err());
    }

    #[test]
    fn option_errors() {
        let (g, _) = diamond();
        let unweighted = project(&g, Direction::Out, false);
        assert!(shortest_paths(&unweighted, &[(0, 1)], true, None).is_err());
        assert!(distances(&unweighted, &[6], false, None).is_err());
    }
}
