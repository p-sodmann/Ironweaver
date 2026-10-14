// batch.rs
//
// Many shortest-path queries at once, in parallel, on a [`Projection`]. The
// projection owns plain data and no payloads, so the queries run on all
// cores without touching the graph (the Python bindings release the GIL).
// Each worker thread reuses one set of per-node arrays across its queries
// and resets only the entries a query touched. Nodes are the projection's
// dense indices.
//
// A* (`shortest_paths_astar`) reads node coordinates from the projection
// (`Projection::coords`, a flat array) and estimates the remaining cost as
// `scale * metric(node, target)`. Metrics are symmetric, so the estimate
// does not depend on the projection's direction. The search does not close
// nodes: a node reached again more cheaply is expanded again, so an
// admissible but inconsistent estimate still gives the cheapest path. Ties
// in f = g + h prefer the larger g (the node closer to the target), which
// keeps grids with many equal-cost paths from being explored in full.
//
// Ties between equal-cost paths are decided by node index, the same way
// for every method, query and thread count: the heaps pop equal keys lowest
// index first (A*: lowest f, then highest g, then lowest index), and a node
// reached again at exactly its current cost takes the new predecessor if
// it has a lower index (over an edge of positive weight: re-parenting over
// a zero-weight edge could close a cycle, so there the first route stays). Walking back from the target, each step of a BFS
// or Dijkstra path (positive weights) therefore goes to the lowest-indexed
// neighbour through which a cheapest route arrives. BFS stops when the
// target is found, but first checks the rest of that level for a lower
// predecessor (a binary search in each sorted row).
//
// Edges of infinite weight are impassable (see `Projection::set_weights`):
// Dijkstra and A* never relax them (an infinite cost never improves on
// "unreached"), and BFS on a weighted projection skips them too.

use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::algo::passable_at;
use crate::pathfinding::Metric;
use crate::{GraphError, Projection};

const NONE: u32 = u32::MAX;

/// A path found by [`shortest_paths`].
#[derive(Clone, Debug, PartialEq)]
pub struct DensePath {
    /// Dense node indices, from source to target.
    pub nodes: Vec<u32>,
    /// Sum of the edge weights (edge count for hop queries).
    pub cost: f64,
    /// Nodes settled by the search (Dijkstra), expanded (A*; a node can be
    /// expanded again if the estimate is not consistent), or reached (BFS).
    pub settled: usize,
}

/// A* options for [`shortest_paths_astar`]: the estimate from node `u` to
/// the target `t` is `scale * metric(coords(u), coords(t))`.
///
/// The path is the cheapest one if the estimate never exceeds the real
/// remaining cost: it suffices that every edge weighs at least `scale`
/// times the metric distance between its endpoints (for example, with
/// `scale` set to the smallest cost per unit of distance). A larger `scale`
/// settles fewer nodes but may return a more expensive path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AStar {
    pub metric: Metric,
    pub scale: f64,
}

impl AStar {
    /// `scale` must be finite and non-negative (0 is Dijkstra).
    pub fn new(metric: Metric, scale: f64) -> Result<Self, GraphError> {
        if !(scale.is_finite() && scale >= 0.0) {
            return Err(GraphError::InvalidArgument(format!(
                "heuristic_scale must be finite and non-negative, got {}",
                scale
            )));
        }
        Ok(AStar { metric, scale })
    }
}

impl Default for AStar {
    /// Euclidean distance, scale 1.
    fn default() -> Self {
        AStar { metric: Metric::Euclidean, scale: 1.0 }
    }
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
    /// Dijkstra's heap, lowest cost then lowest index first.
    heap: BinaryHeap<Reverse<(u64, u32)>>,
    queue: VecDeque<u32>,
    /// A*'s open list, ordered by lowest f, then highest g, then lowest index.
    open: BinaryHeap<(Reverse<u64>, u64, Reverse<u32>)>,
}

