// vertex/core.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use std::collections::HashMap;

use crate::{Node, Edge};

// Import the helper modules as sibling modules
use super::manipulation;
use super::serialization;
use super::analysis;
use super::algorithms;

#[pyclass]
pub struct Vertex {
    #[pyo3(get, set)]
    pub nodes: HashMap<String, Py<Node>>,
    #[pyo3(get, set)]
    pub meta: Py<PyDict>,
    #[pyo3(get, set)]
    pub on_node_add_callbacks: Py<PyList>,
    #[pyo3(get, set)]
    pub on_edge_add_callbacks: Py<PyList>,
}

#[pymethods]
impl Vertex {
    #[new]
    fn new(py: Python<'_>) -> Self {
        Vertex {
            nodes: HashMap::new(),
            meta: PyDict::new(py).into(),
            on_node_add_callbacks: PyList::empty(py).into(),
            on_edge_add_callbacks: PyList::empty(py).into(),
        }
    }

    /// Create a new graph with existing nodes
    #[staticmethod]
    pub fn from_nodes(py: Python<'_>, nodes: HashMap<String, Py<Node>>) -> Self {
        Vertex { 
            nodes,
            meta: PyDict::new(py).into(),
            on_node_add_callbacks: PyList::empty(py).into(),
            on_edge_add_callbacks: PyList::empty(py).into(),
        }
    }

    /// Create a new graph with existing nodes and traversal path
    #[staticmethod]
    pub fn from_nodes_with_path(py: Python<'_>, nodes: HashMap<String, Py<Node>>, nodelist: Vec<String>) -> PyResult<Self> {
        let meta = PyDict::new(py);
        meta.set_item("nodelist", nodelist)?;
        
        Ok(Vertex { 
            nodes,
            meta: meta.into(),
            on_node_add_callbacks: PyList::empty(py).into(),
            on_edge_add_callbacks: PyList::empty(py).into(),
        })
    }

