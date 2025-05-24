use pyo3::prelude::*;
use crate::Node;

#[pyclass]
pub struct Path {
    #[pyo3(get, set)]
    pub nodes: Vec<Py<Node>>,
}

#[pymethods]
impl Path {
    #[new]
    fn new(nodes: Option<Vec<Py<Node>>>) -> Self {
        Path {
            nodes: nodes.unwrap_or_default(),
        }
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        let node_ids: Vec<String> = self.nodes
            .iter()
            .filter_map(|n| n.bind(py)
            .getattr("id")
            .ok()
            .map(|id| id.to_string()))
            .collect();
        format!("Path({:?})", node_ids)
    }

    fn toJSON(&self, py: Python<'_>) -> Vec<String> {
        self.nodes.iter()
            .filter_map(|n| n.bind(py)
            .getattr("id")
            .ok()
            .map(|id| id.to_string()))
            .collect()
    }
}
