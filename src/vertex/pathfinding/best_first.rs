// vertex/pathfinding/best_first.rs
//
// Best-first search shared by Dijkstra (no heuristic) and A*: nodes are
// settled in order of f = g + h, where g is the cheapest known cost from the
// source and h the heuristic estimate to the target. A node can be reopened
// when a cheaper route to it turns up, so the path is optimal for any
// admissible heuristic (one that never overestimates), consistent or not.

use pyo3::prelude::*;
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap};

use super::super::core::Vertex;
use super::super::subgraph::neighbors;
use super::heuristic::Heuristic;
use super::{PathQuery, PathResult};
use crate::Node;

fn key(node: &Py<Node>) -> usize {
    node.as_ptr() as usize
}

struct Info {
    node: Py<Node>,
    /// Cheapest known cost from the source (infinite until reached within max_cost).
    g: f64,
    /// Heuristic estimate, computed once per node.
    h: f64,
    parent: Option<usize>,
}

struct State {
    f: f64,
    g: f64,
    key: usize,
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

/// Cheapest path from `q.source` to `q.target` within `q.max_cost`, walking
/// only nodes that belong to `vertex`.
pub fn search<'py>(
    py: Python<'py>,
    vertex: &Vertex,
    q: &PathQuery<'py>,
    heuristic: &Heuristic<'py>,
) -> PyResult<Option<PathResult>> {
    let target = key(&q.target);
    let mut info: HashMap<usize, Info> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut expanded = 0usize;

    let h0 = heuristic.estimate(py, &q.source)?;
    let start = key(&q.source);
    info.insert(start, Info { node: q.source.clone_ref(py), g: 0.0, h: h0, parent: None });
    if q.max_cost.map_or(true, |limit| h0 <= limit) {
        heap.push(State { f: h0, g: 0.0, key: start });
    }

    while let Some(State { g, key: current, .. }) = heap.pop() {
        let node = {
            let i = &info[&current];
            if g > i.g {
                continue; // stale entry: a cheaper route was found later
            }
            i.node.clone_ref(py)
        };
        if current == target {
            let mut ids = Vec::new();
            let mut k = Some(current);
            while let Some(c) = k {
                let i = &info[&c];
                ids.push(i.node.borrow(py).id.clone());
                k = i.parent;
            }
            ids.reverse();
            return Ok(Some(PathResult { node_ids: ids, cost: g, expanded: Some(expanded + 1) }));
        }
        expanded += 1;

        for (edge, neighbor) in neighbors(py, &node, q.direction) {
            // Resolve through the vertex so results of bfs()/traverse(), whose
            // nodes still point at the full graph, stay within their nodes.
            let next = match vertex.nodes.get(&neighbor.borrow(py).id) {
                Some(n) => n,
                None => continue,
            };
            let ng = g + q.cost.cost(py, &edge)?;
            let nk = key(next);
            let slot = match info.entry(nk) {
                Entry::Occupied(o) => {
                    if ng >= o.get().g {
                        continue;
                    }
                    o.into_mut()
                }
                Entry::Vacant(v) => {
                    let h = heuristic.estimate(py, next)?;
                    v.insert(Info { node: next.clone_ref(py), g: f64::INFINITY, h, parent: None })
                }
            };
            let f = ng + slot.h;
            if q.max_cost.map_or(false, |limit| f > limit) {
                continue; // the estimate is a lower bound, so this can't fit
            }
            slot.g = ng;
            slot.parent = Some(current);
            heap.push(State { f, g: ng, key: nk });
        }
    }
    Ok(None)
}
