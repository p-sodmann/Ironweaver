// vertex/algorithms/shortest_path_dijkstra.rs

use pyo3::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use super::super::core::Vertex;
use super::super::subgraph::{neighbors, Direction};
use super::shortest_path_bfs::path_vertex;

// Min-heap entry ordered by cost.
struct State {
    cost: f64,
    id: String,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.cost.total_cmp(&other.cost) == Ordering::Equal
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
        // Reversed so BinaryHeap pops the smallest cost first
        other.cost.total_cmp(&self.cost)
    }
}

pub fn shortest_path_dijkstra(
    vertex: &Vertex,
    py: Python<'_>,
    root_node_id: String,
    target_node_id: String,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
) -> PyResult<Py<Vertex>> {
    let weight_key = weight.unwrap_or_else(|| "weight".to_string());
    let default_weight = default_weight.unwrap_or(1.0);
    let direction = Direction::parse(direction)?;

    if !vertex.nodes.contains_key(&root_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Root node with id '{}' not found", root_node_id)
        ));
    }
    if !vertex.nodes.contains_key(&target_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Target node with id '{}' not found", target_node_id)
        ));
    }

    let mut dist: HashMap<String, f64> = HashMap::new();
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(root_node_id.clone(), 0.0);
    heap.push(State { cost: 0.0, id: root_node_id.clone() });

    while let Some(State { cost, id }) = heap.pop() {
        if id == target_node_id {
            let mut path_ids = vec![id];
            while let Some(p) = parent.get(path_ids.last().unwrap()) {
                path_ids.push(p.clone());
            }
            path_ids.reverse();
            let result = path_vertex(py, vertex, path_ids)?;
            result.borrow(py).meta.bind(py).set_item("cost", cost)?;
            return Ok(result);
        }
        if cost > dist.get(&id).copied().unwrap_or(f64::INFINITY) {
            continue; // stale heap entry
        }
        // Only nodes that belong to this vertex are expanded
        let node = match vertex.nodes.get(&id) {
            Some(n) => n.clone_ref(py),
            None => continue,
        };

        for (edge, neighbor) in neighbors(py, &node, direction) {
            let w = match edge.borrow(py).attr.get(&weight_key) {
                Some(v) => v.extract::<f64>(py).map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err(format!(
                        "Edge attribute '{}' must be a number", weight_key
                    ))
                })?,
                None => default_weight,
            };
            if !(w >= 0.0) {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Edge weights must be non-negative, got {}", w
                )));
            }
            let next_cost = cost + w;
            if let Some(limit) = max_cost {
                if next_cost > limit {
                    continue;
                }
            }
            let to_id = neighbor.borrow(py).id.clone();
            if next_cost < dist.get(&to_id).copied().unwrap_or(f64::INFINITY) {
                dist.insert(to_id.clone(), next_cost);
                parent.insert(to_id.clone(), id.clone());
                heap.push(State { cost: next_cost, id: to_id });
            }
        }
    }

    Err(pyo3::exceptions::PyValueError::new_err(
        format!("Target node '{}' not reachable from '{}' within max_cost {:?}",
                target_node_id, root_node_id, max_cost)
    ))
}
