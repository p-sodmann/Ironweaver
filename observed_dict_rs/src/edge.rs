// edge.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use std::collections::HashMap;
use crate::Node;


#[pyclass]
pub struct Edge {
    #[pyo3(get, set)]
    pub id: Option<String>,
    #[pyo3(get, set)]
    pub from: Py<Node>,
    #[pyo3(get, set)]
    pub to: Py<Node>,
    #[pyo3(get, set)]
    pub attr: HashMap<String, Py<PyAny>>,
    #[pyo3(get, set)]
    pub watched_by: Vec<Py<PyAny>>,
}


#[pymethods]
impl Edge {
    #[new]
    pub fn new(
        from: Py<Node>,
        to: Py<Node>,
        attr: Option<HashMap<String, Py<PyAny>>>,
        id: Option<String>
    ) -> Self {
        Edge {
            id,
            from,
            to,
            attr: attr.unwrap_or_default(),
            watched_by: Vec::new(),
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let typ = self.attr.get("type")
            .and_then(|v| v.extract::<String>(py).ok())
            .unwrap_or_else(|| "unknown".to_string());
        let from_id = self.from.bind(py).getattr("id").ok().and_then(|obj| obj.extract::<String>().ok()).unwrap_or("?".to_string());
        let to_id = self.to.bind(py).getattr("id").ok().and_then(|obj| obj.extract::<String>().ok()).unwrap_or("?".to_string());
        Ok(format!("{}: {} --> {}", typ, from_id, to_id))
    }

    fn toJSON(&self, py: Python<'_>) -> Py<PyAny> {
        let dict = PyDict::new(py);
        for (k, v) in &self.attr {
            dict.set_item(k, v).unwrap();
        }
        dict.into()
    }
}

