// vertex/serialization.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyString};
use std::fs::File;
use std::io::BufWriter;
use crate::serialization::{dict_to_json, GraphView, LoadGraph};
use crate::gc_pause::GcPause;
use super::Vertex;

fn runtime_error(context: &str, e: impl std::fmt::Display) -> PyErr {
    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}: {}", context, e))
}

/// Save graph to JSON file (when file_path is provided) or return JSON string (when file_path is None).
/// Output is compact unless `pretty` is true.
pub fn save_to_json(
    vertex: &Vertex,
    py: Python<'_>,
    file_path: Option<String>,
    pretty: bool,
) -> PyResult<Py<PyAny>> {
    let view = GraphView::new(py, vertex, false);

    match file_path {
        Some(path) => {
            let json = view.to_json(pretty).map_err(|e| runtime_error("Failed to save graph to JSON", e))?;
            py.allow_threads(|| std::fs::write(&path, json))
                .map_err(|e| runtime_error("Failed to save graph to JSON", e))?;
            Ok(py.None())
        }
        None => {
            let json = view.to_json(pretty).map_err(|e| runtime_error("Failed to serialize graph to JSON", e))?;
            // The serializer only ever writes valid UTF-8
            let text = std::str::from_utf8(&json).map_err(|e| runtime_error("Failed to serialize graph to JSON", e))?;
            Ok(PyString::new(py, text).into_any().unbind())
        }
    }
}

fn save_binary(vertex: &Vertex, py: Python<'_>, file_path: &str, half: bool) -> PyResult<()> {
    let view = GraphView::new(py, vertex, half);
    let file = File::create(file_path).map_err(|e| runtime_error("Failed to save graph to binary", e))?;
    let mut writer = BufWriter::new(file);
    view.write_binary(&mut writer)
        .and_then(|_| Ok(std::io::Write::flush(&mut writer)?))
        .map_err(|e| runtime_error("Failed to save graph to binary", e))
}

pub fn save_to_binary(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    save_binary(vertex, py, &file_path, false)
}

pub fn save_to_binary_f16(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    save_binary(vertex, py, &file_path, true)
}

/// Load graph from JSON file (when source is a string path) or from JSON string/dict (when source is a dict or JSON string)
pub fn load_from_json(py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<Py<Vertex>> {
    // Holds the document bytes when they don't come from a Python str
    let owned: Vec<u8>;

    let bytes: &[u8] = if let Ok(text) = source.downcast::<PyString>() {
        // Borrow the string's UTF-8 buffer instead of copying it
        let text = text.to_str()?;
        if text.trim_start().starts_with('{') {
            // Looks like a JSON string
            text.as_bytes()
        } else {
            // Treat as file path
            owned = py
                .allow_threads(|| std::fs::read(text))
                .map_err(|e| runtime_error("Failed to load graph from JSON file", e))?;
            &owned
        }
    } else if let Ok(dict) = source.downcast::<PyDict>() {
        // Encode the dict as JSON bytes and use the same fast parser
        owned = dict_to_json(dict.as_any()).map_err(|e| runtime_error("Failed to parse dict as graph", e))?;
        &owned
    } else {
        return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
            "source must be a file path (str), JSON string (str), or dict"
        ));
    };

    let graph = py
        .allow_threads(|| LoadGraph::from_json_slice(bytes))
        .map_err(|e| runtime_error("Failed to parse graph JSON", e))?;
    let _gc = GcPause::new(py);
    graph.into_vertex(py)
}

pub fn load_from_binary(py: Python<'_>, file_path: String) -> PyResult<Py<Vertex>> {
    let bytes = py
        .allow_threads(|| std::fs::read(&file_path))
        .map_err(|e| runtime_error("Failed to load graph from binary", e))?;
    let graph = py
        .allow_threads(|| LoadGraph::from_binary_slice(&bytes))
        .map_err(|e| runtime_error("Failed to load graph from binary", e))?;
    let _gc = GcPause::new(py);
    graph.into_vertex(py)
}