impl Workspace {
    fn new(n: usize) -> Self {
        Workspace {
            dist: vec![f64::INFINITY; n],
            parent: vec![NONE; n],
            touched: Vec::new(),
            heap: BinaryHeap::new(),
            queue: VecDeque::new(),
            open: BinaryHeap::new(),
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
        self.open.clear();
    }

    fn reach(&mut self, u: u32, d: f64, parent: u32) {
        if self.dist[u as usize].is_infinite() {
            self.touched.push(u);
        }
        self.dist[u as usize] = d;
        self.parent[u as usize] = parent;
    }

    /// `u` reaches `v` at exactly its current cost, over an edge of
    /// positive weight: keep the lower of the two predecessors. (The source
    /// and unreached nodes have no predecessor and keep none: an impassable
    /// edge "reaches" an unreached node at infinite cost.)
    #[inline]
    fn tie(&mut self, u: u32, v: u32) {
        let parent = self.parent[v as usize];
        if u < parent && parent != NONE {
            self.parent[v as usize] = u;
        }
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
                self.heap.push(Reverse((0f64.to_bits(), source)));
                while let Some(Reverse((bits, u))) = self.heap.pop() {
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
                        let dv = self.dist[v as usize];
                        if nd < dv && max_cost.is_none_or(|limit| nd <= limit) {
                            self.reach(v, nd, u);
                            self.heap.push(Reverse((nd.to_bits(), v)));
                        } else if nd == dv && w > 0.0 {
                            self.tie(u, v);
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
                    let weights = p.out_weights(u);
                    for (i, &v) in p.out_neighbors(u).iter().enumerate() {
                        if !passable_at(weights, i) {
                            continue;
                        }
                        // Distances are whole numbers or infinite: compare bits
                        let dv = self.dist[v as usize].to_bits();
                        if dv == f64::INFINITY.to_bits() {
                            self.reach(v, next, u);
                            if Some(v) == target {
                                self.lowest_parent_in_level(p, v);
                                return self.touched.len();
                            }
                            self.queue.push_back(v);
                        } else if dv == next.to_bits() && u < self.parent[v as usize] {
                            // v (not the source) has a predecessor: keep the lower
                            self.parent[v as usize] = u;
                        }
                    }
                }
                self.touched.len()
            }
        }
    }

    /// BFS just found `target`: the rest of its predecessor's level (at the
    /// front of the queue) may hold a lower predecessor.
    fn lowest_parent_in_level(&mut self, p: &Projection, target: u32) {
        let level = self.dist[target as usize] - 1.0;
        for &u in &self.queue {
            if self.dist[u as usize] != level {
                break;
            }
            if u < self.parent[target as usize] && has_passable_edge(p, u, target) {
                self.parent[target as usize] = u;
            }
        }
    }

