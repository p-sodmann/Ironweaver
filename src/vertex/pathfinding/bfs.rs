// vertex/pathfinding/bfs.rs

use pyo3::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};

use super::super::algorithms::bidirectional::{bidirectional_bfs, is_library_built};
use super::super::core::Vertex;
use super::super::subgraph::neighbors;
use super::{PathMethod, PathQuery, PathResult};

pub const METHOD: PathMethod = PathMethod {
    name: "bfs",
    description: "Fewest edges (bidirectional breadth-first search); ignores weights. \
                  max_cost or max_depth limit the number of edges.",
    options: &["max_depth"],
    weighted: false,
    find,
};

fn find(py: Python<'_>, _vertex: &Vertex, q: &PathQuery<'_>) -> PyResult<Option<PathResult>> {
    // max_cost counts edges here, so it is a depth limit too.
    let max_depth = match (q.options.get::<usize>("max_depth")?, q.max_cost) {
        (d, None) => d,
        (None, Some(c)) => Some(c.floor() as usize),
        (Some(d), Some(c)) => Some(d.min(c.floor() as usize)),
    };
    let done = |ids: Vec<String>| {
        let cost = (ids.len() - 1) as f64;
        Ok(Some(PathResult { node_ids: ids, cost, expanded: None }))
    };

    if q.source_id == q.target_id {
        return done(vec![q.source_id.clone()]);
    }

    // Graphs built through the library keep edges/inverse_edges in sync, so
    // they can be searched from both ends at once.
    if is_library_built(py, &q.source) {
        let path = bidirectional_bfs(py, &q.source, &q.target, max_depth, q.direction, |_, _| Ok(true))?;
        return match path {
            Some(nodes) => done(nodes.iter().map(|n| n.borrow(py).id.clone()).collect()),
            None => Ok(None),
        };
    }

    // Hand-built graphs: one-sided BFS over outgoing/incoming lists only
    let mut visited = HashSet::<String>::new();
    let mut queue = VecDeque::new();
    let mut parent_map = HashMap::<String, String>::new();
    visited.insert(q.source_id.clone());
    queue.push_back((q.source.clone_ref(py), 0usize));

    while let Some((current_node, current_depth)) = queue.pop_front() {
        if max_depth.map_or(false, |d| current_depth >= d) {
            continue;
        }
        let current_id = current_node.borrow(py).id.clone();
        for (_edge, neighbor) in neighbors(py, &current_node, q.direction) {
            let to_id = neighbor.borrow(py).id.clone();
            if visited.insert(to_id.clone()) {
                parent_map.insert(to_id.clone(), current_id.clone());
                if to_id == q.target_id {
                    let mut ids = vec![to_id];
                    while let Some(parent) = parent_map.get(ids.last().unwrap()) {
                        ids.push(parent.clone());
                    }
                    ids.reverse();
                    return done(ids);
                }
                queue.push_back((neighbor, current_depth + 1));
            }
        }
    }
    Ok(None)
}
