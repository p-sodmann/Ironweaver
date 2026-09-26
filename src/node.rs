use pyo3::prelude::*;
use pyo3::types::{PyAny, PyList};
use std::collections::{HashMap, HashSet};
use pyo3::class::basic::CompareOp;
use pyo3::{PyTraverseError, PyVisit};
use crate::Edge;
use crate::Vertex;
use crate::vertex::algorithms::bidirectional::bidirectional_bfs;
use crate::vertex::subgraph::Direction;

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
    /// Callbacks fired when an attribute changes via ``attr_set``.
    /// Shared with the owning ``Vertex.on_node_update_callbacks`` by reference.
    #[pyo3(get, set)]
    pub on_update_callbacks: Py<PyList>,
    /// Back-reference to the owning Vertex (set during ``add_node``).
    #[pyo3(get)]
    pub vertex: Option<Py<PyAny>>,
}

#[pymethods]
impl Node {
    #[new]
    pub fn new(
        py: Python<'_>,
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
            on_update_callbacks: PyList::empty(py).into(),
            vertex: None,
        }
    }

    fn __repr__(&self) -> String {
        format!("{}", self.id)
    }

    // Garbage-collector support. Nodes and edges reference each other
    // (node -> edge -> node) and hold a back-reference to their Vertex, so
    // without these hooks Python can never reclaim a graph.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for v in self.attr.values() {
            visit.call(v)?;
        }
        for e in self.edges.iter().chain(self.inverse_edges.iter()) {
            visit.call(e)?;
        }
        for v in self.meta.values() {
            visit.call(v)?;
        }
        for cb in &self.on_edge_add_callbacks {
            visit.call(cb)?;
        }
        visit.call(&self.on_update_callbacks)?;
        visit.call(&self.vertex)?;
        Ok(())
    }

    fn __clear__(&mut self) {
        self.attr.clear();
        self.edges.clear();
        self.inverse_edges.clear();
        self.meta.clear();
        self.on_edge_add_callbacks.clear();
        self.vertex = None;
    }

    #[getter]
    fn id(&self) -> &str {
        &self.id
    }

    /// Traverse reachable nodes, returning Vertex
    /// If depth is None, traverses all.
    /// filter: Optional HashMap of edge attribute filters (e.g., {"type": "broader"})
    /// edge_filter: Optional Python callable that receives an Edge and returns bool
    /// Returns a Vertex (dict of id:Node) with traversal path in meta["nodelist"]
    #[pyo3(name = "_traverse")]
    fn traverse_nodes<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        depth: Option<usize>,
        filter: Option<HashMap<String, Py<PyAny>>>,
        edge_filter: Option<Py<PyAny>>,
    ) -> PyResult<Py<Vertex>> {
        let self_handle: Py<Node> = slf.into();

        let mut found = HashMap::<String, Py<Node>>::new();
        let mut visited = HashSet::<String>::new();
        let mut nodelist = Vec::<String>::new();
        traverse_iterative(py, self_handle, depth, &mut found, &mut visited, &mut nodelist, &filter, &edge_filter)?;

        Py::new(py, Vertex::from_nodes_with_path(py, found, nodelist)?)
    }

    /// Breadth-First Search traversal of reachable nodes
    /// If depth is None, traverses all nodes.
    /// filter: Optional HashMap of edge attribute filters (e.g., {"type": "broader"})
    /// edge_filter: Optional Python callable that receives an Edge and returns bool
    /// Returns a Vertex (dict of id:Node) in BFS order with traversal path in meta["nodelist"]
    fn bfs<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        depth: Option<usize>,
        filter: Option<HashMap<String, Py<PyAny>>>,
        edge_filter: Option<Py<PyAny>>,
    ) -> PyResult<Py<Vertex>> {
        let self_handle: Py<Node> = slf.into();

        let mut found = HashMap::<String, Py<Node>>::new();
        let mut visited = HashSet::<String>::new();
        let mut nodelist = Vec::<String>::new();
        bfs_iterative(py, self_handle, depth, &mut found, &mut visited, &mut nodelist, &filter, &edge_filter)?;

        Py::new(py, Vertex::from_nodes_with_path(py, found, nodelist)?)
    }

    /// Search for a specific node by ID using BFS
    /// filter: Optional HashMap of edge attribute filters (e.g., {"type": "broader"})
    /// edge_filter: Optional Python callable that receives an Edge and returns bool
    /// Returns the node if found, None otherwise
    fn bfs_search<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        target_id: String,
        depth: Option<usize>,
        filter: Option<HashMap<String, Py<PyAny>>>,
        edge_filter: Option<Py<PyAny>>,
    ) -> PyResult<Option<Py<Node>>> {
        let self_handle: Py<Node> = slf.into();

        // A node created by a Vertex can look the target up and search from
        // both ends at once (its graph keeps edges/inverse_edges in sync).
        if let Some(target) = vertex_node(py, &self_handle, &target_id) {
            let path = bidirectional_bfs(py, &self_handle, &target, depth, Direction::Out, |py, edge| {
                edge_matches_filter(py, edge, &filter, &edge_filter)
            })?;
            return Ok(path.map(|_| target));
        }
        bfs_search_iterative(py, self_handle, target_id, depth, &filter, &edge_filter)
    }

    /// Retrieve a value from ``attr`` by key.
    /// Returns ``None`` if the key does not exist.
    fn attr_get<'py>(&self, py: Python<'py>, key: String) -> Option<Py<PyAny>> {
        self.attr.get(&key).map(|v| v.clone_ref(py))
    }

    /// Set a value in ``attr`` under ``key``.
    /// Fires ``on_update_callbacks`` if the value actually changed.
    fn attr_set(slf: PyRefMut<'_, Self>, py: Python<'_>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let old_value = slf.attr.get(&key).map(|v| v.clone_ref(py));

        // Check whether the value actually changed
        let mut changed = true;
        if let Some(ref old) = old_value {
            let eq_obj = old
                .bind(py)
                .rich_compare(value.bind(py), CompareOp::Eq)?;
            if eq_obj.is_truthy()? {
                changed = false;
            }
        }

        // We need to collect info before the mutable borrow for insert
        let callbacks = slf.on_update_callbacks.clone_ref(py);
        let vertex_ref = slf.vertex.as_ref().map(|v| v.clone_ref(py));
        let self_handle: Py<Node> = slf.into();

        // Insert the new value
        {
            let mut node_ref = self_handle.bind(py).borrow_mut();
            node_ref.attr.insert(key.clone(), value.clone_ref(py));
        }

        // Fire callbacks if changed
        if changed {
            let cb_list = callbacks.bind(py);
            if cb_list.len() > 0 {
                for callback in cb_list.iter() {
                    let cb: Py<PyAny> = callback.into();
                    let result = cb.call1(
                        py,
                        (
                            vertex_ref.as_ref().map(|v| v.clone_ref(py)),
                            self_handle.clone_ref(py),
                            key.clone(),
                            value.clone_ref(py),
                            old_value.as_ref().map(|v| v.clone_ref(py)),
                        ),
                    )?;
                    let should_continue: bool = result.extract(py).unwrap_or(true);
                    if !should_continue {
                        break;
                    }
                }
            }
        }

        Ok(())
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

/// Clone the outgoing edge handles of a node without going through the
/// Python attribute layer. The borrow is released before returning so that
/// Python callbacks invoked afterwards may freely mutate the node.
pub(crate) fn out_edges(py: Python<'_>, node: &Py<Node>) -> Vec<Py<Edge>> {
    node.borrow(py).edges.iter().map(|e| e.clone_ref(py)).collect()
}

/// Look `id` up in the Vertex that owns `node`, if it has one. The Vertex
/// borrow is released before returning.
fn vertex_node(py: Python<'_>, node: &Py<Node>, id: &str) -> Option<Py<Node>> {
    let vertex = node.borrow(py).vertex.as_ref()?.clone_ref(py);
    let vertex = vertex.bind(py).downcast::<Vertex>().ok()?.borrow();
    vertex.nodes.get(id).map(|n| n.clone_ref(py))
}

/// Target node of an edge.
pub(crate) fn edge_target(py: Python<'_>, edge: &Py<Edge>) -> Py<Node> {
    edge.borrow(py).to_node.clone_ref(py)
}

/// Id of a node.
pub(crate) fn node_id(py: Python<'_>, node: &Py<Node>) -> String {
    node.borrow(py).id.clone()
}

/// Call `visit(target_node, target_id)` for every outgoing edge of `node`
/// that passes the filters. Without filters no Python code can run during
/// the scan, so edges are read in place (no handle cloning, no id copies).
fn for_each_target(
    py: Python<'_>,
    node: &Py<Node>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
    edge_filter: &Option<Py<PyAny>>,
    mut visit: impl FnMut(&Py<Node>, &str),
) -> PyResult<()> {
    if filter.is_none() && edge_filter.is_none() {
        let n = node.borrow(py);
        for edge in &n.edges {
            let e = edge.borrow(py);
            let target = e.to_node.borrow(py);
            visit(&e.to_node, &target.id);
        }
        return Ok(());
    }
    // Filters may call back into Python, which could touch the node, so
    // work on a snapshot of the edge list without holding any borrow.
    for edge in out_edges(py, node) {
        if edge_matches_filter(py, &edge, filter, edge_filter)? {
            let to_node = edge_target(py, &edge);
            let to_id = node_id(py, &to_node);
            visit(&to_node, &to_id);
        }
    }
    Ok(())
}

// Helper function to check if an edge matches the filter criteria
fn edge_matches_filter(
    py: Python<'_>,
    edge: &Py<Edge>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
    edge_filter: &Option<Py<PyAny>>,
) -> PyResult<bool> {
    // Check dict-based filter first: only the filtered keys are looked up,
    // the edge's attribute map is never copied.
    if let Some(filter_map) = filter {
        for (filter_key, filter_value) in filter_map {
            let edge_value = edge.borrow(py).attr.get(filter_key).map(|v| v.clone_ref(py));
            match edge_value {
                Some(edge_value) => {
                    if !edge_value.bind(py).eq(filter_value.bind(py))? {
                        return Ok(false);
                    }
                }
                // Edge doesn't have the required attribute
                None => return Ok(false),
            }
        }
    }

    // Check callable edge_filter
    if let Some(ref callable) = edge_filter {
        let result = callable.call1(py, (edge.clone_ref(py),))?;
        let passes: bool = result.extract(py)?;
        if !passes {
            return Ok(false);
        }
    }

    Ok(true)
}

// Iterative depth-first traversal. An explicit stack of frames replaces
// recursion so that very deep graphs (long chains) cannot overflow the
// native stack. Visiting order and depth semantics match the former
// recursive implementation: pre-order, first visit wins.
fn traverse_iterative(
    py: Python<'_>,
    start_node: Py<Node>,
    depth: Option<usize>,
    found: &mut HashMap<String, Py<Node>>,
    visited: &mut HashSet<String>,
    nodelist: &mut Vec<String>,
    filter: &Option<HashMap<String, Py<PyAny>>>,
    edge_filter: &Option<Py<PyAny>>,
) -> PyResult<()> {
    // Frame: (outgoing edges of the node, index of next edge, depth of node)
    let mut stack: Vec<(Vec<Py<Edge>>, usize, usize)> = Vec::new();

    let enter = |node: Py<Node>,
                     current_depth: usize,
                     stack: &mut Vec<(Vec<Py<Edge>>, usize, usize)>,
                     found: &mut HashMap<String, Py<Node>>,
                     visited: &mut HashSet<String>,
                     nodelist: &mut Vec<String>| {
        let id = node_id(py, &node);
        if !visited.insert(id.clone()) {
            return;
        }
        nodelist.push(id.clone());
        let within_depth = depth.map_or(true, |d| current_depth < d);
        if within_depth {
            stack.push((out_edges(py, &node), 0, current_depth));
        }
        found.insert(id, node);
    };

    enter(start_node, 0, &mut stack, found, visited, nodelist);

    while let Some(frame) = stack.last_mut() {
        if frame.1 >= frame.0.len() {
            stack.pop();
            continue;
        }
        let edge = frame.0[frame.1].clone_ref(py);
        frame.1 += 1;
        let next_depth = frame.2 + 1;

        if edge_matches_filter(py, &edge, filter, edge_filter)? {
            let to_node = edge_target(py, &edge);
            enter(to_node, next_depth, &mut stack, found, visited, nodelist);
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
    edge_filter: &Option<Py<PyAny>>,
) -> PyResult<()> {
    use std::collections::VecDeque;

    // Queue stores (node, current_depth)
    let mut queue = VecDeque::new();

    let start_id = node_id(py, &start_node);

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

        for_each_target(py, &current_node, filter, edge_filter, |to_node, to_id| {
            // If not visited, mark and enqueue
            if !visited.contains(to_id) {
                visited.insert(to_id.to_owned());
                found.insert(to_id.to_owned(), to_node.clone_ref(py));
                nodelist.push(to_id.to_owned());
                queue.push_back((to_node.clone_ref(py), current_depth + 1));
            }
        })?;
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
    edge_filter: &Option<Py<PyAny>>,
) -> PyResult<Option<Py<Node>>> {
    use std::collections::VecDeque;

    // Queue stores (node, current_depth)
    let mut queue = VecDeque::new();
    let mut visited = HashSet::<String>::new();

    let start_id = node_id(py, &start_node);

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

        let mut hit: Option<Py<Node>> = None;
        for_each_target(py, &current_node, filter, edge_filter, |to_node, to_id| {
            if hit.is_some() {
                return;
            }
            // If this is our target, remember it
            if to_id == target_id {
                hit = Some(to_node.clone_ref(py));
                return;
            }
            // If not visited, mark and enqueue
            if !visited.contains(to_id) {
                visited.insert(to_id.to_owned());
                queue.push_back((to_node.clone_ref(py), current_depth + 1));
            }
        })?;
        if hit.is_some() {
            return Ok(hit);
        }
    }

    // Target not found
    Ok(None)
}
