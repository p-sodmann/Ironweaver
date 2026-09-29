// vertex/serialization.rs

use ironweaver_core::format::{self, GraphWriter, LoadGraph};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyString};
use std::cell::RefCell;
use std::io::Write;

use super::Vertex;
use crate::convert::{dict_to_json, to_attrs, PyCodec, Strings};
use crate::data::{EdgeData, NodeData};
use crate::errors::Error;
use crate::gc_pause::GcPause;

fn runtime_error(context: &str, e: impl std::fmt::Display) -> PyErr {
    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}: {}", context, e))
}

fn codec<'py>(vertex: &Vertex, py: Python<'py>, half: bool) -> PyCodec<'py> {
    PyCodec { py, meta: vertex.meta.bind(py).clone(), half }
}

/// Save graph to JSON file (when file_path is provided) or return JSON string (when file_path is None).
/// Output is compact unless `pretty` is true.
pub fn save_to_json(vertex: &Vertex, py: Python<'_>, file_path: Option<String>, pretty: bool) -> PyResult<Py<PyAny>> {
    let codec = codec(vertex, py, false);
    let writer = GraphWriter::new(&vertex.graph, &codec);

    match file_path {
        Some(path) => {
            let json = writer.to_json(pretty).map_err(|e| runtime_error("Failed to save graph to JSON", e))?;
            py.detach(|| format::write_atomic(&path, |out| out.write_all(&json)))
                .map_err(|e| runtime_error("Failed to save graph to JSON", e))?;
            Ok(py.None())
        }
        None => {
            let json = writer.to_json(pretty).map_err(|e| runtime_error("Failed to serialize graph to JSON", e))?;
            // The serializer only ever writes valid UTF-8
            let text = std::str::from_utf8(&json).map_err(|e| runtime_error("Failed to serialize graph to JSON", e))?;
            Ok(PyString::new(py, text).into_any().unbind())
        }
    }
}

fn save_binary(vertex: &Vertex, py: Python<'_>, file_path: &str, half: bool) -> PyResult<()> {
    let codec = codec(vertex, py, half);
    let writer = GraphWriter::new(&vertex.graph, &codec);
    // The codec reads Python values, so this runs with the GIL held
    format::write_atomic(file_path, |out| writer.write_binary(out).map_err(std::io::Error::other))
        .map_err(|e| runtime_error("Failed to save graph to binary", e))
}

pub fn save_to_binary(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    save_binary(vertex, py, &file_path, false)
}

pub fn save_to_binary_f16(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    save_binary(vertex, py, &file_path, true)
}

/// Build a Vertex from a parsed document.
fn into_vertex(py: Python<'_>, doc: &LoadGraph<'_>) -> PyResult<Py<Vertex>> {
    let _gc = GcPause::new(py);
    let strings: RefCell<Strings<'_>> = RefCell::new(Strings::new());
    let graph = doc.build(
        |n| -> Result<NodeData, Error> {
            let mut s = strings.borrow_mut();
            Ok(NodeData::new(to_attrs(py, n.attr(), &mut s)?, to_attrs(py, n.meta(), &mut s)?))
        },
        |e| -> Result<EdgeData, Error> {
            let mut s = strings.borrow_mut();
            Ok(EdgeData::new(Some(e.id().into()), to_attrs(py, e.attr(), &mut s)?, to_attrs(py, e.meta(), &mut s)?))
        },
    );
    let graph = graph?;
    let mut vertex = Vertex::with_graph(py, graph);
    let meta = PyDict::new(py);
    for (key, value) in doc.meta().iter() {
        meta.set_item(key, crate::convert::to_python(py, value, &mut strings.borrow_mut())?)?;
    }
    vertex.meta = meta.unbind();
    Py::new(py, vertex)
}

/// Load graph from JSON file (when source is a string path) or from JSON string/dict (when source is a dict or JSON string)
pub fn load_from_json(py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<Py<Vertex>> {
    // Holds the document bytes when they don't come from a Python str
    let owned: Vec<u8>;

    let bytes: &[u8] = if let Ok(text) = source.cast::<PyString>() {
        // Borrow the string's UTF-8 buffer instead of copying it
        let text = text.to_str()?;
        if text.trim_start().starts_with('{') {
            // Looks like a JSON string
            text.as_bytes()
        } else {
            // Treat as file path
            owned = py
                .detach(|| std::fs::read(text))
                .map_err(|e| runtime_error("Failed to load graph from JSON file", e))?;
            &owned
        }
    } else if let Ok(dict) = source.cast::<PyDict>() {
        // Encode the dict as JSON bytes and use the same fast parser
        owned = dict_to_json(dict.as_any()).map_err(|e| runtime_error("Failed to parse dict as graph", e))?;
        &owned
    } else {
        return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
            "source must be a file path (str), JSON string (str), or dict",
        ));
    };

    let doc =
        py.detach(|| LoadGraph::from_json_slice(bytes)).map_err(|e| runtime_error("Failed to parse graph JSON", e))?;
    into_vertex(py, &doc)
}

pub fn load_from_binary(py: Python<'_>, file_path: String) -> PyResult<Py<Vertex>> {
    let bytes =
        py.detach(|| std::fs::read(&file_path)).map_err(|e| runtime_error("Failed to load graph from binary", e))?;
    let doc = py
        .detach(|| LoadGraph::from_binary_slice(&bytes))
        .map_err(|e| runtime_error("Failed to load graph from binary", e))?;
    into_vertex(py, &doc)
}
