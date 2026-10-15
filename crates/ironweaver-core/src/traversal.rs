// traversal.rs
//
// Graph walks: depth-first and breadth-first traversal, multi-source
// expansion and bidirectional shortest-path search.
//
// Edge filters are closures returning `Result<bool, X>`, so a filter can fail
// (e.g. a Python callback raising) and the error is passed straight back.
// The `*_limited` variants bound the work and the result size with a
// `Budget` (see `budget.rs`). Every search checks for cancellation and its
// edge budget once per edge, so a node with millions of edges can't run
// past either.

use std::collections::VecDeque;

use crate::budget::{Budget, Limited, Meter};
use crate::graph::{IxMap, IxSet};
use crate::query::paths::Steps;
use crate::{Direction, Edge, EdgeIx, Graph, GraphError, NodeIx};

/// Hash only small searches. Once enough live nodes have been reached, a
/// slot bitmap avoids hashing every subsequent edge. Its zeroing cost is
/// bounded by the work already done, even for a graph with many vacant slots.
/// This is only used by single-source walks: a stale start has no neighbors,
/// and all other visited handles are live in the immutably borrowed graph.
struct Visited {
    sparse: IxSet<NodeIx>,
    bits: Vec<u64>,
    bound: usize,
}

impl Visited {
    fn new(bound: usize) -> Self {
        Self { sparse: IxSet::default(), bits: Vec::new(), bound }
    }

    fn contains(&self, ix: &NodeIx) -> bool {
        if self.bits.is_empty() {
            self.sparse.contains(ix)
        } else {
            self.bits[ix.slot() / 64] & (1 << (ix.slot() % 64)) != 0
        }
    }

    fn insert(&mut self, ix: NodeIx) {
        if !self.bits.is_empty() {
            self.bits[ix.slot() / 64] |= 1 << (ix.slot() % 64);
        } else {
            self.sparse.insert(ix);
            if self.sparse.len() >= 128 && self.sparse.len() >= self.bound.div_ceil(64) {
                self.bits = vec![0; self.bound.div_ceil(64)];
                for node in self.sparse.drain() {
                    self.bits[node.slot() / 64] |= 1 << (node.slot() % 64);
                }
                self.sparse = IxSet::default();
            }
        }
    }
}

/// Depth-first (pre-order) traversal from `start` along outgoing edges for
/// which `edge_ok` returns true. Returns the nodes in visiting order; `depth`
/// limits how many edges away from `start` a node may be.
///
/// Iterative (an explicit stack), so arbitrarily long chains are fine. The
/// filter is asked about every edge of every visited node, including edges
/// to nodes that were already visited.
pub fn dfs<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    depth: Option<usize>,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Vec<NodeIx>, X> {
    let mut meter = Meter::new(Budget::UNLIMITED);
    dfs_metered(g, start, depth, Direction::Out, &mut meter, edge_ok)
}

/// [`dfs`] following edges in `direction`, under a [`Budget`]: nodes whose
/// edges are followed count as visited, edges passed to `edge_ok` as
/// examined, nodes returned as results. With [`Direction::Both`] a node's
/// outgoing edges come before its incoming ones, and a self-loop is
/// examined once.
pub fn dfs_limited<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    depth: Option<usize>,
    direction: Direction,
    budget: Budget,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Limited<Vec<NodeIx>>, X>
where
    X: From<GraphError>,
{
    let mut meter = Meter::new(budget);
    let order = dfs_metered(g, start, depth, direction, &mut meter, edge_ok)?;
    Ok(meter.finish(order)?)
}