    /// A* from `source` to `target` with weights (checked: the projection
    /// is weighted and has coordinates). Returns the number of expansions.
    fn run_astar(&mut self, p: &Projection, astar: &AStar, source: u32, target: u32, max_cost: Option<f64>) -> usize {
        self.reach(source, 0.0, NONE);
        if source == target {
            return 1;
        }
        let goal = p.coords(target).expect("checked: the projection has coordinates");
        // Computed when needed (a few operations on the flat coordinate
        // array): cheaper than caching. 0 * an overflowing distance would
        // be NaN, hence the test.
        let h = |v: u32| -> f64 {
            let c = p.coords(v).expect("checked: the projection has coordinates");
            if astar.scale == 0.0 {
                0.0
            } else {
                astar.scale * astar.metric.distance(c, goal)
            }
        };
        let mut expanded = 0;
        let hs = h(source);
        self.open.push((Reverse(hs.to_bits()), 0f64.to_bits(), Reverse(source)));
        // f and g are non-negative, so their bit patterns order like the
        // values themselves.
        while let Some((_, g_bits, Reverse(u))) = self.open.pop() {
            let du = f64::from_bits(g_bits);
            if du > self.dist[u as usize] {
                continue; // stale entry
            }
            expanded += 1;
            if u == target {
                break;
            }
            let weights = p.out_weights(u).expect("checked: the projection is weighted");
            for (&v, &w) in p.out_neighbors(u).iter().zip(weights) {
                let nd = du + w;
                let dv = self.dist[v as usize];
                if nd == dv && w > 0.0 {
                    self.tie(u, v);
                } else if nd < dv {
                    let f = nd + h(v);
                    // The estimate never exceeds the rest of the path, so
                    // f > max_cost rules v out (h = 0 at the target).
                    if max_cost.is_some_and(|limit| f > limit) {
                        continue;
                    }
                    self.reach(v, nd, u);
                    self.open.push((Reverse(f.to_bits()), nd.to_bits(), Reverse(v)));
                }
            }
        }
        expanded
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

/// Whether `u` has a passable edge to `v` (rows are sorted).
fn has_passable_edge(p: &Projection, u: u32, v: u32) -> bool {
    let row = p.out_neighbors(u);
    let first = row.partition_point(|&x| x < v);
    let last = first + row[first..].partition_point(|&x| x == v);
    (first..last).any(|i| passable_at(p.out_weights(u), i))
}

/// Shortest path for every `(source, target)` pair, computed in parallel:
/// Dijkstra over the projection's weights if `weighted`, BFS (edge count)
/// otherwise. `None` for pairs whose target is not reachable within
/// `max_cost` (for BFS: edges). Results are in the order of `pairs`.
///
/// Among several cheapest paths, walking back from the target, each step
/// goes to the lowest-indexed node through which a cheapest route arrives
/// (over a zero-weight edge, only if strictly cheaper than the route found
/// first). The result does not depend on the order of the edges or on the
/// number of threads.
pub fn shortest_paths(
    p: &Projection,
    pairs: &[(u32, u32)],
    weighted: bool,
    max_cost: Option<f64>,
) -> Result<Vec<Option<DensePath>>, GraphError> {
    check(p, pairs.iter().flat_map(|&(a, b)| [a, b]), weighted, max_cost)?;
    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("shortest paths", Some(pairs.len() as u64));
    Ok(pairs
        .par_iter()
        .map_init(
            || Workspace::new(p.node_count()),
            |ws, &(source, target)| {
                if stop.requested() {
                    return None;
                }
                let settled = ws.run(p, weighted, source, Some(target), max_cost);
                report.add(1);
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

/// Shortest path for every `(source, target)` pair by A*, in parallel,
/// over the projection's weights, guided by its node coordinates (see
/// [`AStar`]). Results are like [`shortest_paths`]'; with an admissible
/// estimate the costs are the same, with fewer nodes settled. Ties follow
/// the same rule among the nodes A* expands (by lowest f, then highest g,
/// then lowest index), so among equal-cost paths A* may pick another one
/// than Dijkstra, but always the same one.
pub fn shortest_paths_astar(
    p: &Projection,
    pairs: &[(u32, u32)],
    astar: &AStar,
    max_cost: Option<f64>,
) -> Result<Vec<Option<DensePath>>, GraphError> {
    check(p, pairs.iter().flat_map(|&(a, b)| [a, b]), true, max_cost)?;
    AStar::new(astar.metric, astar.scale)?;
    if p.coord_dim() == 0 {
        return Err(GraphError::InvalidArgument(
            "astar needs a projection with node coordinates (project with coords=... or from_arrays(coords=...))"
                .into(),
        ));
    }
    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("shortest paths", Some(pairs.len() as u64));
    Ok(pairs
        .par_iter()
        .map_init(
            || Workspace::new(p.node_count()),
            |ws, &(source, target)| {
                if stop.requested() {
                    return None;
                }
                let settled = ws.run_astar(p, astar, source, target, max_cost);
                report.add(1);
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
    let report = crate::cancel::progress();
    report.start("distances", Some(sources.len() as u64));
    Ok(sources
        .par_iter()
        .map_init(
            || Workspace::new(p.node_count()),
            |ws, &source| {
                if stop.requested() {
                    return Vec::new();
                }
                ws.run(p, weighted, source, None, max_cost);
                report.add(1);
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

    /// A `w` x `h` 4-connected grid with directed edges both ways; entering
    /// a tile costs its cost (at least 1), some tiles are walls (no edges).
    fn grid(w: u32, h: u32, seed: u64) -> Projection {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let n = (w * h) as usize;
        let cost: Vec<f64> = (0..n).map(|_| 1.0 + (next() % 4) as f64).collect();
        let wall: Vec<bool> = (0..n).map(|i| i != 0 && i != n - 1 && next() % 5 == 0).collect();
        let (mut src, mut dst, mut wt) = (Vec::new(), Vec::new(), Vec::new());
        for y in 0..h {
            for x in 0..w {
                let u = y * w + x;
                for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let v = ny as u32 * w + nx as u32;
                    if wall[u as usize] || wall[v as usize] {
                        continue;
                    }
                    src.push(u as i64);
                    dst.push(v as i64);
                    wt.push(cost[v as usize]);
                }
            }
        }
        let ids = (0..n).map(|i| i.to_string()).collect();
        let coords = (0..n as u32).flat_map(|i| [(i % w) as f64, (i / w) as f64]).collect();
        Projection::from_edges(ids, Direction::Out, &src, &dst, Some(&wt))
            .unwrap()
            .with_coords(2, coords)
            .unwrap()
            .finish()
    }

    fn costs(r: &[Option<DensePath>]) -> Vec<Option<f64>> {
        r.iter().map(|p| p.as_ref().map(|p| p.cost)).collect()
    }

    /// The path's edges exist and its weights add up to its cost.
    fn check_path(p: &Projection, path: &DensePath) {
        let mut total = 0.0;
        for pair in path.nodes.windows(2) {
            let (u, v) = (pair[0], pair[1]);
            let row = p.out_neighbors(u);
            let w = p.out_weights(u).unwrap();
            total += row.iter().zip(w).filter(|&(&x, _)| x == v).map(|(_, &w)| w).fold(f64::INFINITY, f64::min);
        }
        assert!((total - path.cost).abs() < 1e-9, "{} vs {}", total, path.cost);
    }

    #[test]
    fn astar_matches_dijkstra_on_grids() {
        for seed in 1..6u64 {
            let p = grid(40, 30, seed);
            let n = p.node_count() as u32;
            let pairs: Vec<(u32, u32)> =
                (0..60u32).map(|i| ((i * 7919 + seed as u32) % n, (i * 104_729 + 3) % n)).chain([(0, n - 1)]).collect();
            let dijkstra = shortest_paths(&p, &pairs, true, None).unwrap();
            for metric in [Metric::Manhattan, Metric::Euclidean] {
                let astar = shortest_paths_astar(&p, &pairs, &AStar::new(metric, 1.0).unwrap(), None).unwrap();
                assert_eq!(costs(&astar), costs(&dijkstra));
                for (a, d) in astar.iter().zip(&dijkstra) {
                    if let (Some(a), Some(d)) = (a, d) {
                        check_path(&p, a);
                        assert!(a.settled <= d.settled, "{} > {}", a.settled, d.settled);
                    }
                }
            }
            // Corner to corner (open the way first) A* settles far fewer nodes
            let (a, d) = (
                shortest_paths_astar(&p, &[(0, n - 1)], &AStar::new(Metric::Manhattan, 1.0).unwrap(), None).unwrap(),
                shortest_paths(&p, &[(0, n - 1)], true, None).unwrap(),
            );
            if let (Some(a), Some(d)) = (&a[0], &d[0]) {
                assert!(a.settled < d.settled);
            }
            // max_cost gives the same answers
            for limit in [0.0, 10.0, 40.0] {
                let a = shortest_paths_astar(&p, &pairs, &AStar::default(), Some(limit)).unwrap();
                assert_eq!(costs(&a), costs(&shortest_paths(&p, &pairs, true, Some(limit)).unwrap()));
            }
        }
    }

    #[test]
    fn astar_matches_dijkstra_on_random_graphs() {
        // Random points; each edge weighs its Euclidean length times a
        // factor >= 1, so the Euclidean estimate is admissible (and with
        // scale 0 A* is Dijkstra).
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for direction in [Direction::Out, Direction::In, Direction::Both] {
            for round in 0..5 {
                let n = 50 + round * 20;
                let pts: Vec<f64> = (0..2 * n).map(|_| (next() % 1000) as f64 / 10.0).collect();
                let (mut src, mut dst, mut wt) = (Vec::new(), Vec::new(), Vec::new());
                for _ in 0..n * 3 {
                    let (u, v) = ((next() % n as u64) as usize, (next() % n as u64) as usize);
                    let d = ((pts[2 * u] - pts[2 * v]).powi(2) + (pts[2 * u + 1] - pts[2 * v + 1]).powi(2)).sqrt();
                    src.push(u as i64);
                    dst.push(v as i64);
                    wt.push(d * (1.0 + (next() % 100) as f64 / 50.0));
                }
                let ids = (0..n).map(|i| i.to_string()).collect();
                let p = Projection::from_edges(ids, direction, &src, &dst, Some(&wt))
                    .unwrap()
                    .with_coords(2, pts)
                    .unwrap()
                    .finish();
                let pairs: Vec<(u32, u32)> =
                    (0..100).map(|_| ((next() % n as u64) as u32, (next() % n as u64) as u32)).collect();
                let want = shortest_paths(&p, &pairs, true, None).unwrap();
                for (metric, scale) in [(Metric::Euclidean, 1.0), (Metric::Euclidean, 0.0), (Metric::Euclidean, 0.5)] {
                    let got = shortest_paths_astar(&p, &pairs, &AStar::new(metric, scale).unwrap(), None).unwrap();
                    for (g, w) in got.iter().zip(&want) {
                        match (g, w) {
                            (Some(g), Some(w)) => {
                                assert!((g.cost - w.cost).abs() < 1e-9);
                                check_path(&p, g);
                            }
                            (None, None) => {}
                            _ => panic!("reachability differs"),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn astar_reopens_nodes_for_inconsistent_estimates() {
        // s -> a (1) -> c (1) -> t (4), s -> b (1) -> c (5). With t at x = 0,
        // a's estimate (5) is admissible but inconsistent (a -> c costs 1
        // and c's estimate is 0): c is expanded first via b, then again.
        let ids = ["s", "a", "b", "c", "t"].map(String::from).to_vec();
        let (src, dst, w) = ([0i64, 0, 1, 2, 3], [1i64, 2, 3, 3, 4], [1.0, 1.0, 1.0, 5.0, 4.0]);
        let p = Projection::from_edges(ids, Direction::Out, &src, &dst, Some(&w))
            .unwrap()
            .with_coords(1, vec![0.0, 5.0, 0.0, 0.0, 0.0])
            .unwrap()
            .finish();
        let r = shortest_paths_astar(&p, &[(0, 4)], &AStar::new(Metric::Manhattan, 1.0).unwrap(), None).unwrap();
        let path = r[0].as_ref().unwrap();
        assert_eq!((&path.nodes[..], path.cost, path.settled), (&[0, 1, 3, 4][..], 6.0, 6));
    }

    #[test]
    fn astar_errors() {
        let (g, _) = diamond();
        let p = project(&g, Direction::Out, true);
        let err = |r: Result<Vec<Option<DensePath>>, GraphError>| r.unwrap_err().to_string();
        assert!(err(shortest_paths_astar(&p, &[(0, 1)], &AStar::default(), None)).contains("coordinates"));
        assert!(AStar::new(Metric::Manhattan, -1.0).is_err());
        assert!(AStar::new(Metric::Manhattan, f64::NAN).is_err());
        let unweighted = project(&g, Direction::Out, false);
        assert!(err(shortest_paths_astar(&unweighted, &[(0, 1)], &AStar::default(), None)).contains("weights"));
        let q = grid(3, 3, 1);
        assert!(shortest_paths_astar(&q, &[(0, 9)], &AStar::default(), None).is_err());
        assert_eq!(
            shortest_paths_astar(&q, &[(4, 4)], &AStar::default(), None).unwrap()[0].as_ref().unwrap().nodes,
            [4]
        );
    }
}
