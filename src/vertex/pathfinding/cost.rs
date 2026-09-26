// vertex/pathfinding/cost.rs

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

use crate::Edge;

/// How much traversing an edge costs.
pub enum EdgeCost {
    /// Every edge costs 1 (hop count).
    Unit,
    /// The edge attribute `key`, or `default` when the edge doesn't have it.
    Weighted { key: String, default: f64 },
}

impl EdgeCost {
    pub fn weighted(key: Option<String>, default: Option<f64>) -> Self {
        EdgeCost::Weighted {
            key: key.unwrap_or_else(|| "weight".to_string()),
            default: default.unwrap_or(1.0),
        }
    }

    /// Cost of `edge`; weights must be non-negative numbers.
    pub fn cost(&self, py: Python<'_>, edge: &Py<Edge>) -> PyResult<f64> {
        let (key, default) = match self {
            EdgeCost::Unit => return Ok(1.0),
            EdgeCost::Weighted { key, default } => (key, *default),
        };
        let w = match edge.borrow(py).attr.get(key) {
            Some(v) => v.extract::<f64>(py).map_err(|_| {
                PyTypeError::new_err(format!("Edge attribute '{}' must be a number", key))
            })?,
            None => default,
        };
        if !(w >= 0.0) {
            return Err(PyValueError::new_err(format!(
                "Edge weights must be non-negative, got {}",
                w
            )));
        }
        Ok(w)
    }
}
