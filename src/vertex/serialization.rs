// vertex/serialization.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use crate::serialization::SerializableGraph;
use super::Vertex;

/// Save graph to JSON file (when file_path is provided) or return JSON string (when file_path is None)
pub fn save_to_json(vertex: &Vertex, py: Python<'_>, file_path: Option<String>) -> PyResult<Py<PyAny>> {
    let serializable_graph = SerializableGraph::from_vertex(py, vertex)?;
    
    match file_path {
        Some(path) => {
            serializable_graph.save_to_json(&path)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Failed to save graph to JSON: {}", e)
                ))?;
            Ok(py.None())
        }
        None => {
            let json_string = serializable_graph.to_json_string()
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Failed to serialize graph to JSON: {}", e)
                ))?;
            Ok(json_string.into_pyobject(py)?.into_any().unbind())
        }
    }
}

pub fn save_to_binary(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    let serializable_graph = SerializableGraph::from_vertex(py, vertex)?;
    serializable_graph.save_to_binary(&file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("Failed to save graph to binary: {}", e)
        ))?;
    Ok(())
}

pub fn save_to_binary_f16(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    let serializable_graph = SerializableGraph::from_vertex(py, vertex)?;
    serializable_graph.save_to_binary_f16(&file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("Failed to save graph to binary: {}", e)
        ))?;
    Ok(())
}

/// Load graph from JSON file (when source is a string path) or from JSON string/dict (when source is a dict or JSON string)
pub fn load_from_json(py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<Py<Vertex>> {
    let serializable_graph = if let Ok(path) = source.extract::<String>() {
        // Try to parse as JSON string first, if that fails treat as file path
        if path.trim().starts_with('{') {
            // Looks like a JSON string
            SerializableGraph::from_json_string(&path)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Failed to parse JSON string: {}", e)
                ))?
        } else {
            // Treat as file path
            SerializableGraph::load_from_json(&path)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Failed to load graph from JSON file: {}", e)
                ))?
        }
    } else if let Ok(dict) = source.downcast::<PyDict>() {
        // Convert Python dict to JSON string, then parse
        let json_module = py.import("json")?;
        let json_string: String = json_module.call_method1("dumps", (dict,))?.extract()?;
        SerializableGraph::from_json_string(&json_string)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("Failed to parse dict as graph: {}", e)
            ))?
    } else {
        return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
            "source must be a file path (str), JSON string (str), or dict"
        ));
    };
    
    let vertex = serializable_graph.to_vertex(py)?;
    Py::new(py, vertex)
}

pub fn load_from_binary(py: Python<'_>, file_path: String) -> PyResult<Py<Vertex>> {
    let serializable_graph = SerializableGraph::load_from_binary(&file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("Failed to load graph from binary: {}", e)
        ))?;
    let vertex = serializable_graph.to_vertex(py)?;
    Py::new(py, vertex)
}
