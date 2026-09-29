// errors.rs
//
// Mapping core `GraphError`s to Python exceptions.

use ironweaver_core::GraphError;
use pyo3::exceptions::{PyKeyboardInterrupt, PyOverflowError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::PyErr;

/// The Python exception for a core error: `TypeError` for wrong types,
/// `RuntimeError` for format errors and stale handles, `KeyboardInterrupt`
/// for cancelled computations, `OverflowError` for size limits, `ValueError`
/// otherwise.
pub fn graph_error(e: GraphError) -> PyErr {
    match e {
        GraphError::InvalidType(msg) => PyTypeError::new_err(msg),
        GraphError::Format(msg) => PyRuntimeError::new_err(msg),
        GraphError::Stale => PyRuntimeError::new_err(e.to_string()),
        GraphError::Interrupted => PyKeyboardInterrupt::new_err(e.to_string()),
        GraphError::Capacity(msg) => PyOverflowError::new_err(msg),
        _ => PyValueError::new_err(e.to_string()),
    }
}

/// Error type threaded through core algorithms from the bindings: either a
/// Python exception raised by a callback / attribute lookup, or a core error.
/// (`PyErr: From<GraphError>` can't be implemented here: neither type is
/// local to this crate.)
pub struct Error(pub PyErr);

impl From<PyErr> for Error {
    fn from(e: PyErr) -> Self {
        Error(e)
    }
}

impl From<GraphError> for Error {
    fn from(e: GraphError) -> Self {
        Error(graph_error(e))
    }
}

impl From<Error> for PyErr {
    fn from(e: Error) -> Self {
        e.0
    }
}
