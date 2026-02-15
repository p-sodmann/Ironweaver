// edge.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use pyo3::class::basic::CompareOp;
use std::collections::HashMap;
use crate::Node;


#[pyclass]
pub struct Edge {
    #[pyo3(get, set)]
    pub id: Option<String>,
    #[pyo3(get, set)]
    pub from_node: Py<Node>,
    #[pyo3(get, set)]
    pub to_node: Py<Node>,
    #[pyo3(get, set)]
    pub attr: HashMap<String, Py<PyAny>>,
    #[pyo3(get, set)]
    pub watched_by: Vec<Py<PyAny>>,
    #[pyo3(get, set)]
    pub meta: HashMap<String, Py<PyAny>>,
    #[pyo3(get, set)]
    pub on_meta_change_callbacks: Vec<Py<PyAny>>,
    /// Callbacks fired when an attribute changes via ``attr_set``.
    /// Shared with the owning ``Vertex.on_edge_update_callbacks`` by reference.
    #[pyo3(get, set)]
    pub on_update_callbacks: Py<PyList>,
    /// Back-reference to the owning Vertex (set during ``add_edge``).
    #[pyo3(get)]
    pub vertex: Option<Py<PyAny>>,
}


#[pymethods]
impl Edge {
    #[new]
    pub fn new(
        py: Python<'_>,
        from_node: Py<Node>,
        to_node: Py<Node>,
        attr: Option<HashMap<String, Py<PyAny>>>,
        id: Option<String>
    ) -> Self {
        Edge {
            id,
            from_node,
            to_node,
            attr: attr.unwrap_or_default(),
            watched_by: Vec::new(),
            meta: HashMap::new(),
            on_meta_change_callbacks: Vec::new(),
            on_update_callbacks: PyList::empty(py).into(),
            vertex: None,
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let typ = self.attr.get("type")
            .and_then(|v| v.extract::<String>(py).ok())
            .unwrap_or_else(|| "unknown".to_string());
        let from_id = self.from_node.bind(py).getattr("id").ok().and_then(|obj| obj.extract::<String>().ok()).unwrap_or("?".to_string());
        let to_id = self.to_node.bind(py).getattr("id").ok().and_then(|obj| obj.extract::<String>().ok()).unwrap_or("?".to_string());
        Ok(format!("{}: {} --> {}", typ, from_id, to_id))
    }

    fn toJSON(&self, py: Python<'_>) -> Py<PyAny> {
        let dict = PyDict::new(py);
        for (k, v) in &self.attr {
            dict.set_item(k, v).unwrap();
        }
        dict.into()
    }

    /// Set a value in ``attr`` under ``key``.
    /// Fires ``on_update_callbacks`` if the value actually changed.
    fn attr_set(slf: PyRefMut<'_, Self>, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let old_value = slf.attr.get(&key).map(|v| v.clone_ref(py));

        // Check whether the value actually changed
        let mut changed = true;
        if let Some(ref old) = old_value {
            let eq_obj = old
                .bind(py)
                .rich_compare(value.bind(py), CompareOp::Eq)?;
            if eq_obj.is_truthy()? {
                changed = false;
            }
        }

        let callbacks = slf.on_update_callbacks.clone_ref(py);
        let vertex_ref = slf.vertex.as_ref().map(|v| v.clone_ref(py));
        let self_handle: Py<Edge> = slf.into();

        // Insert the new value
        {
            let mut edge_ref = self_handle.bind(py).borrow_mut();
            edge_ref.attr.insert(key.clone(), value.clone_ref(py));
        }

        // Fire callbacks if changed
        if changed {
            let cb_list = callbacks.bind(py);
            if cb_list.len() > 0 {
                for callback in cb_list.iter() {
                    let cb: Py<PyAny> = callback.into();
                    let result = cb.call1(
                        py,
                        (
                            vertex_ref.as_ref().map(|v| v.clone_ref(py)),
                            self_handle.clone_ref(py),
                            key.clone(),
                            value.clone_ref(py),
                            old_value.as_ref().map(|v| v.clone_ref(py)),
                        ),
                    )?;
                    let should_continue: bool = result.extract(py).unwrap_or(true);
                    if !should_continue {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Retrieve a value from ``attr`` by key.
    /// Returns ``None`` if the key does not exist.
    fn attr_get<'py>(&self, py: Python<'py>, key: String) -> Option<Py<PyAny>> {
        self.attr.get(&key).map(|v| v.clone_ref(py))
    }
}