fn dfs_metered<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    depth: Option<usize>,
    direction: Direction,
    meter: &mut Meter,
    mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Vec<NodeIx>, X> {
    // Frame: (the node's remaining steps, node depth)
    type Frame<'g> = (Steps<'g>, usize);
    struct Walk<'g, 'm> {
        order: Vec<NodeIx>,
        visited: Visited,
        stack: Vec<Frame<'g>>,
        meter: &'m mut Meter,
        direction: Direction,
    }
    /// Visit `ix` (if new); false if the budget stops the walk.
    fn enter<'g, N, E>(g: &'g Graph<N, E>, w: &mut Walk<'g, '_>, ix: NodeIx, d: usize, depth: Option<usize>) -> bool {
        if w.visited.contains(&ix) {
            return true;
        }
        if !w.meter.produce() {
            return false;
        }
        w.visited.insert(ix);
        w.order.push(ix);
        if depth.is_none_or(|max| d < max) && g.node(ix).is_some() {
            if !w.meter.enter() {
                return false;
            }
            w.stack.push((Steps::new(g, ix, w.direction), d));
        }
        true
    }

    let mut w = Walk { order: Vec::new(), visited: Visited::new(g.node_bound()), stack: Vec::new(), meter, direction };
    if !enter(g, &mut w, start, 0, depth) {
        return Ok(w.order);
    }
    let both = direction == Direction::Both;
    let stop = crate::cancel::stop();
    while let Some(frame) = w.stack.last_mut() {
        if stop.poll() {
            break;
        }
        let Some((e, neighbor)) = frame.0.next(g, both) else {
            w.stack.pop();
            continue;
        };
        if !w.meter.examine() {
            break;
        }
        let next_depth = frame.1 + 1;
        if edge_ok(e, g.edge_ref(e))? && !enter(g, &mut w, neighbor, next_depth, depth) {
            break;
        }
    }
    Ok(w.order)
}

/// Breadth-first traversal from `start` along outgoing edges for which
/// `edge_ok` returns true. Returns the nodes in visiting order; `depth`
/// limits how many edges away from `start` a node may be.
pub fn bfs<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    depth: Option<usize>,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Vec<NodeIx>, X> {
    let mut meter = Meter::new(Budget::UNLIMITED);
    bfs_metered(g, start, depth, Direction::Out, &mut meter, edge_ok)
}

/// [`bfs`] following edges in `direction`, under a [`Budget`]: nodes whose
/// edges are followed count as visited, edges passed to `edge_ok` as
/// examined, nodes returned as results. With [`Direction::Both`] a node's
/// outgoing edges come before its incoming ones, and a self-loop is
/// examined once.
pub fn bfs_limited<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    depth: Option<usize>,
    direction: Direction,
    budget: Budget,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Limited<Vec<NodeIx>>, X>
where
    X: From<GraphError>,
{
    let mut meter = Meter::new(budget);
    let order = bfs_metered(g, start, depth, direction, &mut meter, edge_ok)?;
    Ok(meter.finish(order)?)
}

fn bfs_metered<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    depth: Option<usize>,
    direction: Direction,
    meter: &mut Meter,
    mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Vec<NodeIx>, X> {
    if !meter.produce() {
        return Ok(Vec::new());
    }
    let mut order = vec![start];
    let mut visited = Visited::new(g.node_bound());
    visited.insert(start);
    let mut queue = VecDeque::from([(start, 0usize)]);

    let both = direction == Direction::Both;
    let stop = crate::cancel::stop();
    'search: while let Some((ix, d)) = queue.pop_front() {
        if stop.poll() {
            break;
        }
        if depth.is_some_and(|max| d >= max) {
            continue;
        }
        if g.node(ix).is_none() {
            continue;
        }
        if !meter.enter() {
            break;
        }
        let mut steps = Steps::new(g, ix, direction);
        while let Some((e, neighbor)) = steps.next(g, both) {
            if stop.poll() || !meter.examine() {
                break 'search;
            }
            if edge_ok(e, g.edge_ref(e))? && !visited.contains(&neighbor) {
                if !meter.produce() {
                    break 'search;
                }
                visited.insert(neighbor);
                order.push(neighbor);
                queue.push_back((neighbor, d + 1));
            }
        }
    }
    Ok(order)
}

/// Every node within `depth` edges (in `direction`) of any of the `seeds`,
/// seeds first, then in discovery order. One multi-source BFS: O(V + E)
/// however many seeds there are.
pub fn expand<N, E>(
    g: &Graph<N, E>,
    seeds: impl IntoIterator<Item = NodeIx>,
    depth: usize,
    direction: Direction,
) -> Vec<NodeIx> {
    let all = |_: EdgeIx, _: &Edge<E>| Ok::<_, std::convert::Infallible>(true);
    match expand_metered(g, seeds, depth, direction, &mut Meter::new(Budget::UNLIMITED), all) {
        Ok(order) => order,
        Err(never) => match never {},
    }
}

