use pyo3::prelude::*;
use pyo3::types::{PyAny, PyList};
use std::collections::{HashMap, HashSet};
use crate::Edge;
use crate::Vertex;

#[pyclass]
pub struct Node {
    #[pyo3(get, set)]
    pub id: String,
    #[pyo3(get, set)]
    pub attr: HashMap<String, Py<PyAny>>,
    #[pyo3(get, set)]
    pub edges: Vec<Py<Edge>>,
    #[pyo3(get, set)]
    pub inverse_edges: Vec<Py<Edge>>,
    #[pyo3(get, set)]
    pub meta: HashMap<String, Py<PyAny>>,
    #[pyo3(get, set)]
    pub on_edge_add_callbacks: Vec<Py<PyAny>>,
}

#[pymethods]
impl Node {
    #[new]
    pub fn new(
        id: String,
        attr: Option<HashMap<String, Py<PyAny>>>,
        edges: Option<Vec<Py<Edge>>>,
    ) -> Self {
        Node {
            id,
            attr: attr.unwrap_or_default(),
            edges: edges.unwrap_or_default(),
            inverse_edges: Vec::new(),
            meta: HashMap::new(),
            on_edge_add_callbacks: Vec::new(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{}", self.id)
    }

    #[getter]
    fn id(&self) -> &str {
        &self.id
    }

    /// Traverse reachable nodes, returning Vertex
    /// If depth is None, traverses all.
    /// filter: Optional HashMap of edge attribute filters (e.g., {"type": "broader"})
    /// Returns a Vertex (dict of id:Node) with traversal path in meta["nodelist"]
    fn traverse<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        depth: Option<usize>,
        filter: Option<HashMap<String, Py<PyAny>>>
    ) -> PyResult<Py<Vertex>> {
        let self_handle: Py<Node> = slf.into();

        let mut found = HashMap::<String, Py<Node>>::new();
        let mut visited = HashSet::<String>::new();
        let mut nodelist = Vec::<String>::new();
        traverse_recursive(py, self_handle, depth, 0, &mut found, &mut visited, &mut nodelist, &filter)?;

        Py::new(py, Vertex::from_nodes_with_path(py, found, nodelist)?)
    }

    /// Breadth-First Search traversal of reachable nodes
    /// If depth is None, traverses all nodes.
    /// filter: Optional HashMap of edge attribute filters (e.g., {"type": "broader"})
    /// Returns a Vertex (dict of id:Node) in BFS order with traversal path in meta["nodelist"]
    fn bfs<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        depth: Option<usize>,
        filter: Option<HashMap<String, Py<PyAny>>>
    ) -> PyResult<Py<Vertex>> {
        let self_handle: Py<Node> = slf.into();

        let mut found = HashMap::<String, Py<Node>>::new();
        let mut visited = HashSet::<String>::new();
        let mut nodelist = Vec::<String>::new();
        bfs_iterative(py, self_handle, depth, &mut found, &mut visited, &mut nodelist, &filter)?;

        Py::new(py, Vertex::from_nodes_with_path(py, found, nodelist)?)
    }

    /// Search for a specific node by ID using BFS
    /// filter: Optional HashMap of edge attribute filters (e.g., {"type": "broader"})
    /// Returns the node if found, None otherwise
    fn bfs_search<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        target_id: String,
        depth: Option<usize>,
        filter: Option<HashMap<String, Py<PyAny>>>
    ) -> PyResult<Option<Py<Node>>> {
        let self_handle: Py<Node> = slf.into();
        bfs_search_iterative(py, self_handle, target_id, depth, &filter)
    }

    /// Retrieve a value from ``attr`` by key.
    /// Returns ``None`` if the key does not exist.
    fn attr_get<'py>(&self, py: Python<'py>, key: String) -> Option<Py<PyAny>> {
        self.attr.get(&key).map(|v| v.clone_ref(py))
    }

    /// Set a value in ``attr`` under ``key``.
    fn attr_set(&mut self, key: String, value: Py<PyAny>) {
        self.attr.insert(key, value);
    }

    /// Append ``value`` to a list stored at ``key`` in ``attr``.
    /// If the list does not exist, it will be created.
    #[pyo3(signature = (key, value))]
    fn attr_list_append(&mut self, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        if let Some(existing) = self.attr.get(&key) {
            let list_any = existing.bind(py);
            let list = list_any.downcast::<PyList>()?;
            list.append(value)?;
        } else {
            let list = PyList::empty(py);
            list.append(value)?;
            self.attr.insert(key, list.into());
        }
        Ok(())
    }
}

