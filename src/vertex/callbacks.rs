// vertex/callbacks.rs

use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

/// Call every callback in `callbacks` with `args`, in order. A callback
/// returning `False` stops the remaining ones.
///
/// Node/edge add callbacks get `(vertex, node_or_edge)`; update callbacks
/// get `(vertex, node_or_edge, key, new_value, old_value)`.
pub fn fire(callbacks: &Bound<'_, PyList>, args: Bound<'_, PyTuple>) -> PyResult<()> {
    for callback in callbacks.iter() {
        let result = callback.call1(args.clone())?;
        let should_continue: bool = result.extract().unwrap_or(true);
        if !should_continue {
            break;
        }
    }
    Ok(())
}