/// [`expand`] along the edges for which `edge_ok` returns true, under a
/// [`Budget`]: nodes whose neighbours are listed count as visited, the edges
/// they are listed through as examined (before `edge_ok`; with
/// [`Direction::Both`], an edge is examined from both ends, a self-loop
/// once), nodes returned (seeds included) as results. `edge_ok` is only
/// asked about edges to nodes not discovered yet.
pub fn expand_limited<N, E, X>(
    g: &Graph<N, E>,
    seeds: impl IntoIterator<Item = NodeIx>,
    depth: usize,
    direction: Direction,
    budget: Budget,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Limited<Vec<NodeIx>>, X>
where
    X: From<GraphError>,
{
    let mut meter = Meter::new(budget);
    let order = expand_metered(g, seeds, depth, direction, &mut meter, edge_ok)?;
    Ok(meter.finish(order)?)
}

fn expand_metered<N, E, X>(
    g: &Graph<N, E>,
    seeds: impl IntoIterator<Item = NodeIx>,
    depth: usize,
    direction: Direction,
    meter: &mut Meter,
    mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Vec<NodeIx>, X> {
    let mut order = Vec::new();
    let mut discovered = IxSet::default();
    let mut queue = VecDeque::new();
    for seed in seeds {
        if g.node(seed).is_some() && !discovered.contains(&seed) {
            if !meter.produce() {
                return Ok(order);
            }
            discovered.insert(seed);
            order.push(seed);
            queue.push_back((seed, 0usize));
        }
    }
    let both = direction == Direction::Both;
    let stop = crate::cancel::stop();
    'search: while let Some((ix, d)) = queue.pop_front() {
        if stop.poll() {
            break;
        }
        if d >= depth {
            continue;
        }
        if !meter.enter() {
            break;
        }
        let mut steps = Steps::new(g, ix, direction);
        while let Some((e, neighbor)) = steps.next(g, both) {
            if stop.poll() || !meter.examine() {
                break 'search;
            }
            if !discovered.contains(&neighbor) && edge_ok(e, g.edge_ref(e))? {
                if !meter.produce() {
                    break 'search;
                }
                discovered.insert(neighbor);
                order.push(neighbor);
                if d + 1 < depth {
                    queue.push_back((neighbor, d + 1));
                }
            }
        }
    }
    Ok(order)
}

/// Depth of each node reached by one side of a bidirectional search (0 for
/// the side's starting node).
type Depths = IxMap<NodeIx, usize>;

/// Shortest path (fewest edges) from `source` to `target` following edges in
/// `direction`, using only edges for which `edge_ok` returns true. Returns
/// the nodes on the path (both ends included), or `None` if there is no path
/// of at most `max_depth` edges.
///
/// Grows one frontier from each end and stops when they meet; on graphs
/// whose BFS frontier fans out quickly this explores roughly the square root
/// of what a one-sided search does. Each round expands one complete level of
/// the smaller frontier, so the first meeting gives the length of a
/// shortest path.
///
/// Among several shortest paths the one returned is fixed: walking back
/// from the target, each step goes to the neighbour with the lowest
/// [`NodeIx`] (slot) among those one edge closer to the source on a
/// shortest path. That is the path a one-sided BFS (or Dijkstra with unit
/// weights) picks under the same rule (see [`find_path`](crate::pathfinding::find_path)); it does
/// not depend on the order of the edges, nor on how the frontiers met.
/// Use [`bidirectional_bfs_any`] to skip this canonical choice.
pub fn bidirectional_bfs<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Option<Vec<NodeIx>>, X> {
    let mut meter = Meter::new(Budget::UNLIMITED);
    bidirectional_metered(g, source, target, max_depth, direction, &mut meter, edge_ok)
}

/// [`bidirectional_bfs`] under a [`Budget`]: frontier nodes whose edges are
/// listed (from either end) count as visited, the edges listed as examined
/// (before `edge_ok`), a path found as a result. With a truncated search
/// the value is `None`: no path was found within the budget.
#[allow(clippy::too_many_arguments)]
pub fn bidirectional_bfs_limited<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    budget: Budget,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Limited<Option<Vec<NodeIx>>>, X>
where
    X: From<GraphError>,
{
    let mut meter = Meter::new(budget);
    let path = bidirectional_metered(g, source, target, max_depth, direction, &mut meter, edge_ok)?;
    Ok(meter.finish(path)?)
}

/// A shortest path through the first meeting of the two frontiers.
///
/// Like [`bidirectional_bfs`], but skips canonical tie-breaking. The chosen
/// shortest path depends on edge order. Reconstruction follows decreasing
/// depths on each side, examining only edges at nodes on the returned path.
/// Edge filters should give consistent answers throughout a call.
pub fn bidirectional_bfs_any<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Option<Vec<NodeIx>>, X> {
    let mut meter = Meter::new(Budget::UNLIMITED);
    bidirectional_search(g, source, target, max_depth, direction, &mut meter, edge_ok, PathMode::Any)
}

/// [`bidirectional_bfs_any`] under a [`Budget`]. Accounting follows
/// [`bidirectional_bfs_limited`], including edges examined in reconstruction.
/// If the budget stops reconstruction, returns `None`, never a partial path.
#[allow(clippy::too_many_arguments)]
pub fn bidirectional_bfs_any_limited<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    budget: Budget,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Limited<Option<Vec<NodeIx>>>, X>
where
    X: From<GraphError>,
{
    let mut meter = Meter::new(budget);
    let path = bidirectional_search(g, source, target, max_depth, direction, &mut meter, edge_ok, PathMode::Any)?;
    Ok(meter.finish(path)?)
}

#[derive(Clone, Copy)]
enum PathMode {
    Canonical,
    Any,
    Reachable,
}

/// Reachability only: stop at the first meeting without constructing a path.
pub fn bidirectional_reachable<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<bool, X> {
    let mut meter = Meter::new(Budget::UNLIMITED);
    Ok(bidirectional_search(g, source, target, max_depth, direction, &mut meter, edge_ok, PathMode::Reachable)?
        .is_some())
}

pub(crate) fn bidirectional_metered<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    meter: &mut Meter,
    edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Option<Vec<NodeIx>>, X> {
    bidirectional_search(g, source, target, max_depth, direction, meter, edge_ok, PathMode::Canonical)
}

#[allow(clippy::too_many_arguments)]
fn bidirectional_search<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    meter: &mut Meter,
    mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
    mode: PathMode,
) -> Result<Option<Vec<NodeIx>>, X> {
    if source == target {
        return Ok(meter.produce().then(|| vec![source]));
    }
    let mut fwd = Depths::default();
    fwd.insert(source, 0);
    let mut bwd = Depths::default();
    bwd.insert(target, 0);
    let mut fwd_frontier = vec![source];
    let mut bwd_frontier = vec![target];
    let (mut fwd_depth, mut bwd_depth) = (0usize, 0usize);

    let stop = crate::cancel::stop();
    while !fwd_frontier.is_empty() && !bwd_frontier.is_empty() {
        if stop.poll() {
            return Ok(None);
        }
        // The next meeting would give a path of fwd_depth + bwd_depth + 1 edges
        if max_depth.is_some_and(|d| fwd_depth + bwd_depth >= d) {
            return Ok(None);
        }

        let forward = fwd_frontier.len() <= bwd_frontier.len();
        let (frontier, this, other, dir, depth) = if forward {
            (&fwd_frontier, &mut fwd, &bwd, direction, fwd_depth)
        } else {
            (&bwd_frontier, &mut bwd, &fwd, direction.reversed(), bwd_depth)
        };

        let mut next = Vec::new();
        let mut meeting = None;
        'level: for &ix in frontier.iter() {
            if !meter.enter() {
                return Ok(None);
            }
            for (e, neighbor) in g.neighbors(ix, dir) {
                if stop.poll() || !meter.examine() {
                    return Ok(None);
                }
                if this.contains_key(&neighbor) || !edge_ok(e, g.edge_ref(e))? {
                    continue;
                }
                if other.contains_key(&neighbor) {
                    meeting = Some(if forward { (ix, neighbor) } else { (neighbor, ix) });
                    break 'level;
                }
                this.insert(neighbor, depth + 1);
                next.push(neighbor);
            }
        }

        if let Some((left, right)) = meeting {
            let ends = Ends { source, target, a: fwd_depth, b: bwd_depth, fwd: &fwd, bwd: &bwd };
            let path = match mode {
                PathMode::Reachable => Some(Vec::new()),
                PathMode::Canonical => ends.canonical_path(g, direction, meter, &mut edge_ok)?,
                PathMode::Any => ends.any_path(g, left, right, direction, meter, &mut edge_ok)?,
            };
            return Ok(path.filter(|_| meter.produce()));
        }
        if forward {
            fwd_frontier = next;
            fwd_depth += 1;
        } else {
            bwd_frontier = next;
            bwd_depth += 1;
        }
    }
    Ok(None)
}

