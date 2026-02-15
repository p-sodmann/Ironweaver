// vertex/callbacks.rs

use pyo3::prelude::*;
use pyo3::types::PyList;
use crate::Node;
use crate::Edge;

/// Fire node-add callbacks stored on the Vertex.
///
/// Each callback receives `(vertex, node)` and may return `False` to stop
/// further callbacks from being invoked.
pub fn fire_node_add_callbacks(
    py: Python<'_>,
    callbacks_list: &Bound<'_, PyList>,
    vertex: Py<PyAny>,
    node: Py<Node>,
) -> PyResult<()> {
    for callback in callbacks_list.iter() {
        let cb: Py<PyAny> = callback.into();
        let result = cb.call1(py, (vertex.clone_ref(py), node.clone_ref(py)))?;
        let should_continue: bool = result.extract(py).unwrap_or(true);
        if !should_continue {
            break;
        }
    }
    Ok(())
}

/// Fire edge-add callbacks stored on the Vertex.
///
/// Each callback receives `(vertex, edge)` and may return `False` to stop
/// further callbacks from being invoked.
pub fn fire_edge_add_callbacks(
    py: Python<'_>,
    callbacks_list: &Bound<'_, PyList>,
    vertex: Py<PyAny>,
    edge: Py<Edge>,
) -> PyResult<()> {
    for callback in callbacks_list.iter() {
        let cb: Py<PyAny> = callback.into();
        let result = cb.call1(py, (vertex.clone_ref(py), edge.clone_ref(py)))?;
        let should_continue: bool = result.extract(py).unwrap_or(true);
        if !should_continue {
            break;
        }
    }
    Ok(())
}

/// Fire node-update callbacks when an attribute on a node changes.
///
/// Each callback receives `(vertex, node, key, new_value, old_value)` and may
/// return `False` to stop further callbacks from being invoked.
pub fn fire_node_update_callbacks(
    py: Python<'_>,
    callbacks_list: &Bound<'_, PyList>,
    vertex: Option<&Py<PyAny>>,
    node: Py<Node>,
    key: &str,
    new_value: &Py<PyAny>,
    old_value: Option<&Py<PyAny>>,
) -> PyResult<()> {
    for callback in callbacks_list.iter() {
        let cb: Py<PyAny> = callback.into();
        let result = cb.call1(
            py,
            (
                vertex.map(|v| v.clone_ref(py)),
                node.clone_ref(py),
                key.to_string(),
                new_value.clone_ref(py),
                old_value.map(|v| v.clone_ref(py)),
            ),
        )?;
        let should_continue: bool = result.extract(py).unwrap_or(true);
        if !should_continue {
            break;
        }
    }
    Ok(())
}

/// Fire edge-update callbacks when an attribute on an edge changes.
///
/// Each callback receives `(vertex, edge, key, new_value, old_value)` and may
/// return `False` to stop further callbacks from being invoked.
pub fn fire_edge_update_callbacks(
    py: Python<'_>,
    callbacks_list: &Bound<'_, PyList>,
    vertex: Option<&Py<PyAny>>,
    edge: Py<Edge>,
    key: &str,
    new_value: &Py<PyAny>,
    old_value: Option<&Py<PyAny>>,
) -> PyResult<()> {
    for callback in callbacks_list.iter() {
        let cb: Py<PyAny> = callback.into();
        let result = cb.call1(
            py,
            (
                vertex.map(|v| v.clone_ref(py)),
                edge.clone_ref(py),
                key.to_string(),
                new_value.clone_ref(py),
                old_value.map(|v| v.clone_ref(py)),
            ),
        )?;
        let should_continue: bool = result.extract(py).unwrap_or(true);
        if !should_continue {
            break;
        }
    }
    Ok(())
}
