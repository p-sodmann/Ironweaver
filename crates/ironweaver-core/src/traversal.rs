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
        visited: IxSet<NodeIx>,
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

    let mut w = Walk { order: Vec::new(), visited: IxSet::default(), stack: Vec::new(), meter, direction };
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
    let mut visited = IxSet::default();
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

/// Parent links of one side of a bidirectional search (`None` for the side's
/// starting node).
type Parents = IxMap<NodeIx, Option<NodeIx>>;

/// Shortest path (fewest edges) from `source` to `target` following edges in
/// `direction`, using only edges for which `edge_ok` returns true. Returns
/// the nodes on the path (both ends included), or `None` if there is no path
/// of at most `max_depth` edges.
///
/// Grows one frontier from each end and stops when they meet; on graphs
/// whose BFS frontier fans out quickly this explores roughly the square root
/// of what a one-sided search does. Each round expands one complete level of
/// the smaller frontier, so the first meeting is a shortest path. Among
/// several shortest paths the one returned is not specified.
pub fn bidirectional_bfs<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    max_depth: Option<usize>,
    direction: Direction,
    mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
) -> Result<Option<Vec<NodeIx>>, X> {
    if source == target {
        return Ok(Some(vec![source]));
    }

    let mut fwd = Parents::default();
    fwd.insert(source, None);
    let mut bwd = Parents::default();
    bwd.insert(target, None);
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
        let (frontier, this, other, dir) = if forward {
            (&mut fwd_frontier, &mut fwd, &bwd, direction)
        } else {
            (&mut bwd_frontier, &mut bwd, &fwd, direction.reversed())
        };

        let mut next = Vec::new();
        let mut meeting = None;
        'level: for &ix in frontier.iter() {
            for (e, neighbor) in g.neighbors(ix, dir) {
                if stop.poll() {
                    return Ok(None);
                }
                if this.contains_key(&neighbor) || !edge_ok(e, g.edge_ref(e))? {
                    continue;
                }
                this.insert(neighbor, Some(ix));
                if other.contains_key(&neighbor) {
                    meeting = Some(neighbor);
                    break 'level;
                }
                next.push(neighbor);
            }
        }

        if let Some(m) = meeting {
            return Ok(Some(reconstruct(&fwd, &bwd, m)));
        }
        *frontier = next;
        if forward {
            fwd_depth += 1;
        } else {
            bwd_depth += 1;
        }
    }
    Ok(None)
}

/// Path source -> meeting -> target from the two parent maps.
fn reconstruct(fwd: &Parents, bwd: &Parents, meeting: NodeIx) -> Vec<NodeIx> {
    let mut path = Vec::new();
    let mut k = Some(meeting);
    while let Some(cur) = k {
        path.push(cur);
        k = fwd[&cur];
    }
    path.reverse();
    let mut k = bwd[&meeting];
    while let Some(cur) = k {
        path.push(cur);
        k = bwd[&cur];
    }
    path
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
