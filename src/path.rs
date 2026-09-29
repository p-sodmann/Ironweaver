use crate::{Edge, Node};
use pyo3::prelude::*;
use pyo3::{PyTraverseError, PyVisit};

/// A path: its nodes in order and the edges between them (`Node.paths`
/// returns these; `len(path)` is the number of edges).
#[pyclass]
pub struct Path {
    #[pyo3(get, set)]
    pub nodes: Vec<Py<Node>>,
    #[pyo3(get, set)]
    pub edges: Vec<Py<Edge>>,
}

#[pymethods]
impl Path {
    #[new]
    #[pyo3(signature = (nodes=None, edges=None))]
    fn new(nodes: Option<Vec<Py<Node>>>, edges: Option<Vec<Py<Edge>>>) -> Self {
        Path { nodes: nodes.unwrap_or_default(), edges: edges.unwrap_or_default() }
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for n in &self.nodes {
            visit.call(n)?;
        }
        for e in &self.edges {
            visit.call(e)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }

    /// Number of edges.
    fn __len__(&self) -> usize {
        self.edges.len()
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        format!("Path({:?})", self.ids(py))
    }

    /// The node ids in order.
    fn ids(&self, py: Python<'_>) -> Vec<String> {
        self.nodes
            .iter()
            .filter_map(|n| n.bind(py).getattr("id").ok().and_then(|id| id.extract::<String>().ok()))
            .collect()
    }

    /// Return the node ids in order.
    #[allow(non_snake_case)]
    fn toJSON(&self, py: Python<'_>) -> Vec<String> {
        self.ids(py)
    }
}
