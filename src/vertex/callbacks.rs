// vertex/callbacks.rs

use pyo3::class::basic::CompareOp;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use crate::Vertex;

/// Compare before mutating an attribute. Missing and existing `None` differ.
/// Equality and truth testing can run Python, so no graph borrow may be held.
pub(crate) fn value_changed(old: Option<&Bound<'_, PyAny>>, new: &Bound<'_, PyAny>) -> PyResult<bool> {
    match old {
        Some(old) => Ok(!old.rich_compare(new, CompareOp::Eq)?.is_truthy()?),
        None => Ok(true),
    }
}

/// Dispatch an update after mutation and node reindexing. The caller fetches
/// the current callback list after those operations and releases its borrow.
pub(crate) fn fire_update(
    callbacks: &Bound<'_, PyList>,
    vertex: &Py<Vertex>,
    item: Py<PyAny>,
    key: String,
    new: Py<PyAny>,
    old: Option<Py<PyAny>>,
) -> PyResult<()> {
    let py = callbacks.py();
    let args = PyTuple::new(
        py,
        [
            vertex.clone_ref(py).into_any(),
            item,
            key.into_pyobject(py)?.into_any().unbind(),
            new,
            old.unwrap_or_else(|| py.None()),
        ],
    )?;
    fire(callbacks, args)
}

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
