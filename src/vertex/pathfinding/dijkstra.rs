// vertex/pathfinding/dijkstra.rs

use pyo3::prelude::*;

use super::super::core::Vertex;
use super::heuristic::Heuristic;
use super::{best_first, PathMethod, PathQuery, PathResult};

pub const METHOD: PathMethod = PathMethod {
    name: "dijkstra",
    description: "Cheapest path by edge weight (Dijkstra). Weights must be non-negative.",
    options: &[],
    weighted: true,
    find,
};

fn find(py: Python<'_>, vertex: &Vertex, q: &PathQuery<'_>) -> PyResult<Option<PathResult>> {
    best_first::search(py, vertex, q, &Heuristic::Zero)
}
