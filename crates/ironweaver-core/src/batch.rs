// batch.rs
//
// Many shortest-path queries at once, in parallel.
//
// `Snapshot::build` copies the graph structure (for one direction) and the
// edge costs into flat arrays (CSR layout). The snapshot owns plain data and
// no payloads, so the queries can run on all cores without touching the
// graph; the Python bindings build it while holding the GIL and release the
// GIL for the queries. Each worker thread reuses one set of per-node arrays
// across its queries and resets only the entries a query touched.

use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::pathfinding::{EdgeCost, PathResult};
use crate::{Attributes, Direction, Graph, GraphError, NodeIx};

/// Flat, read-only copy of a graph's adjacency (one direction) and edge
/// costs.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Neighbours of dense node `i` are `to[start[i]..start[i + 1]]`.
    start: Vec<u32>,
    to: Vec<u32>,
    /// Cost of each entry of `to`; `None` means every edge costs 1 (BFS).
    weight: Option<Vec<f64>>,
    /// Dense index -> node.
    nodes: Vec<NodeIx>,
    /// Node slot -> dense index (`u32::MAX` for empty slots).
    dense: Vec<u32>,
}

const NONE: u32 = u32::MAX;

impl Snapshot {
    /// Snapshot of `g` following edges in `direction`, with costs from
    /// `cost` (`EdgeCost::Unit` for hop counts). Every edge's cost is read
    /// and validated here, so a negative or non-numeric weight anywhere is
    /// an error.
    pub fn build<N, E, X>(g: &Graph<N, E>, direction: Direction, cost: &EdgeCost) -> Result<Self, X>
    where
        E: Attributes,
        X: From<GraphError> + From<E::Error>,
    {
        let mut dense = vec![NONE; g.node_bound()];
        let nodes: Vec<NodeIx> = g.node_indices().collect();
        for (i, ix) in nodes.iter().enumerate() {
            dense[ix.slot()] = i as u32;
        }
        let edges_hint = if direction == Direction::Both { 2 * g.edge_count() } else { g.edge_count() };
        let mut start = Vec::with_capacity(nodes.len() + 1);
        let mut to = Vec::with_capacity(edges_hint);
        let mut weight = match cost {
            EdgeCost::Unit => None,
            EdgeCost::Weighted { .. } => Some(Vec::with_capacity(edges_hint)),
        };
        start.push(0);
        for &ix in &nodes {
            for (e, neighbor) in g.neighbors(ix, direction) {
                to.push(dense[neighbor.slot()]);
                if let Some(w) = &mut weight {
                    w.push(cost.cost::<E, X>(&g.edge_ref(e).data)?);
                }
            }
            start.push(u32::try_from(to.len()).expect("too many edges for a snapshot"));
        }
        Ok(Snapshot { start, to, weight, nodes, dense })
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Whether edges carry weights (Dijkstra) or all cost 1 (BFS).
    pub fn is_weighted(&self) -> bool {
        self.weight.is_some()
    }

    fn index_of(&self, ix: NodeIx) -> Result<u32, GraphError> {
        match self.dense.get(ix.slot()) {
            Some(&i) if i != NONE && self.nodes[i as usize] == ix => Ok(i),
            _ => Err(GraphError::Stale),
        }
    }

    fn neighbors(&self, u: u32) -> std::ops::Range<usize> {
        self.start[u as usize] as usize..self.start[u as usize + 1] as usize
    }
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
    /// `max_cost` is, without a target). Returns the number of nodes settled.
    /// Afterwards `touched` lists the nodes reached, in order.
    fn run(&mut self, s: &Snapshot, source: u32, target: Option<u32>, max_cost: Option<f64>) -> usize {
        self.reach(source, 0.0, NONE);
        if target == Some(source) {
            return 1;
        }
        match &s.weight {
            // Dijkstra. Costs are non-negative, so their bit patterns order
            // like the values themselves.
            Some(weight) => {
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
                    for i in s.neighbors(u) {
                        let v = s.to[i];
                        let nd = du + weight[i];
                        if nd < self.dist[v as usize] && max_cost.is_none_or(|limit| nd <= limit) {
                            self.reach(v, nd, u);
                            self.heap.push((Reverse(nd.to_bits()), v));
                        }
                    }
                }
                settled
            }
            // BFS; max_cost limits the number of edges.
            None => {
                let limit = max_cost.map(|c| c.floor());
                self.queue.push_back(source);
                while let Some(u) = self.queue.pop_front() {
                    let next = self.dist[u as usize] + 1.0;
                    if limit.is_some_and(|l| next > l) {
                        continue;
                    }
                    for i in s.neighbors(u) {
                        let v = s.to[i];
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

    fn path_to(&self, s: &Snapshot, target: u32) -> Vec<NodeIx> {
        let mut path = Vec::new();
        let mut u = target;
        while u != NONE {
            path.push(s.nodes[u as usize]);
            u = self.parent[u as usize];
        }
        path.reverse();
        path
    }
}

/// Shortest path for every `(source, target)` pair, computed in parallel:
/// Dijkstra if the snapshot is weighted, BFS otherwise. `None` for pairs
/// whose target is not reachable within `max_cost` (for BFS: edges).
/// Results are in the order of `pairs`; `expanded` is set for Dijkstra.
pub fn shortest_paths(
    s: &Snapshot,
    pairs: &[(NodeIx, NodeIx)],
    max_cost: Option<f64>,
) -> Result<Vec<Option<PathResult>>, GraphError> {
    crate::pathfinding::check_max_cost(max_cost)?;
    let dense: Vec<(u32, u32)> =
        pairs.iter().map(|&(a, b)| Ok((s.index_of(a)?, s.index_of(b)?))).collect::<Result<_, GraphError>>()?;
    Ok(dense
        .par_iter()
        .map_init(
            || Workspace::new(s.len()),
            |ws, &(source, target)| {
                let settled = ws.run(s, source, Some(target), max_cost);
                let result = ws.dist[target as usize].is_finite().then(|| PathResult {
                    nodes: ws.path_to(s, target),
                    cost: ws.dist[target as usize],
                    expanded: s.is_weighted().then_some(settled),
                });
                ws.reset();
                result
            },
        )
        .collect())
}

/// Every node reachable from each source within `max_cost`, with its cost
/// (edge count for BFS), computed in parallel. One list per source, in
/// discovery order, starting with the source itself.
pub fn distances(
    s: &Snapshot,
    sources: &[NodeIx],
    max_cost: Option<f64>,
) -> Result<Vec<Vec<(NodeIx, f64)>>, GraphError> {
    crate::pathfinding::check_max_cost(max_cost)?;
    let dense: Vec<u32> = sources.iter().map(|&a| s.index_of(a)).collect::<Result<_, _>>()?;
    Ok(dense
        .par_iter()
        .map_init(
            || Workspace::new(s.len()),
            |ws, &source| {
                ws.run(s, source, None, max_cost);
                let out = ws.touched.iter().map(|&u| (s.nodes[u as usize], ws.dist[u as usize])).collect();
                ws.reset();
                out
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathfinding::{find_path, PathQuery};
    use crate::{Record, Value};

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

    fn snap(g: &Graph<Record, Record>, dir: Direction, weighted: bool) -> Snapshot {
        let cost = if weighted { EdgeCost::weighted(None, None) } else { EdgeCost::Unit };
        Snapshot::build::<_, _, GraphError>(g, dir, &cost).unwrap()
    }

    #[test]
    fn paths_match_find_path() {
        let (g, ix) = diamond();
        let s = snap(&g, Direction::Out, true);
        let r = shortest_paths(&s, &[(ix[0], ix[4]), (ix[4], ix[0]), (ix[2], ix[2])], None).unwrap();
        let single = find_path::<_, _, GraphError>(&g, ix[0], ix[4], &mut PathQuery::dijkstra()).unwrap().unwrap();
        let p = r[0].as_ref().unwrap();
        assert_eq!((p.nodes.clone(), p.cost), (single.nodes, single.cost));
        assert!(r[1].is_none());
        assert_eq!(r[2].as_ref().unwrap().nodes, [ix[2]]);

        let bfs = snap(&g, Direction::Out, false);
        let p = shortest_paths(&bfs, &[(ix[0], ix[4])], None).unwrap()[0].clone().unwrap();
        assert_eq!((p.nodes.len(), p.cost, p.expanded), (4, 3.0, None));
        assert!(shortest_paths(&bfs, &[(ix[0], ix[4])], Some(2.5)).unwrap()[0].is_none());

        let back = snap(&g, Direction::In, true);
        let p = shortest_paths(&back, &[(ix[4], ix[0])], None).unwrap()[0].clone().unwrap();
        assert_eq!(p.cost, 4.0);
    }

    #[test]
    fn distances_and_limits() {
        let (g, ix) = diamond();
        let s = snap(&g, Direction::Out, true);
        let d = distances(&s, &[ix[0], ix[5]], Some(3.0)).unwrap();
        let mut got: Vec<(NodeIx, f64)> = d[0].clone();
        got.sort_by_key(|&(n, _)| n);
        assert_eq!(got, [(ix[0], 0.0), (ix[1], 1.0), (ix[2], 2.0), (ix[3], 3.0)]);
        assert_eq!(d[1], [(ix[5], 0.0)]);
        let both = snap(&g, Direction::Both, false);
        assert_eq!(distances(&both, &[ix[4]], None).unwrap()[0].len(), 5);
        assert!(distances(&s, &[ix[0]], Some(-1.0)).is_err());
    }

    #[test]
    fn stale_and_bad_weights() {
        let (mut g, ix) = diamond();
        let s = snap(&g, Direction::Out, true);
        g.remove_node(ix[5]);
        let f2 = g.add_node("f2", Record::default()).unwrap();
        assert_eq!(shortest_paths(&s, &[(ix[0], f2)], None).unwrap_err(), GraphError::Stale);
        g.add_edge(ix[0], ix[1], weighted(-1.0)).unwrap();
        let err = Snapshot::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::weighted(None, None));
        assert!(err.is_err());
    }
}
