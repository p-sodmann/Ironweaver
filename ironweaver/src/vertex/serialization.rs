// vertex/serialization.rs

use pyo3::prelude::*;
use crate::serialization::SerializableGraph;
use super::Vertex;

pub fn save_to_json(vertex: &Vertex, py: Python<'_>, file_path: String) -> PyResult<()> {
    let serializable_graph = SerializableGraph::from_vertex(py, vertex)?;
    serializable_graph.save_to_json(&file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("Failed to save graph to JSON: {}", e)
        ))?;
    Ok(())
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

pub fn load_from_json(py: Python<'_>, file_path: String) -> PyResult<Py<Vertex>> {
    let serializable_graph = SerializableGraph::load_from_json(&file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("Failed to load graph from JSON: {}", e)
        ))?;
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
