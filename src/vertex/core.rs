// vertex/core.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use pyo3::{PyTraverseError, PyVisit};
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

    // Garbage-collector support (see Node::__traverse__).
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for n in self.nodes.values() {
            visit.call(n)?;
        }
        visit.call(&self.meta)?;
        visit.call(&self.on_node_add_callbacks)?;
        visit.call(&self.on_edge_add_callbacks)?;
        visit.call(&self.on_node_update_callbacks)?;
        visit.call(&self.on_edge_update_callbacks)?;
        Ok(())
    }

    fn __clear__(&mut self) {
        self.nodes.clear();
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
        let keys: Vec<String> = self.nodes.values().map(|n| n.borrow(py).id.clone()).collect();
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
    #[pyo3(signature = (id, attr=None))]
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
    #[pyo3(signature = (from_id, to_id, attr=None))]
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
    ///     pretty (bool, optional): Indent the output. Defaults to False (compact JSON).
    ///     
    /// Returns:
    ///     None if file_path is provided, or str (JSON) if file_path is None
    ///     
    /// Raises:
    ///     RuntimeError: If saving/serialization fails
    #[pyo3(signature = (file_path=None, pretty=false))]
    fn save_to_json(&self, py: Python<'_>, file_path: Option<String>, pretty: bool) -> PyResult<Py<PyAny>> {
        serialization::save_to_json(self, py, file_path, pretty)
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
    ///     direction (str, optional): "out" (default) follows outgoing edges, "in" follows
    ///         incoming edges, "both" ignores edge direction.
    ///     
    /// Returns:
    ///     Vertex: A new vertex containing only the nodes in the shortest path from source to target
    ///     
    /// Raises:
    ///     ValueError: If either source or target node doesn't exist, or if target is not reachable within max_depth
    #[pyo3(signature = (root_node_id, target_node_id, max_depth=None, direction=None))]
    fn shortest_path_bfs(
        &self,
        py: Python<'_>,
        root_node_id: String,
        target_node_id: String,
        max_depth: Option<usize>,
        direction: Option<&str>,
    ) -> PyResult<Py<Vertex>> {
        algorithms::shortest_path_bfs(self, py, root_node_id, target_node_id, max_depth, direction)
    }

    /// Find the cheapest path between two nodes using Dijkstra's algorithm
    ///
    /// Args:
    ///     root_node_id (str): ID of the source node
    ///     target_node_id (str): ID of the target node
    ///     weight (str, optional): Edge attribute holding the edge cost. Defaults to "weight".
    ///     default_weight (float, optional): Cost of edges without that attribute. Defaults to 1.0.
    ///     max_cost (float, optional): Ignore paths more expensive than this.
    ///     direction (str, optional): "out" (default), "in" or "both".
    ///
    /// Returns:
    ///     Vertex: The nodes on the cheapest path (and the edges between them), with
    ///         meta["nodelist"] holding the path in order and meta["cost"] its total cost
    ///
    /// Raises:
    ///     ValueError: If a node doesn't exist, the target is unreachable, or a weight is negative
    ///     TypeError: If a weight attribute is not a number
    #[pyo3(signature = (root_node_id, target_node_id, weight=None, default_weight=None, max_cost=None, direction=None))]
    fn shortest_path_dijkstra(
        &self,
        py: Python<'_>,
        root_node_id: String,
        target_node_id: String,
        weight: Option<String>,
        default_weight: Option<f64>,
        max_cost: Option<f64>,
        direction: Option<&str>,
    ) -> PyResult<Py<Vertex>> {
        algorithms::shortest_path_dijkstra(
            self, py, root_node_id, target_node_id, weight, default_weight, max_cost, direction,
        )
    }

    /// Expand the current vertex by adding neighbor nodes from a source vertex
    ///
    /// Args:
    ///     source_vertex (Vertex): The source vertex to expand from (contains the full graph)
    ///     depth (int, optional): Maximum depth to traverse for expansion. Defaults to 1.
    ///     direction (str, optional): "out" (default) follows outgoing edges, "in" follows
    ///         incoming edges, "both" ignores edge direction.
    ///     
    /// Returns:
    ///     Vertex: A new vertex containing the original nodes plus neighbors found within the specified depth
    ///     
    /// Raises:
    ///     ValueError: If expansion fails
    #[pyo3(signature = (source_vertex, depth=None, direction=None))]
    fn expand(
        &self,
        py: Python<'_>,
        source_vertex: &Vertex,
        depth: Option<usize>,
        direction: Option<&str>,
    ) -> PyResult<Py<Vertex>> {
        algorithms::expand(self, py, source_vertex, depth, direction)
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
                let mut all_match = true;
                for (key, value) in &filters {
                    let node_val = node.borrow(py).attr.get(key).map(|v| v.clone_ref(py));
                    match node_val {
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
    /// Remove a node and all edges attached to it
    ///
    /// Args:
    ///     id (str): ID of the node to remove
    ///
    /// Returns:
    ///     Node: The removed node (with its edge lists emptied)
    ///
    /// Raises:
    ///     KeyError: If no node with the given ID exists
    fn remove_node(&mut self, py: Python<'_>, id: &str) -> PyResult<Py<Node>> {
        manipulation::remove_node(self, py, id)
    }

    /// Remove the edge(s) from one node to another
    ///
    /// Args:
    ///     from_id (str): ID of the source node
    ///     to_id (str): ID of the target node
    ///     attr (dict, optional): Only remove edges whose attributes match all of these pairs
    ///
    /// Returns:
    ///     int: The number of edges removed
    ///
    /// Raises:
    ///     ValueError: If either node doesn't exist
    #[pyo3(signature = (from_id, to_id, attr=None))]
    fn remove_edge(
        &self,
        py: Python<'_>,
        from_id: &str,
        to_id: &str,
        attr: Option<HashMap<String, Py<PyAny>>>,
    ) -> PyResult<usize> {
        manipulation::remove_edge(self, py, from_id, to_id, attr)
    }

    /// Remove edges and inverse_edges that reference nodes not present in the vertex.
    ///
    /// This is useful after filtering or subsetting the graph, when edges may still
    /// point to nodes that are no longer part of the vertex.
    ///
    /// Returns:
    ///     int: The number of edges removed
    fn prune(&self, py: Python<'_>) -> PyResult<usize> {
        manipulation::prune(self, py)
    }

    /// Perform multiple random walks from a starting node
    ///
    /// Args:
    ///     start_node_id (str, optional): ID of the node to start the random walks from.
    ///         May be None only when stratified=True, in which case each walk's start
    ///         is sampled across all nodes with probability inversely proportional to
    ///         how often each node has been visited so far.
    ///     max_length (int): Maximum length of each random walk
    ///     num_attempts (int): Number of random walk attempts to perform
    ///     min_length (int, optional): Minimum length of each random walk. Defaults to 1.
    ///     allow_revisit (bool, optional): Whether to allow revisiting nodes. Defaults to False.
    ///     include_edge_types (bool, optional): Whether to include edge types in the result. Defaults to False.
    ///     edge_type_field (str, optional): Field name to extract edge type from. Defaults to "type".
    ///     stratified (bool, optional): Equalize node visit frequencies. Every choice
    ///         (the start node when start_node_id is None, and each step) is weighted by
    ///         1 / (1 + times_visited), steering walks towards least-visited nodes.
    ///         Visit counts persist across all attempts of one call. Defaults to False.
    ///     seed (int, optional): Seed for the random number generator. The same seed and
    ///         arguments always produce the same walks. Defaults to a random seed.
    ///
    /// Returns:
    ///     list: A list of lists. If include_edge_types is False, each inner list contains node IDs.
    ///           If include_edge_types is True, each inner list alternates between node IDs and edge types.
    ///           Duplicates are automatically removed.
    ///
    /// Raises:
    ///     ValueError: If start_node_id doesn't exist, is None without stratified=True,
    ///         max_length is 0, or min_length > max_length
    #[pyo3(signature = (start_node_id, max_length, num_attempts, min_length=None, allow_revisit=None, include_edge_types=None, edge_type_field=None, stratified=None, seed=None))]
    fn random_walks(
        &self,
        py: Python<'_>,
        start_node_id: Option<String>,
        max_length: usize,
        num_attempts: usize,
        min_length: Option<usize>,
        allow_revisit: Option<bool>,
        include_edge_types: Option<bool>,
        edge_type_field: Option<String>,
        stratified: Option<bool>,
        seed: Option<u64>,
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
            stratified,
            seed,
        )
    }
}