// Helper function to check if an edge matches the filter criteria
fn edge_matches_filter(
    py: Python<'_>,
    edge: &Py<Edge>,
    filter: &Option<HashMap<String, Py<PyAny>>>
) -> PyResult<bool> {
    if let Some(filter_map) = filter {
        let edge_ref = edge.bind(py);
        let edge_attr: HashMap<String, Py<PyAny>> = edge_ref.getattr("attr")?.extract()?;
        
        // Check if all filter criteria are met
        for (filter_key, filter_value) in filter_map {
            if let Some(edge_value) = edge_attr.get(filter_key) {
                // Compare the values by converting to Python objects and using Python's equality
                let edge_py_obj = edge_value.bind(py);
                let filter_py_obj = filter_value.bind(py);
                
                if !edge_py_obj.eq(filter_py_obj)? {
                    return Ok(false);
                }
            } else {
                // Edge doesn't have the required attribute
                return Ok(false);
            }
        }
    }
    Ok(true)
}

// Helper is Rust-only, not a #[pymethods]
fn traverse_recursive(
    py: Python<'_>,
    node_handle: Py<Node>,
    depth: Option<usize>,
    current_depth: usize,
    found: &mut HashMap<String, Py<Node>>,
    visited: &mut HashSet<String>,
    nodelist: &mut Vec<String>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
) -> PyResult<()> {
    let node_ref = node_handle.bind(py);

    // Use node id as unique key
    let id = node_ref.getattr("id")?.extract::<String>()?;
    if !visited.insert(id.clone()) {
        return Ok(());
    }
    found.insert(id.clone(), node_handle.clone_ref(py));
    nodelist.push(id.clone());

    // Check depth limit
    if let Some(d) = depth {
        if current_depth >= d {
            return Ok(());
        }
    }

    // Traverse edges
    let edges: Vec<Py<Edge>> = node_ref.getattr("edges")?.extract()?;
    for edge in edges {
        // Check if edge matches filter criteria
        if edge_matches_filter(py, &edge, filter)? {
            let to_node: Py<Node> = edge.bind(py).getattr("to_node")?.extract()?;
            traverse_recursive(py, to_node, depth, current_depth + 1, found, visited, nodelist, filter)?;
        }
    }
    Ok(())
}

// BFS helper function using iterative approach with queue
fn bfs_iterative(
    py: Python<'_>,
    start_node: Py<Node>,
    depth: Option<usize>,
    found: &mut HashMap<String, Py<Node>>,
    visited: &mut HashSet<String>,
    nodelist: &mut Vec<String>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
) -> PyResult<()> {
    use std::collections::VecDeque;
    
    // Queue stores (node, current_depth)
    let mut queue = VecDeque::new();
    
    // Get starting node ID
    let start_id = start_node.bind(py).getattr("id")?.extract::<String>()?;
    
    // Mark starting node and add to queue
    visited.insert(start_id.clone());
    found.insert(start_id.clone(), start_node.clone_ref(py));
    nodelist.push(start_id);
    queue.push_back((start_node, 0));
    
    while let Some((current_node, current_depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(d) = depth {
            if current_depth >= d {
                continue;
            }
        }
        
        // Get edges from current node
        let edges: Vec<Py<Edge>> = current_node.bind(py).getattr("edges")?.extract()?;
        
        for edge in edges {
            // Check if edge matches filter criteria
            if edge_matches_filter(py, &edge, filter)? {
                let to_node: Py<Node> = edge.bind(py).getattr("to_node")?.extract()?;
                let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
                
                // If not visited, mark and enqueue
                if !visited.contains(&to_id) {
                    visited.insert(to_id.clone());
                    found.insert(to_id.clone(), to_node.clone_ref(py));
                    nodelist.push(to_id);
                    queue.push_back((to_node, current_depth + 1));
                }
            }
        }
    }
    
    Ok(())
}

// BFS search helper function that stops when target is found
fn bfs_search_iterative(
    py: Python<'_>,
    start_node: Py<Node>,
    target_id: String,
    depth: Option<usize>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
) -> PyResult<Option<Py<Node>>> {
    use std::collections::VecDeque;
    
    // Queue stores (node, current_depth)
    let mut queue = VecDeque::new();
    let mut visited = HashSet::<String>::new();
    
    // Get starting node ID
    let start_id = start_node.bind(py).getattr("id")?.extract::<String>()?;
    
    // Check if start node is the target
    if start_id == target_id {
        return Ok(Some(start_node));
    }
    
    // Mark starting node and add to queue
    visited.insert(start_id);
    queue.push_back((start_node, 0));
    
    while let Some((current_node, current_depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(d) = depth {
            if current_depth >= d {
                continue;
            }
        }
        
        // Get edges from current node
        let edges: Vec<Py<Edge>> = current_node.bind(py).getattr("edges")?.extract()?;
        
        for edge in edges {
            // Check if edge matches filter criteria
            if edge_matches_filter(py, &edge, filter)? {
                let to_node: Py<Node> = edge.bind(py).getattr("to_node")?.extract()?;
                let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
                
                // If this is our target, return it
                if to_id == target_id {
                    return Ok(Some(to_node));
                }
                
                // If not visited, mark and enqueue
                if !visited.contains(&to_id) {
                    visited.insert(to_id);
                    queue.push_back((to_node, current_depth + 1));
                }
            }
        }
    }
    
    // Target not found
    Ok(None)
}
