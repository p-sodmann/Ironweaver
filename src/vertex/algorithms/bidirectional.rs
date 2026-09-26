// vertex/algorithms/bidirectional.rs
//
// Bidirectional breadth-first search: grow one frontier from the source and
// one from the target (walking edges backwards) and stop when they meet.
// On graphs where a BFS frontier fans out quickly this explores roughly the
// square root of what a one-sided BFS does.
//
// The backward side walks `inverse_edges` (or `edges` for `direction="in"`),
// so it relies on those lists mirroring each other. Every library API keeps
// them in sync; hand-built graphs (Node/Edge constructors) may not, so callers
// only use this when the start node was created by a Vertex (it has a
// `vertex` back-reference) and fall back to a forward BFS otherwise.

use pyo3::prelude::*;
use std::collections::HashMap;
use crate::{Edge, Node};
use super::super::subgraph::{neighbors, Direction};

/// Entry in a visited map: the node itself and the key of the node it was
/// reached from (`None` for the side's starting node).
type Visited = HashMap<usize, (Py<Node>, Option<usize>)>;

fn key(node: &Py<Node>) -> usize {
    node.as_ptr() as usize
}

/// Whether a node was created by a Vertex (and so has consistent
/// `edges` / `inverse_edges`).
pub fn is_library_built(py: Python<'_>, node: &Py<Node>) -> bool {
    node.borrow(py).vertex.is_some()
}

/// Shortest path from `source` to `target` following edges in `direction`,
/// using only edges for which `edge_ok` returns true. Returns the nodes on
/// the path (both ends included), or `None` if there is no path of at most
/// `max_depth` edges.
///
/// Each round expands one complete level of the smaller frontier. The first
/// meeting found is a shortest path: before the round no node was reached by
/// both sides, so every path has at least `depth_fwd + depth_bwd + 1` edges,
/// and the meeting path has exactly that many. Among several shortest paths
/// the one returned is not specified.
pub fn bidirectional_bfs<F>(
    py: Python<'_>,
    source: &Py<Node>,
    target: &Py<Node>,
    max_depth: Option<usize>,
    direction: Direction,
    mut edge_ok: F,
) -> PyResult<Option<Vec<Py<Node>>>>
where
    F: FnMut(Python<'_>, &Py<Edge>) -> PyResult<bool>,
{
    if key(source) == key(target) {
        return Ok(Some(vec![source.clone_ref(py)]));
    }

    let mut fwd: Visited = HashMap::new();
    let mut bwd: Visited = HashMap::new();
    fwd.insert(key(source), (source.clone_ref(py), None));
    bwd.insert(key(target), (target.clone_ref(py), None));
    let mut fwd_frontier = vec![source.clone_ref(py)];
    let mut bwd_frontier = vec![target.clone_ref(py)];
    let (mut fwd_depth, mut bwd_depth) = (0usize, 0usize);

    while !fwd_frontier.is_empty() && !bwd_frontier.is_empty() {
        // The next meeting would give a path of fwd_depth + bwd_depth + 1 edges
        if max_depth.map_or(false, |d| fwd_depth + bwd_depth >= d) {
            return Ok(None);
        }

        let forward = fwd_frontier.len() <= bwd_frontier.len();
        let (frontier, this, other, dir) = if forward {
            (&mut fwd_frontier, &mut fwd, &bwd, direction)
        } else {
            (&mut bwd_frontier, &mut bwd, &fwd, direction.reversed())
        };

        let mut next = Vec::new();
        let mut meeting: Option<usize> = None;
        'level: for node in frontier.iter() {
            let node_key = key(node);
            for (edge, neighbor) in neighbors(py, node, dir) {
                let k = key(&neighbor);
                if this.contains_key(&k) || !edge_ok(py, &edge)? {
                    continue;
                }
                this.insert(k, (neighbor.clone_ref(py), Some(node_key)));
                if other.contains_key(&k) {
                    meeting = Some(k);
                    break 'level;
                }
                next.push(neighbor);
            }
        }

        if let Some(m) = meeting {
            return Ok(Some(reconstruct(py, &fwd, &bwd, m)));
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
fn reconstruct(py: Python<'_>, fwd: &Visited, bwd: &Visited, meeting: usize) -> Vec<Py<Node>> {
    let mut path = Vec::new();
    let mut k = Some(meeting);
    while let Some(cur) = k {
        let (node, parent) = &fwd[&cur];
        path.push(node.clone_ref(py));
        k = *parent;
    }
    path.reverse();
    let mut k = bwd[&meeting].1;
    while let Some(cur) = k {
        let (node, parent) = &bwd[&cur];
        path.push(node.clone_ref(py));
        k = *parent;
    }
    path
}
