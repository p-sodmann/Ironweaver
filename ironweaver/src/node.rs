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
    edge: &Py<Edge>,
    py: Python<'_>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
) -> PyResult<bool> {
    if let Some(filter_map) = filter {
        let edge_ref = edge.borrow(py);
        for (filter_key, filter_value) in filter_map {
            match edge_ref.attr.get(filter_key) {
                Some(edge_value) => {
                    let eq_obj = edge_value
                        .bind(py)
                        .rich_compare(filter_value.bind(py), pyo3::basic::CompareOp::Eq)?;
                    if !eq_obj.is_truthy()? {
                        return Ok(false);
                    }
                }
                None => return Ok(false),
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
        if edge_matches_filter(&edge, py, filter)? {
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
    let start_node_ref = start_node.borrow(py);
    let start_id = start_node_ref.id.clone();
    
    // Mark starting node and add to queue
    visited.insert(start_id.clone());
    found.insert(start_id.clone(), start_node.clone_ref(py));
    nodelist.push(start_id.clone());
    queue.push_back((start_node.clone_ref(py), 0));
    
    while let Some((current_node, current_depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(d) = depth {
            if current_depth >= d {
                continue;
            }
        }

        // Get edges from current node
        let current_ref = current_node.borrow(py);
        for edge in &current_ref.edges {
            // Check if edge matches filter criteria
            if edge_matches_filter(&edge, py, filter)? {
                let edge_ref = edge.borrow(py);
                let to_node: Py<Node> = edge_ref.to_node.clone_ref(py);
                let to_id = to_node.borrow(py).id.clone();
                
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
    let start_node_ref = start_node.borrow(py);
    let start_id = start_node_ref.id.clone();
    
    // Check if start node is the target
    if start_id == target_id {
        return Ok(Some(start_node.clone_ref(py)));
    }
    
    // Mark starting node and add to queue
    visited.insert(start_id.clone());
    queue.push_back((start_node.clone_ref(py), 0));
    
    while let Some((current_node, current_depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(d) = depth {
            if current_depth >= d {
                continue;
            }
        }
        
        // Get edges from current node
        let current_ref = current_node.borrow(py);
        for edge in &current_ref.edges {
            // Check if edge matches filter criteria
            if edge_matches_filter(&edge, py, filter)? {
                let edge_ref = edge.borrow(py);
                let to_node: Py<Node> = edge_ref.to_node.clone_ref(py);
                let to_id = to_node.borrow(py).id.clone();
                
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
