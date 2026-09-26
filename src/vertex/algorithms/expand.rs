// vertex/algorithms/expand.rs

use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::{HashSet, VecDeque};
use crate::Node;
use super::super::core::Vertex;
use super::super::subgraph::{build_subgraph, neighbors, Direction};

pub fn expand(
    vertex: &Vertex,
    py: Python<'_>,
    source_vertex: &Vertex,
    depth: Option<usize>,
    direction: Option<&str>,
) -> PyResult<Py<Vertex>> {
    let expansion_depth = depth.unwrap_or(1);
    let direction = Direction::parse(direction)?;

    // A single multi-source BFS: every seed starts at depth 0 and all seeds
    // share one visited set. A node ends up in the result iff its distance
    // from the nearest seed is <= depth, which is exactly the union of the
    // per-seed BFS results, but costs O(V + E) instead of O(seeds * (V + E)).
    let mut discovered: HashSet<String> = vertex.nodes.keys().cloned().collect();
    let mut queue: VecDeque<(Py<Node>, usize)> = VecDeque::new();

    for seed_id in vertex.nodes.keys() {
        if let Some(source_node) = source_vertex.nodes.get(seed_id) {
            queue.push_back((source_node.clone_ref(py), 0));
        }
    }

    while let Some((current_node, current_depth)) = queue.pop_front() {
        if current_depth >= expansion_depth {
            continue;
        }
        for (_edge, neighbor) in neighbors(py, &current_node, direction) {
            let to_id = neighbor.borrow(py).id.clone();
            if discovered.insert(to_id.clone()) && current_depth + 1 < expansion_depth {
                if let Some(source_neighbor) = source_vertex.nodes.get(&to_id) {
                    queue.push_back((source_neighbor.clone_ref(py), current_depth + 1));
                }
            }
        }
    }

    build_subgraph(py, source_vertex, &discovered, PyDict::new(py).into(), false)
}
