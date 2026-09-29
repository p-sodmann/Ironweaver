// pathfinding/best_first.rs
//
// Best-first search shared by Dijkstra (no heuristic) and A*: nodes are
// settled in order of f = g + h, where g is the cheapest known cost from the
// source and h the heuristic estimate to the target. A node can be reopened
// when a cheaper route to it turns up, so the path is optimal for any
// admissible heuristic (one that never overestimates), consistent or not.

use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::BinaryHeap;

use super::{Heuristic, PathQuery, PathResult};
use crate::graph::IxMap;
use crate::{Attributes, Graph, GraphError, NodeIx};

struct Info {
    /// Cheapest known cost from the source (infinite until reached within max_cost).
    g: f64,
    /// Heuristic estimate, computed once per node.
    h: f64,
    parent: Option<NodeIx>,
}

struct State {
    f: f64,
    g: f64,
    node: NodeIx,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for State {}
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap: smallest f first, then largest g (closer
        // to the target) to break ties.
        other.f.total_cmp(&self.f).then(self.g.total_cmp(&other.g))
    }
}

/// Cheapest path from `source` to `target` within `max_cost`, guided by
/// `q.heuristic` if `use_heuristic` (otherwise Dijkstra).
pub fn search<N, E, X>(
    graph: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    q: &mut PathQuery<'_, N, X>,
    use_heuristic: bool,
) -> Result<Option<PathResult>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    let node = |ix: NodeIx| graph.node(ix).expect("search only visits live nodes");
    let mut zero = Heuristic::Zero;
    let PathQuery { direction, cost, max_cost, heuristic, .. } = q;
    let heuristic = if use_heuristic { heuristic } else { &mut zero };
    let (direction, max_cost) = (*direction, *max_cost);
    let mut info: IxMap<NodeIx, Info> = IxMap::default();
    let mut heap = BinaryHeap::new();
    let mut expanded = 0usize;

    let h0 = heuristic.estimate(node(source))?;
    info.insert(source, Info { g: 0.0, h: h0, parent: None });
    if max_cost.is_none_or(|limit| h0 <= limit) {
        heap.push(State { f: h0, g: 0.0, node: source });
    }

    let stop = crate::cancel::stop();
    while let Some(State { g, node: current, .. }) = heap.pop() {
        if stop.poll() {
            return Ok(None);
        }
        if g > info[&current].g {
            continue; // stale entry: a cheaper route was found later
        }
        if current == target {
            let mut nodes = Vec::new();
            let mut k = Some(current);
            while let Some(c) = k {
                nodes.push(c);
                k = info[&c].parent;
            }
            nodes.reverse();
            return Ok(Some(PathResult { nodes, cost: g, expanded: Some(expanded + 1) }));
        }
        expanded += 1;

        for (e, next) in graph.neighbors(current, direction) {
            let ng = g + cost.cost::<E, X>(&graph.edge_ref(e).data)?;
            let slot = match info.entry(next) {
                Entry::Occupied(o) => {
                    if ng >= o.get().g {
                        continue;
                    }
                    o.into_mut()
                }
                Entry::Vacant(v) => {
                    let h = heuristic.estimate(node(next))?;
                    v.insert(Info { g: f64::INFINITY, h, parent: None })
                }
            };
            let f = ng + slot.h;
            if max_cost.is_some_and(|limit| f > limit) {
                continue; // the estimate is a lower bound, so this can't fit
            }
            slot.g = ng;
            slot.parent = Some(current);
            heap.push(State { f, g: ng, node: next });
        }
    }
    Ok(None)
}
