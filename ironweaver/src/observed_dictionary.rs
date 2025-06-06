// observed_dictionary.rs

use pyo3::prelude::*;
use pyo3::class::basic::CompareOp;
use std::collections::HashMap;

#[pyclass]
#[derive(Default)]
pub struct ObservedDictionary {
    dict: HashMap<String, Py<PyAny>>,
    node: Option<Py<PyAny>>,
    callbacks: HashMap<String, Vec<Py<PyAny>>>,
}

#[pymethods]
impl ObservedDictionary {
    #[new]
    fn new(
        node: Option<Py<PyAny>>,
        callbacks: Option<HashMap<String, Vec<Py<PyAny>>>>,
    ) -> Self {
        ObservedDictionary {
            dict: HashMap::new(),
            node,
            callbacks: callbacks.unwrap_or_default(),
        }
    }

    fn __setitem__(&mut self, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let old_value = self.dict.get(&key).map(|v| v.clone_ref(py));

        // Determine whether the value actually changed using Python's equality
        let mut changed = true;
        if let Some(ref old) = old_value {
            let eq_obj = old
                .bind(py)
                .rich_compare(value.bind(py), CompareOp::Eq)?;
            if eq_obj.is_truthy()? {
                changed = false;
            }
        }

        self.dict.insert(key.clone(), value.clone_ref(py));

        if changed {
            if let Some(callbacks) = self.callbacks.get(&key) {
                for cb in callbacks {
                    cb.call1(
                        py,
                        (
                            self.node.as_ref().map(|n| n.clone_ref(py)),
                            key.clone(),
                            value.clone_ref(py),
                            old_value.as_ref().map(|v| v.clone_ref(py)),
                        ),
                    )?;
                }
            }
        }

        Ok(())
    }

    fn __getitem__(&self, py: Python<'_>, key: String) -> PyResult<Py<PyAny>> {
        self.dict
            .get(&key)
            .map(|v| v.clone_ref(py))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("Key '{}' not found", key)))
    }
}


