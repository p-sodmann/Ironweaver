// vertex/core.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use std::collections::HashMap;

use crate::{Edge, Node};

// Import the helper modules as sibling modules
use super::algorithms;
use super::analysis;
use super::callbacks;
use super::manipulation;
use super::serialization;

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
    #[pyo3(get, set)]
    pub on_node_update_callbacks: Py<PyList>,
    #[pyo3(get, set)]
    pub on_edge_update_callbacks: Py<PyList>,
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
            on_node_update_callbacks: PyList::empty(py).into(),
            on_edge_update_callbacks: PyList::empty(py).into(),
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
            on_node_update_callbacks: PyList::empty(py).into(),
            on_edge_update_callbacks: PyList::empty(py).into(),
        }
    }

    /// Create a new graph with existing nodes and traversal path
    #[staticmethod]
    pub fn from_nodes_with_path(
        py: Python<'_>,
        nodes: HashMap<String, Py<Node>>,
        nodelist: Vec<String>,
    ) -> PyResult<Self> {
        let meta = PyDict::new(py);
        meta.set_item("nodelist", nodelist)?;

        Ok(Vertex {
            nodes,
            meta: meta.into(),
            on_node_add_callbacks: PyList::empty(py).into(),
            on_edge_add_callbacks: PyList::empty(py).into(),
            on_node_update_callbacks: PyList::empty(py).into(),
            on_edge_update_callbacks: PyList::empty(py).into(),
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
        let keys: Vec<String> = self
            .nodes
            .values()
            .filter_map(|n| {
                n.bind(py)
                    .getattr("id")
                    .ok()
                    .and_then(|o| o.extract::<String>().ok())
            })
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
        attr: Option<HashMap<String, Py<PyAny>>>,
    ) -> PyResult<Py<Node>> {
        // First create the node
        let node = manipulation::add_node(&mut slf, py, id, attr)?;

        // Collect the callback lists before consuming slf
        let update_cbs = slf.on_node_update_callbacks.clone_ref(py);
        let add_cbs = slf.on_node_add_callbacks.clone_ref(py);
        let py_self: Py<Self> = slf.into();

        // Link the vertex's on_node_update_callbacks to the new node so that
        // future attr_set calls on the node fire the vertex-level callbacks.
        // Also store a back-reference to the vertex so callbacks can access it.
        {
            let mut node_ref = node.bind(py).borrow_mut();
            node_ref.on_update_callbacks = update_cbs;
            node_ref.vertex = Some(py_self.clone_ref(py).into_any());
        }

        callbacks::fire_node_add_callbacks(
            py,
            add_cbs.bind(py),
            py_self.into_any(),
            node.clone_ref(py),
        )?;

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
        attr: Option<HashMap<String, Py<PyAny>>>,
    ) -> PyResult<Py<Edge>> {
        let edge = manipulation::add_edge(&mut slf, py, from_id, to_id, attr)?;

        // Collect the callback lists before consuming slf
        let update_cbs = slf.on_edge_update_callbacks.clone_ref(py);
        let add_cbs = slf.on_edge_add_callbacks.clone_ref(py);
        let py_self: Py<Self> = slf.into();

        // Link the vertex's on_edge_update_callbacks to the new edge so that
        // future attr_set calls on the edge fire the vertex-level callbacks.
        // Also store a back-reference to the vertex so callbacks can access it.
        {
            let mut edge_ref = edge.bind(py).borrow_mut();
            edge_ref.on_update_callbacks = update_cbs;
            edge_ref.vertex = Some(py_self.clone_ref(py).into_any());
        }

        callbacks::fire_edge_add_callbacks(
            py,
            add_cbs.bind(py),
            py_self.into_any(),
            edge.clone_ref(py),
        )?;

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
    /// Save the graph to a JSON file or return JSON string
    ///
    /// Args:
    ///     file_path (str, optional): Path to save the graph to. If None, returns JSON string.
    ///     
    /// Returns:
    ///     None if file_path is provided, or str (JSON) if file_path is None
    ///     
    /// Raises:
    ///     RuntimeError: If saving/serialization fails
    #[pyo3(signature = (file_path=None))]
    fn save_to_json(&self, py: Python<'_>, file_path: Option<String>) -> PyResult<Py<PyAny>> {
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

    /// Save the graph to a binary file using f16 precision for floats
    #[pyo3(text_signature = "(self, file_path)")]
    fn save_to_binary_f16(&self, py: Python<'_>, file_path: String) -> PyResult<()> {
        serialization::save_to_binary_f16(self, py, file_path)
    }

    /// Load a graph from a JSON file, JSON string, or dict
    ///
    /// Args:
    ///     source (str | dict): Either a file path, a JSON string, or a dict representing the graph
    ///     
    /// Returns:
    ///     Vertex: The loaded graph
    ///     
    /// Raises:
    ///     RuntimeError: If loading fails
    ///     TypeError: If source is not a valid type
    #[staticmethod]
    fn load_from_json(py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<Py<Vertex>> {
        serialization::load_from_json(py, source)
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
        max_depth: Option<usize>,
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
        depth: Option<usize>,
    ) -> PyResult<Py<Vertex>> {
        algorithms::expand(self, py, source_vertex, depth)
    }

    /// Create a new vertex containing only the specified nodes and their connecting edges
    ///
    /// Args:
    ///     ids (list, optional): List of node IDs to include
    ///     id (str, optional): Single node ID to include
    ///     **kwargs: Attribute key/value pairs to match nodes
    ///
    /// Returns:
    ///     Vertex: A new vertex containing only the specified nodes and edges between them
    ///
    /// Raises:
    ///     ValueError: If any of the specified node IDs don't exist in the vertex or
    ///                 no filter criteria are provided
    #[pyo3(signature = (**kwargs))]
    fn filter(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<Vertex>> {
        let kwargs = kwargs.ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(
                "Must specify ids, id, or attribute filters",
            )
        })?;

        let mut filters: HashMap<String, Py<PyAny>> = kwargs.extract()?;

        // Determine which node IDs to include based on the provided keyword arguments
        let node_ids: Vec<String> = if let Some(ids_any) = filters.remove("ids") {
            ids_any.extract(py)?
        } else if let Some(id_any) = filters.remove("id") {
            vec![id_any.extract(py)?]
        } else if !filters.is_empty() {
            let mut matches = Vec::new();
            for (node_id, node) in &self.nodes {
                let node_ref = node.bind(py);
                let attrs: HashMap<String, Py<PyAny>> =
                    node_ref.getattr("attr")?.extract().unwrap_or_default();

                let mut all_match = true;
                for (key, value) in &filters {
                    match attrs.get(key) {
                        Some(node_val) => {
                            if !node_val.bind(py).eq(value.bind(py))? {
                                all_match = false;
                                break;
                            }
                        }
                        None => {
                            all_match = false;
                            break;
                        }
                    }
                }

                if all_match {
                    matches.push(node_id.clone());
                }
            }
            matches
        } else {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Must specify ids, id, or attribute filters",
            ));
        };

        algorithms::filter(self, py, node_ids)
    }
    /// Perform multiple random walks from a starting node
    ///
    /// Args:
    ///     start_node_id (str): ID of the node to start the random walks from
    ///     max_length (int): Maximum length of each random walk
    ///     num_attempts (int): Number of random walk attempts to perform
    ///     min_length (int, optional): Minimum length of each random walk. Defaults to 1.
    ///     allow_revisit (bool, optional): Whether to allow revisiting nodes. Defaults to False.
    ///     include_edge_types (bool, optional): Whether to include edge types in the result. Defaults to False.
    ///     edge_type_field (str, optional): Field name to extract edge type from. Defaults to "type".
    ///     
    /// Returns:
    ///     list: A list of lists. If include_edge_types is False, each inner list contains node IDs.
    ///           If include_edge_types is True, each inner list alternates between node IDs and edge types.
    ///           Duplicates are automatically removed.
    ///     
    /// Raises:
    ///     ValueError: If start_node_id doesn't exist, max_length is 0, or min_length > max_length#[pyo3(signature = (start_node_id, max_length, num_attempts, min_length=None, allow_revisit=None, include_edge_types=None, edge_type_field=None))]
    fn random_walks(
        &self,
        py: Python<'_>,
        start_node_id: String,
        max_length: usize,
        num_attempts: usize,
        min_length: Option<usize>,
        allow_revisit: Option<bool>,
        include_edge_types: Option<bool>,
        edge_type_field: Option<String>,
    ) -> PyResult<Py<PyList>> {
        algorithms::random_walks(
            self,
            py,
            start_node_id,
            max_length,
            min_length,
            num_attempts,
            allow_revisit,
            include_edge_types,
            edge_type_field,
        )
    }
}
