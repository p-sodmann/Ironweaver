use pyo3::prelude::*;
use pyo3::types::PyAny;
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
    /// Returns a Vertex (dict of id:Node)
    fn traverse<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        depth: Option<usize>
    ) -> PyResult<Py<Vertex>> {
        let self_handle: Py<Node> = slf.into();

        let mut found = HashMap::<String, Py<Node>>::new();
        let mut visited = HashSet::<String>::new();
        traverse_recursive(py, self_handle, depth, 0, &mut found, &mut visited)?;

        Py::new(py, Vertex::from_nodes(py, found))
    }

    /// Breadth-First Search traversal of reachable nodes
    /// If depth is None, traverses all nodes.
    /// Returns a Vertex (dict of id:Node) in BFS order
    fn bfs<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        depth: Option<usize>
    ) -> PyResult<Py<Vertex>> {
        let self_handle: Py<Node> = slf.into();

        let mut found = HashMap::<String, Py<Node>>::new();
        let mut visited = HashSet::<String>::new();
        bfs_iterative(py, self_handle, depth, &mut found, &mut visited)?;

        Py::new(py, Vertex::from_nodes(py, found))
    }

    /// Search for a specific node by ID using BFS
    /// Returns the node if found, None otherwise
    fn bfs_search<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        target_id: String,
        depth: Option<usize>
    ) -> PyResult<Option<Py<Node>>> {
        let self_handle: Py<Node> = slf.into();
        bfs_search_iterative(py, self_handle, target_id, depth)
    }
}

// Helper is Rust-only, not a #[pymethods]
fn traverse_recursive(
    py: Python<'_>,
    node_handle: Py<Node>,
    depth: Option<usize>,
    current_depth: usize,
    found: &mut HashMap<String, Py<Node>>,
    visited: &mut HashSet<String>,
) -> PyResult<()> {
    let node_ref = node_handle.bind(py);

    // Use node id as unique key
    let id = node_ref.getattr("id")?.extract::<String>()?;
    if !visited.insert(id.clone()) {
        return Ok(());
    }
    found.insert(id.clone(), node_handle.clone_ref(py));

    // Check depth limit
    if let Some(d) = depth {
        if current_depth >= d {
            return Ok(());
        }
    }

    // Traverse edges
    let edges: Vec<Py<Edge>> = node_ref.getattr("edges")?.extract()?;
    for edge in edges {
        let to_node: Py<Node> = edge.bind(py).getattr("to")?.extract()?;
        traverse_recursive(py, to_node, depth, current_depth + 1, found, visited)?;
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
) -> PyResult<()> {
    use std::collections::VecDeque;
    
    // Queue stores (node, current_depth)
    let mut queue = VecDeque::new();
    
    // Get starting node ID
    let start_id = start_node.bind(py).getattr("id")?.extract::<String>()?;
    
    // Mark starting node and add to queue
    visited.insert(start_id.clone());
    found.insert(start_id, start_node.clone_ref(py));
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
            let to_node: Py<Node> = edge.bind(py).getattr("to")?.extract()?;
            let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
            
            // If not visited, mark and enqueue
            if !visited.contains(&to_id) {
                visited.insert(to_id.clone());
                found.insert(to_id, to_node.clone_ref(py));
                queue.push_back((to_node, current_depth + 1));
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
            let to_node: Py<Node> = edge.bind(py).getattr("to")?.extract()?;
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
    
    // Target not found
    Ok(None)
}
