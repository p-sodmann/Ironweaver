use crate::Node;
use pyo3::prelude::*;
use pyo3::{PyTraverseError, PyVisit};

/// An ordered list of nodes. Reserved for future use: path algorithms return
/// a `Vertex` with the ordered ids in `meta["nodelist"]` instead.
#[pyclass]
pub struct Path {
    #[pyo3(get, set)]
    pub nodes: Vec<Py<Node>>,
}

#[pymethods]
impl Path {
    #[new]
    fn new(nodes: Option<Vec<Py<Node>>>) -> Self {
        Path { nodes: nodes.unwrap_or_default() }
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for n in &self.nodes {
            visit.call(n)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.nodes.clear();
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        let node_ids: Vec<String> = self
            .nodes
            .iter()
            .filter_map(|n| n.bind(py).getattr("id").ok().and_then(|id| id.extract::<String>().ok()))
            .collect();
        format!("Path({:?})", node_ids)
    }

    /// Return the node ids in order.
    #[allow(non_snake_case)]
    fn toJSON(&self, py: Python<'_>) -> Vec<String> {
        self.nodes
            .iter()
            .filter_map(|n| n.bind(py).getattr("id").ok().and_then(|id| id.extract::<String>().ok()))
            .collect()
    }
}
