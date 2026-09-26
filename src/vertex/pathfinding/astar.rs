// vertex/pathfinding/astar.rs

use pyo3::prelude::*;

use super::super::core::Vertex;
use super::heuristic::Heuristic;
use super::{best_first, PathMethod, PathQuery, PathResult};

pub const METHOD: PathMethod = PathMethod {
    name: "astar",
    description: "Cheapest path by edge weight (A*), guided by node coordinates \
                  (heuristic='euclidean'|'manhattan', coords=...) or precomputed \
                  estimates (distances='<vertex.meta key>'). Estimates must not \
                  overestimate the remaining cost.",
    options: &["heuristic", "coords", "distances"],
    weighted: true,
    find,
};

fn find<'py>(py: Python<'py>, vertex: &Vertex, q: &PathQuery<'py>) -> PyResult<Option<PathResult>> {
    let heuristic = Heuristic::from_query(py, vertex, q)?;
    best_first::search(py, vertex, q, &heuristic)
}