/// The state of a bidirectional search whose frontiers just met: `fwd`
/// holds every node within `a` edges of the source (and maybe some at
/// `a + 1`), `bwd` every node within `b` edges of the target (and maybe some
/// at `b + 1`). Shortest paths have `a + b + 1` edges and cross from depth
/// `a` of `fwd` to depth `b` of `bwd`.
struct Ends<'a> {
    source: NodeIx,
    target: NodeIx,
    a: usize,
    b: usize,
    fwd: &'a Depths,
    bwd: &'a Depths,
}

impl Ends<'_> {
    /// Both meeting endpoints already have a shortest route to their end.
    /// No viability search is needed: take the first allowed lower-depth edge.
    #[allow(clippy::too_many_arguments)]
    fn any_path<N, E, X>(
        &self,
        g: &Graph<N, E>,
        left: NodeIx,
        right: NodeIx,
        direction: Direction,
        meter: &mut Meter,
        edge_ok: &mut impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
    ) -> Result<Option<Vec<NodeIx>>, X> {
        let stop = crate::cancel::stop();
        let mut path = vec![left];
        for (start, depth, depths, dir) in
            [(left, self.a, self.fwd, direction.reversed()), (right, self.b, self.bwd, direction)]
        {
            if start == right {
                path.reverse();
                path.push(right);
            }
            let mut cur = start;
            for want in (0..depth).rev() {
                let mut next = None;
                for (e, u) in g.neighbors(cur, dir) {
                    if stop.poll() || !meter.examine() {
                        return Ok(None);
                    }
                    if depths.get(&u) == Some(&want) && edge_ok(e, g.edge_ref(e))? {
                        next = Some(u);
                        break;
                    }
                }
                let Some(u) = next else { return Ok(None) };
                path.push(u);
                cur = u;
            }
        }
        Ok(Some(path))
    }

    /// Whether a target-side node can reach the source frontier, following
    /// strictly increasing backward depths. Iterative so long chains do not
    /// use the call stack. Every examined edge still consumes the budget.
    #[allow(clippy::too_many_arguments)]
    fn viable<'g, N, E, X>(
        &self,
        g: &'g Graph<N, E>,
        start: NodeIx,
        direction: Direction,
        meter: &mut Meter,
        edge_ok: &mut impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
        cache: &mut IxMap<NodeIx, bool>,
        stack: &mut Vec<(NodeIx, Steps<'g>)>,
    ) -> Result<bool, X> {
        if let Some(&answer) = cache.get(&start) {
            return Ok(answer);
        }
        let reverse = direction.reversed();
        stack.clear();
        stack.push((start, Steps::new(g, start, reverse)));
        let stop = crate::cancel::stop();
        while let Some((u, steps)) = stack.last_mut() {
            if stop.poll() {
                return Ok(false);
            }
            let Some((e, v)) = steps.next(g, direction == Direction::Both) else {
                cache.insert(*u, false);
                stack.pop();
                continue;
            };
            if !meter.examine() {
                return Ok(false);
            }
            let depth = self.bwd[u];
            let at_meeting = depth == self.b;
            let fits =
                if at_meeting { self.fwd.get(&v) == Some(&self.a) } else { self.bwd.get(&v) == Some(&(depth + 1)) };
            if !fits || (!at_meeting && cache.get(&v) == Some(&false)) || !edge_ok(e, g.edge_ref(e))? {
                continue;
            }
            if at_meeting || cache.get(&v) == Some(&true) {
                for &(u, _) in stack.iter() {
                    cache.insert(u, true);
                }
                return Ok(true);
            }
            stack.push((v, Steps::new(g, v, reverse)));
        }
        Ok(false)
    }

    /// The shortest path that walks back from the target to the lowest
    /// node (by `NodeIx`) one step closer to the source each time. Edges
    /// looked at count as examined; `None` if the budget runs out or the
    /// search is cancelled (or if `edge_ok` contradicts itself).
    fn canonical_path<N, E, X>(
        &self,
        g: &Graph<N, E>,
        direction: Direction,
        meter: &mut Meter,
        edge_ok: &mut impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
    ) -> Result<Option<Vec<NodeIx>>, X> {
        let (a, len) = (self.a, self.a + self.b + 1);
        let stop = crate::cancel::stop();
        // Test target-side candidates lazily. Most queries need only one
        // successful chain to the meeting level, rather than every node on
        // every shortest path. Cache failed branches as well as successes.
        let mut viable: IxMap<NodeIx, bool> = IxMap::default();

        // Walk back from the target: the node at distance `pos` from the
        // source steps to the lowest neighbour at distance `pos - 1`.
        let mut path = vec![self.target];
        let mut cur = self.target;
        let mut candidates = Vec::new();
        let mut stack = Vec::new();
        for want in (0..len).rev() {
            candidates.clear();
            let mut best = None;
            for (e, u) in g.neighbors(cur, direction.reversed()) {
                if stop.poll() || !meter.examine() {
                    return Ok(None);
                }
                if want <= a {
                    // Source-side depths already prove viability. Find the
                    // lowest predecessor in one pass, without sorting.
                    if best.is_none_or(|best| u < best) && self.fwd.get(&u) == Some(&want) && edge_ok(e, g.edge_ref(e))?
                    {
                        best = Some(u);
                    }
                } else if self.bwd.get(&u) == Some(&(len - want)) {
                    candidates.push((u, e));
                }
            }
            // On the target side, try lowest predecessors first: do not
            // prove viability for a higher candidate we would replace.
            if want > a {
                candidates.sort_unstable();
                for &(u, e) in &candidates {
                    if edge_ok(e, g.edge_ref(e))?
                        && self.viable(g, u, direction, meter, edge_ok, &mut viable, &mut stack)?
                    {
                        best = Some(u);
                        break;
                    }
                }
            }
            // Only an edge filter that changes its answers leaves no step.
            let Some(u) = best else { return Ok(None) };
            path.push(u);
            cur = u;
        }
        path.reverse();
        Ok((path[0] == self.source).then_some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::convert::Infallible;

    fn all<E>(_: EdgeIx, _: &Edge<E>) -> Result<bool, Infallible> {
        Ok(true)
    }

    // a -> b -> d, a -> c -> d, d -> e
    fn diamond() -> (Graph<(), &'static str>, HashMap<&'static str, NodeIx>) {
        let mut g = Graph::new();
        let mut ix = HashMap::new();
        for id in ["a", "b", "c", "d", "e"] {
            ix.insert(id, g.add_node(id, ()).unwrap());
        }
        for (f, t, ty) in [("a", "b", "x"), ("b", "d", "x"), ("a", "c", "y"), ("c", "d", "y"), ("d", "e", "x")] {
            g.add_edge(ix[f], ix[t], ty).unwrap();
        }
        (g, ix)
    }

    fn ids(g: &Graph<(), &str>, nodes: &[NodeIx]) -> Vec<String> {
        nodes.iter().map(|&n| g.node(n).unwrap().id().to_string()).collect()
    }

    #[test]
    fn large_walks_keep_order_handles_filters_and_budgets() {
        let mut g = Graph::<(), ()>::new();
        let mut nodes: Vec<_> = (0..512).map(|i| g.add_node(i.to_string(), ()).unwrap()).collect();
        // Reuse slots with new generations, then connect in logical order.
        for i in (0..512).step_by(3) {
            g.remove_node(nodes[i]).unwrap();
            nodes[i] = g.add_node(format!("replacement-{i}"), ()).unwrap();
        }
        for i in 0..511 {
            g.add_edge(nodes[i], nodes[i + 1], ()).unwrap();
            g.add_edge(nodes[i], nodes[i], ()).unwrap();
            g.add_edge(nodes[i], nodes[i + 1], ()).unwrap();
        }
        assert_eq!(bfs(&g, nodes[0], None, all).unwrap(), nodes);
        assert_eq!(dfs(&g, nodes[0], None, all).unwrap(), nodes);
        // A filter still sees self-loops and parallel/already-visited edges.
        let mut called = 0;
        let order = bfs(&g, nodes[0], None, |_, _| {
            called += 1;
            Ok::<_, Infallible>(true)
        })
        .unwrap();
        assert_eq!(order, nodes);
        assert_eq!(called, g.edge_count());
        let budget = Budget::UNLIMITED.max_results(140).truncate();
        let accept = |_: EdgeIx, _: &Edge<()>| Ok::<_, GraphError>(true);
        for order in [
            bfs_limited(&g, nodes[0], None, Direction::Out, budget, accept).unwrap(),
            dfs_limited(&g, nodes[0], None, Direction::Out, budget, accept).unwrap(),
        ] {
            assert_eq!(order.value, nodes[..140]);
            assert!(order.truncated);
        }
        let mut backward = nodes.clone();
        backward.reverse();
        assert_eq!(
            bfs_limited(&g, nodes[511], None, Direction::In, Budget::UNLIMITED, accept).unwrap().value,
            backward
        );
        assert_eq!(
            dfs_limited(&g, nodes[511], None, Direction::In, Budget::UNLIMITED, accept).unwrap().value,
            backward
        );
        // Fallible filters must still propagate errors after bitmap promotion.
        let failed = bfs(
            &g,
            nodes[0],
            None,
            |_, edge| {
                if edge.target() == nodes[300] {
                    Err("filter failed")
                } else {
                    Ok(true)
                }
            },
        );
        assert_eq!(failed, Err("filter failed"));
        let stale = nodes[0];
        g.remove_node(stale).unwrap();
        g.add_node("new-root", ()).unwrap();
        assert_eq!(bfs(&g, stale, None, all).unwrap(), [stale]);
        assert_eq!(dfs(&g, stale, None, all).unwrap(), [stale]);
    }

    #[test]
    fn dfs_and_bfs_order() {
        let (g, ix) = diamond();
        assert_eq!(ids(&g, &dfs(&g, ix["a"], Some(2), all).unwrap()), ["a", "b", "d", "c"]);
        assert_eq!(ids(&g, &bfs(&g, ix["a"], None, all).unwrap()), ["a", "b", "c", "d", "e"]);
        assert_eq!(ids(&g, &bfs(&g, ix["a"], Some(1), all).unwrap()), ["a", "b", "c"]);
        let only_x = |_: EdgeIx, e: &Edge<&str>| Ok::<_, Infallible>(e.data == "x");
        assert_eq!(ids(&g, &bfs(&g, ix["a"], None, only_x).unwrap()), ["a", "b", "d", "e"]);
    }

    #[test]
    fn filter_errors_propagate() {
        let (g, ix) = diamond();
        let failing = |_: EdgeIx, _: &Edge<&str>| Err::<bool, _>("boom");
        assert_eq!(bfs(&g, ix["a"], None, failing), Err("boom"));
        assert_eq!(dfs(&g, ix["a"], None, failing), Err("boom"));
    }

    #[test]
    fn deep_chain_dfs() {
        let mut g: Graph<(), ()> = Graph::new();
        let mut prev = g.add_node("0", ()).unwrap();
        let first = prev;
        for i in 1..200_000 {
            let n = g.add_node(i.to_string(), ()).unwrap();
            g.add_edge(prev, n, ()).unwrap();
            prev = n;
        }
        assert_eq!(dfs(&g, first, None, all).unwrap().len(), 200_000);
    }

    #[test]
    fn expand_directions() {
        let (g, ix) = diamond();
        let mut got = ids(&g, &expand(&g, [ix["b"], ix["c"]], 1, Direction::Out));
        got.sort();
        assert_eq!(got, ["b", "c", "d"]);
        let mut got = ids(&g, &expand(&g, [ix["d"]], 1, Direction::Both));
        got.sort();
        assert_eq!(got, ["b", "c", "d", "e"]);
    }

    #[derive(Debug, PartialEq)]
    struct Never;
    impl From<GraphError> for Never {
        fn from(e: GraphError) -> Self {
            panic!("{e}")
        }
    }

    fn sorted(g: &Graph<(), &str>, nodes: &[NodeIx]) -> Vec<String> {
        let mut got = ids(g, nodes);
        got.sort();
        got
    }

    #[test]
    fn limited_traversals_follow_a_direction() {
        let (g, ix) = diamond();
        let any = |_: EdgeIx, _: &Edge<&str>| Ok::<_, Never>(true);
        let unlimited = Budget::UNLIMITED;
        for (dir, want) in [
            (Direction::Out, vec!["d", "e"]),
            (Direction::In, vec!["a", "b", "c", "d"]),
            (Direction::Both, vec!["a", "b", "c", "d", "e"]),
        ] {
            let b = bfs_limited(&g, ix["d"], None, dir, unlimited, any).unwrap();
            assert_eq!(sorted(&g, &b.value), want, "bfs {dir:?}");
            let d = dfs_limited(&g, ix["d"], None, dir, unlimited, any).unwrap();
            assert_eq!(sorted(&g, &d.value), want, "dfs {dir:?}");
        }
        // Incoming edges in order, depth counted the same way
        let b = bfs_limited(&g, ix["e"], Some(2), Direction::In, unlimited, any).unwrap();
        assert_eq!(ids(&g, &b.value), ["e", "d", "b", "c"]);
        let d = dfs_limited(&g, ix["e"], None, Direction::In, unlimited, any).unwrap();
        assert_eq!(ids(&g, &d.value), ["e", "d", "b", "a", "c"]);
        // Out matches the unlimited traversals
        let b = bfs_limited(&g, ix["a"], None, Direction::Out, unlimited, any).unwrap();
        assert_eq!(b.value, bfs(&g, ix["a"], None, all).unwrap());
        let d = dfs_limited(&g, ix["a"], None, Direction::Out, unlimited, any).unwrap();
        assert_eq!(d.value, dfs(&g, ix["a"], None, all).unwrap());
    }

    #[test]
    fn both_examines_a_self_loop_once() {
        let mut g: Graph<(), ()> = Graph::new();
        let a = g.add_node("a", ()).unwrap();
        g.add_edge(a, a, ()).unwrap();
        let any = |_: EdgeIx, _: &Edge<()>| Ok::<_, Never>(true);
        for dir in [Direction::Out, Direction::In, Direction::Both] {
            let b = bfs_limited(&g, a, None, dir, Budget::UNLIMITED, any).unwrap();
            assert_eq!((b.value.len(), b.edges), (1, 1), "bfs {dir:?}");
            let d = dfs_limited(&g, a, None, dir, Budget::UNLIMITED, any).unwrap();
            assert_eq!((d.value.len(), d.edges), (1, 1), "dfs {dir:?}");
            let x = expand_limited(&g, [a], 2, dir, Budget::UNLIMITED, any).unwrap();
            assert_eq!((x.value.len(), x.edges), (1, 1), "expand {dir:?}");
        }
    }

    #[test]
    fn expand_filters_edges() {
        let (g, ix) = diamond();
        let only_x = |_: EdgeIx, e: &Edge<&str>| Ok::<_, Never>(e.data == "x");
        let x = expand_limited(&g, [ix["a"]], 5, Direction::Out, Budget::UNLIMITED, only_x).unwrap();
        assert_eq!(ids(&g, &x.value), ["a", "b", "d", "e"]);
        // Edges are examined before the filter
        assert_eq!(x.edges, 4);
        let x = expand_limited(&g, [ix["d"]], 1, Direction::Both, Budget::UNLIMITED, only_x).unwrap();
        assert_eq!(sorted(&g, &x.value), ["b", "d", "e"]);
        let any = |_: EdgeIx, _: &Edge<&str>| Ok::<_, Never>(true);
        let x = expand_limited(&g, [ix["d"]], 1, Direction::Both, Budget::UNLIMITED, any).unwrap();
        assert_eq!(x.value, expand(&g, [ix["d"]], 1, Direction::Both));
        let failing = |_: EdgeIx, _: &Edge<&str>| Err::<bool, _>(Never);
        assert_eq!(expand_limited(&g, [ix["a"]], 1, Direction::Out, Budget::UNLIMITED, failing), Err(Never));
    }

    #[test]
    fn bidirectional_matches_bfs_distances() {
        let (g, ix) = diamond();
        let path = bidirectional_bfs(&g, ix["a"], ix["e"], None, Direction::Out, all).unwrap().unwrap();
        assert_eq!(path.len(), 4);
        assert_eq!(path[0], ix["a"]);
        assert_eq!(path[3], ix["e"]);
        assert!(bidirectional_bfs(&g, ix["a"], ix["e"], Some(2), Direction::Out, all).unwrap().is_none());
        assert!(bidirectional_bfs(&g, ix["e"], ix["a"], None, Direction::Out, all).unwrap().is_none());
        let back = bidirectional_bfs(&g, ix["e"], ix["a"], None, Direction::In, all).unwrap().unwrap();
        assert_eq!(back.len(), 4);
    }
}