    fn __getitem__(&self, py: Python<'_>, key: String) -> PyResult<Py<Node>> {
        self.nodes
            .get(&key)
            .map(|n| n.clone_ref(py))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(key))
    }

    fn keys(&self) -> Vec<String> {
        self.nodes.keys().cloned().collect()
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        let keys: Vec<String> = self.nodes
            .values()
            .filter_map(|n| n.bind(py).getattr("id").ok().and_then(|o| o.extract::<String>().ok()))
            .collect();
        format!("Vertex({})", keys.join(", "))
    }

    fn toJSON(&self, py: Python<'_>) -> Py<PyAny> {
        let dict = PyDict::new(py);
        for (node_id, node) in &self.nodes {
            dict.set_item(node_id, node).unwrap();
        }
        dict.into()
    }

    /// Check if a node with the given ID exists
    /// 
    /// Args:
    ///     id (str): The node ID to check
    ///     
    /// Returns:
    ///     bool: True if the node exists, False otherwise
    fn has_node(&self, id: String) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Get the number of nodes in the graph
    /// 
    /// Returns:
    ///     int: The number of nodes
    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // Manipulation methods
    /// Add a new node to the graph
    /// 
    /// Args:
    ///     id (str): Unique identifier for the node
    ///     attr (dict, optional): Attributes for the node
    ///     
    /// Returns:
    ///     Node: The created node
    ///     
    /// Raises:
    ///     ValueError: If a node with the same ID already exists
    fn add_node(
        mut slf: PyRefMut<'_, Self>,
        py: Python<'_>, 
        id: String, 
        attr: Option<HashMap<String, Py<PyAny>>>
    ) -> PyResult<Py<Node>> {
        // First create the node
        let node = manipulation::add_node(&mut slf, py, id, attr)?;
        
        // Get callbacks from PyList
        let callbacks_list = slf.on_node_add_callbacks.bind(py);
        let mut callbacks_to_call: Vec<Py<PyAny>> = Vec::new();
        for callback in callbacks_list.iter() {
            callbacks_to_call.push(callback.into());
        }
        
        let py_self: Py<Self> = slf.into();
        
        // Execute callbacks for node addition
        for callback in &callbacks_to_call {
            // Call callback with (self, node) parameters
            let result = callback.call1(py, (py_self.clone_ref(py), node.clone_ref(py)))?;
            let should_continue: bool = result.extract(py).unwrap_or(true);
            if !should_continue {
                break;
            }
        }
        
        Ok(node)
    }

    /// Add a new edge between two nodes in the graph
    /// 
    /// Args:
    ///     from_id (str): ID of the source node
    ///     to_id (str): ID of the target node
    ///     attr (dict, optional): Attributes for the edge
    ///     
    /// Returns:
    ///     Edge: The created edge
    ///     
    /// Raises:
    ///     ValueError: If either node doesn't exist
    fn add_edge(
        mut slf: PyRefMut<'_, Self>,
        py: Python<'_>,
        from_id: String,
        to_id: String,
        attr: Option<HashMap<String, Py<PyAny>>>
    ) -> PyResult<Py<Edge>> {
        let edge = manipulation::add_edge(&mut slf, py, from_id, to_id, attr)?;
        
        // Get callbacks from PyList
        let callbacks_list = slf.on_edge_add_callbacks.bind(py);
        let mut callbacks_to_call: Vec<Py<PyAny>> = Vec::new();
        for callback in callbacks_list.iter() {
            callbacks_to_call.push(callback.into());
        }
        
        let py_self: Py<Self> = slf.into();

        // Execute callbacks for edge addition
        for callback in &callbacks_to_call {
            // Call callback with (self, edge) parameters
            let result = callback.call1(py, (py_self.clone_ref(py), edge.clone_ref(py)))?;
            let should_continue: bool = result.extract(py).unwrap_or(true);
            if !should_continue {
                break;
            }
        }
        
        Ok(edge)
    }

    /// Get a node by its ID
    /// 
    /// Args:
    ///     id (str): The node ID to look up
    ///     
    /// Returns:
    ///     Node: The node with the given ID
    ///     
    /// Raises:
    ///     KeyError: If no node with the given ID exists
    fn get_node(&self, py: Python<'_>, id: String) -> PyResult<Py<Node>> {
        manipulation::get_node(self, py, id)
    }

    // Serialization methods
    /// Save the graph to a JSON file
    /// 
    /// Args:
    ///     file_path (str): Path to save the graph to
    ///     
    /// Raises:
    ///     RuntimeError: If saving fails
    fn save_to_json(&self, py: Python<'_>, file_path: String) -> PyResult<()> {
        serialization::save_to_json(self, py, file_path)
    }

    /// Save the graph to a binary file (more efficient for large graphs)
    /// 
    /// Args:
    ///     file_path (str): Path to save the graph to
    ///     
    /// Raises:
    ///     RuntimeError: If saving fails
    fn save_to_binary(&self, py: Python<'_>, file_path: String) -> PyResult<()> {
        serialization::save_to_binary(self, py, file_path)
    }

    /// Load a graph from a JSON file
    /// 
    /// Args:
    ///     file_path (str): Path to load the graph from
    ///     
    /// Returns:
    ///     Vertex: The loaded graph
    ///     
    /// Raises:
    ///     RuntimeError: If loading fails
    #[staticmethod]
    fn load_from_json(py: Python<'_>, file_path: String) -> PyResult<Py<Vertex>> {
        serialization::load_from_json(py, file_path)
    }

    /// Load a graph from a binary file
    /// 
    /// Args:
    ///     file_path (str): Path to load the graph from
    ///     
    /// Returns:
    ///     Vertex: The loaded graph
    ///     
    /// Raises:
    ///     RuntimeError: If loading fails
    #[staticmethod]
    fn load_from_binary(py: Python<'_>, file_path: String) -> PyResult<Py<Vertex>> {
        serialization::load_from_binary(py, file_path)
    }

    // Analysis methods
    /// Get metadata about the graph (node count, edge count, etc.)
    fn get_metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        analysis::get_metadata(self, py)
    }

    /// Convert the graph to a NetworkX DiGraph object
    /// 
    /// Returns:
    ///     networkx.DiGraph: A NetworkX directed graph representation of this vertex
    ///     
    /// Raises:
    ///     RuntimeError: If NetworkX is not available or conversion fails
    fn to_networkx(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        analysis::to_networkx(self, py)
    }

    // Algorithm methods
    /// Find the shortest path between source and target nodes using Breadth-First Search
    /// 
    /// Args:
    ///     root_node_id (str): ID of the source node to start the search from
    ///     target_node_id (str): ID of the target node to find
    ///     max_depth (int, optional): Maximum depth to search. If None, searches indefinitely.
    ///     
    /// Returns:
    ///     Vertex: A new vertex containing only the nodes in the shortest path from source to target
    ///     
    /// Raises:
    ///     ValueError: If either source or target node doesn't exist, or if target is not reachable within max_depth
    #[pyo3(signature = (root_node_id, target_node_id, max_depth=None))]
    fn shortest_path_bfs(
        &self,
        py: Python<'_>,
        root_node_id: String,
        target_node_id: String,
        max_depth: Option<usize>
    ) -> PyResult<Py<Vertex>> {
        algorithms::shortest_path_bfs(self, py, root_node_id, target_node_id, max_depth)
    }

    /// Expand the current vertex by adding neighbor nodes from a source vertex
    /// 
    /// Args:
    ///     source_vertex (Vertex): The source vertex to expand from (contains the full graph)
    ///     depth (int, optional): Maximum depth to traverse for expansion. Defaults to 1.
    ///     
    /// Returns:
    ///     Vertex: A new vertex containing the original nodes plus neighbors found within the specified depth
    ///     
    /// Raises:
    ///     ValueError: If expansion fails
    #[pyo3(signature = (source_vertex, depth=None))]
    fn expand(
        &self,
        py: Python<'_>,
        source_vertex: &Vertex,
        depth: Option<usize>
    ) -> PyResult<Py<Vertex>> {
        algorithms::expand(self, py, source_vertex, depth)
    }

    /// Create a new vertex containing only the specified nodes and their connecting edges
    /// 
    /// Args:
    ///     node_ids (list): List of node IDs to include in the filtered vertex
    ///     
    /// Returns:
    ///     Vertex: A new vertex containing only the specified nodes and edges between them
    ///     
    /// Raises:
    ///     ValueError: If any of the specified node IDs don't exist in the vertex
    fn filter(
        &self,
        py: Python<'_>,
        node_ids: Vec<String>
    ) -> PyResult<Py<Vertex>> {
        algorithms::filter(self, py, node_ids)
    }
}
