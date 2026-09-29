// vertex/core.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use crate::data::{split_reserved, AttrMap, EdgeData, NodeData, PyAttrs, PyGraph};
use crate::errors::graph_error;
use crate::{Edge, Node};

// Import the helper modules as sibling modules
use super::algorithms;
use super::analysis;
use super::batch;
use super::callbacks::fire;
use super::manipulation;
use super::pathfinding;
use super::serialization;

/// A directed graph: a collection of `Node`s, keyed by id, connected by `Edge`s.
///
/// Build it with `add_node(id, attr=None)` / `add_edge(from_id, to_id, attr=None)`,
/// look nodes up with `graph[id]` or `get_node(id)`, and use the algorithms
/// (`filter`, `expand`, `shortest_path` with method "bfs", "dijkstra" or "astar",
/// `random_walks`, ...) which return new `Vertex` objects.
///
/// The graph data lives in Rust (`ironweaver_core::Graph`); `Node` and `Edge`
/// objects are handles into it. `nodes` returns a new id -> Node dict (add/remove
/// nodes with the methods, not by editing that dict). `meta` is a live dict for
/// your own graph-level data, and the `on_*_callbacks` lists are live too.
#[pyclass]
pub struct Vertex {
    pub graph: PyGraph,
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

impl Vertex {
    /// A Vertex around `graph` with an empty `meta` and no callbacks.
    pub fn with_graph(py: Python<'_>, graph: PyGraph) -> Self {
        Vertex {
            graph,
            meta: PyDict::new(py).into(),
            on_node_add_callbacks: PyList::empty(py).into(),
            on_edge_add_callbacks: PyList::empty(py).into(),
            on_node_update_callbacks: PyList::empty(py).into(),
            on_edge_update_callbacks: PyList::empty(py).into(),
        }
    }

    /// Copy the given nodes (which may come from several vertices) and the
    /// edges between nodes of the same source vertex into a new graph.
    fn copy_nodes(py: Python<'_>, nodes: &Bound<'_, PyDict>) -> PyResult<PyGraph> {
        use std::collections::HashMap;
        let mut graph = PyGraph::with_capacity(nodes.len(), 0);
        let mut copied = Vec::with_capacity(nodes.len());
        let mut map = HashMap::with_capacity(nodes.len());
        for (key, node) in nodes.iter() {
            let id: String = key.extract()?;
            let node = node.cast::<Node>()?.get();
            let source = node.vertex.try_borrow(py)?;
            let data = source
                .graph
                .node(node.ix)
                .ok_or_else(|| graph_error(ironweaver_core::GraphError::Stale))?
                .data
                .copy(py)?;
            let new = graph.add_node(id, data).map_err(graph_error)?;
            map.insert((node.vertex.as_ptr() as usize, node.ix), new);
            copied.push((node.vertex.clone_ref(py), node.ix, new));
        }
        for (vertex, old, new) in copied {
            let source = vertex.try_borrow(py)?;
            let key = vertex.as_ptr() as usize;
            for &e in source.graph.node(old).expect("checked above").out_edges() {
                let edge = source.graph.edge(e).expect("listed edges are live");
                if let Some(&to) = map.get(&(key, edge.target())) {
                    graph.add_edge(new, to, edge.data.copy(py)?).map_err(graph_error)?;
                }
            }
        }
        Ok(graph)
    }
}

#[pymethods]
impl Vertex {
    #[new]
    fn new(py: Python<'_>) -> Self {
        Vertex::with_graph(py, PyGraph::new())
    }

    /// Create a graph from an ``{id: Node}`` mapping.
    ///
    /// The nodes are copied (with their ``attr`` / ``meta``), together with
    /// the edges between them.
    #[staticmethod]
    pub fn from_nodes(py: Python<'_>, nodes: &Bound<'_, PyDict>) -> PyResult<Self> {
        Ok(Vertex::with_graph(py, Vertex::copy_nodes(py, nodes)?))
    }

    /// Like ``from_nodes``, and stores ``nodelist`` in ``meta["nodelist"]``.
    #[staticmethod]
    pub fn from_nodes_with_path(py: Python<'_>, nodes: &Bound<'_, PyDict>, nodelist: Vec<String>) -> PyResult<Self> {
        let vertex = Vertex::from_nodes(py, nodes)?;
        vertex.meta.bind(py).set_item("nodelist", nodelist)?;
        Ok(vertex)
    }

    // Garbage-collector support: attribute values may reference the graph's
    // own nodes, edges or the vertex (cycles through the handles).
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        let mut result = Ok(());
        let mut call = |obj: &Py<PyAny>| {
            if result.is_ok() {
                result = visit.call(obj);
            }
        };
        for (_, node) in self.graph.nodes() {
            node.data.visit(&mut call);
        }
        for (_, edge) in self.graph.edges() {
            edge.data.visit(&mut call);
        }
        result?;
        visit.call(&self.meta)?;
        visit.call(&self.on_node_add_callbacks)?;
        visit.call(&self.on_edge_add_callbacks)?;
        visit.call(&self.on_node_update_callbacks)?;
        visit.call(&self.on_edge_update_callbacks)?;
        Ok(())
    }

    fn __clear__(&mut self) {
        self.graph = PyGraph::new();
    }

    fn __getitem__(slf: &Bound<'_, Self>, key: String) -> PyResult<Py<Node>> {
        let ix = slf.try_borrow()?.graph.node_ix(&key);
        match ix {
            Some(ix) => Node::handle(slf.py(), &slf.clone().unbind(), ix),
            None => Err(pyo3::exceptions::PyKeyError::new_err(key)),
        }
    }

    /// Return the ids of all nodes (in graph order).
    fn keys(&self) -> Vec<String> {
        self.graph.nodes().map(|(_, n)| n.id().to_string()).collect()
    }

    fn __repr__(&self) -> String {
        let keys: Vec<&str> = self.graph.nodes().map(|(_, n)| n.id()).collect();
        format!("Vertex({})", keys.join(", "))
    }

    /// A new ``{id: Node}`` dict of all nodes.
    #[getter]
    fn nodes<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        let py = slf.py();
        let handle = slf.clone().unbind();
        let dict = PyDict::new(py);
        for (ix, node) in slf.try_borrow()?.graph.nodes() {
            dict.set_item(node.id(), Node::handle(py, &handle, ix)?)?;
        }
        Ok(dict)
    }

    /// Return a ``{id: Node}`` dict (for JSON encoders). Use ``save_to_json``
    /// to serialize the whole graph.
    #[allow(non_snake_case)]
    fn toJSON<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        Vertex::nodes(slf)
    }

    /// Check if a node with the given ID exists
    ///
    /// Args:
    ///     id (str): The node ID to check
    ///     
    /// Returns:
    ///     bool: True if the node exists, False otherwise
    fn has_node(&self, id: &str) -> bool {
        self.graph.contains_node(id)
    }

    /// Get the number of nodes in the graph
    ///
    /// Returns:
    ///     int: The number of nodes
    fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Add a new node to the graph
    ///
    /// Args:
    ///     id (str): Unique identifier for the node
    ///     attr (dict, optional): Attributes for the node. A "labels" entry
    ///         holding a list of str becomes the node's labels.
    ///     labels (list[str], optional): Labels of the node
    ///
    /// Returns:
    ///     Node: The created node
    ///
    /// Raises:
    ///     ValueError: If a node with the same ID already exists
    #[pyo3(signature = (id, attr=None, labels=None))]
    fn add_node(
        slf: &Bound<'_, Self>,
        id: String,
        attr: Option<Bound<'_, PyDict>>,
        labels: Option<Vec<String>>,
    ) -> PyResult<Py<Node>> {
        let py = slf.py();
        let (attr, from_attr) = split_reserved(attr.as_ref(), "labels")?;
        let mut labels = labels.unwrap_or_default();
        if let Some(l) = from_attr {
            labels.extend(l.extract::<Vec<String>>()?);
        }
        let data = NodeData::new(attr, PyAttrs::default());
        let (ix, callbacks) = {
            let mut v = slf.try_borrow_mut()?;
            let ix = v.graph.add_node(id, data).map_err(graph_error)?;
            for label in &labels {
                v.graph.add_label(ix, label).map_err(graph_error)?;
            }
            (ix, v.on_node_add_callbacks.clone_ref(py))
        };
        let vertex = slf.clone().unbind();
        super::index::reindex(py, &vertex, &[ix])?;
        let node = Node::handle(py, &vertex, ix)?;
        if !callbacks.bind(py).is_empty() {
            let args = PyTuple::new(py, [vertex.into_any(), node.clone_ref(py).into_any()])?;
            fire(callbacks.bind(py), args)?;
        }
        Ok(node)
    }

    /// Add many nodes in one call: each item is an id or ``(id, attrs)``;
    /// ``labels`` go on every node (an item's ``attrs["labels"]`` adds its
    /// own). ``attrs`` gives attributes as columns, ``{name: [one value per
    /// node]}`` (None: not set), which is faster than a dict per node.
    /// Checks every item first, so on an error nothing is added. Returns the
    /// number of nodes added. Add-callbacks fire after the batch.
    ///
    /// Raises:
    ///     ValueError: A duplicate id
    ///     TypeError: An item of the wrong shape
    #[pyo3(signature = (nodes, *, labels=None, attrs=None))]
    fn add_nodes(
        slf: &Bound<'_, Self>,
        nodes: &Bound<'_, PyAny>,
        labels: Option<Vec<String>>,
        attrs: Option<Bound<'_, PyDict>>,
    ) -> PyResult<usize> {
        super::bulk::add_nodes(slf, nodes, labels, attrs.as_ref())
    }

    /// Add many edges in one call: each item is ``(from_id, to_id)`` or
    /// ``(from_id, to_id, attrs)``; ``type`` applies to every edge (else an
    /// item's ``attrs["type"]``). ``attrs`` gives attributes as columns,
    /// ``{name: [one value per edge]}`` (None: not set; a ``"type"`` column
    /// sets the types), which is faster than a dict per edge. Checks every
    /// item first, so on an error nothing is added. Returns the number of
    /// edges added. Add-callbacks fire after the batch.
    ///
    /// Raises:
    ///     ValueError: An unknown node id
    ///     TypeError: An item of the wrong shape
    #[pyo3(signature = (edges, *, r#type=None, attrs=None))]
    fn add_edges(
        slf: &Bound<'_, Self>,
        edges: &Bound<'_, PyAny>,
        r#type: Option<String>,
        attrs: Option<Bound<'_, PyDict>>,
    ) -> PyResult<usize> {
        super::bulk::add_edges(slf, edges, r#type, attrs.as_ref())
    }

    /// Add a new edge between two nodes in the graph
    ///
    /// Args:
    ///     from_id (str): ID of the source node
    ///     to_id (str): ID of the target node
    ///     attr (dict, optional): Attributes for the edge. A "type" entry
    ///         holding a str becomes the edge's type.
    ///     type (str, optional): Type of the edge
    ///
    /// Returns:
    ///     Edge: The created edge (``edge.id`` is its persistent integer id)
    ///
    /// Raises:
    ///     ValueError: If either node doesn't exist
    #[pyo3(signature = (from_id, to_id, attr=None, r#type=None))]
    fn add_edge(
        slf: &Bound<'_, Self>,
        from_id: &str,
        to_id: &str,
        attr: Option<Bound<'_, PyDict>>,
        r#type: Option<String>,
    ) -> PyResult<Py<Edge>> {
        let py = slf.py();
        let (attr, from_attr) = split_reserved(attr.as_ref(), "type")?;
        let ty = match (r#type, from_attr) {
            (Some(t), _) => Some(t),
            (None, Some(t)) => Some(t.extract::<String>()?),
            (None, None) => None,
        };
        let data = EdgeData::new(attr, PyAttrs::default());
        let (ix, callbacks) = {
            let mut v = slf.try_borrow_mut()?;
            let lookup = |id: &str| {
                v.graph
                    .node_ix(id)
                    .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Node with id '{}' not found", id)))
            };
            let (from, to) = (lookup(from_id)?, lookup(to_id)?);
            let ix = v.graph.insert_edge(from, to, None, ty.as_deref(), data).map_err(graph_error)?;
            (ix, v.on_edge_add_callbacks.clone_ref(py))
        };
        let vertex = slf.clone().unbind();
        let edge = Edge::handle(py, &vertex, ix)?;
        if !callbacks.bind(py).is_empty() {
            let args = PyTuple::new(py, [vertex.into_any(), edge.clone_ref(py).into_any()])?;
            fire(callbacks.bind(py), args)?;
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
    fn get_node(slf: &Bound<'_, Self>, id: &str) -> PyResult<Py<Node>> {
        let ix = slf
            .try_borrow()?
            .graph
            .node_ix(id)
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("Node with id '{}' not found", id)))?;
        Node::handle(slf.py(), &slf.clone().unbind(), ix)
    }

    /// The edge with this persistent id (``edge.id``)
    ///
    /// Raises:
    ///     KeyError: If no edge has this id
    fn get_edge(slf: &Bound<'_, Self>, id: u64) -> PyResult<Py<Edge>> {
        let ix = slf
            .try_borrow()?
            .graph
            .edge_ix(ironweaver_core::EdgeId(id))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("Edge with id {} not found", id)))?;
        Edge::handle(slf.py(), &slf.clone().unbind(), ix)
    }

    /// Every occurrence of a pattern, as a list of dicts ``{name: Node | Edge |
    /// list[Edge]}`` (named variables only)
    ///
    /// The pattern is Cypher-like: ``(a:Person {name: "Ann"})-[k:KNOWS*1..3]->(b)``,
    /// with ``<-[...]-`` and ``-[...]-`` (either direction), ``-->`` / ``<--`` /
    /// ``--``, several paths separated by commas, and a name used twice meaning
    /// the same node. Each edge variable binds a different edge; nodes may repeat.
    ///
    /// Args:
    ///     pattern (str): The pattern.
    ///     where (dict, optional): ``{name: Expr}`` extra conditions per variable.
    ///     ids (dict, optional): ``{name: id or [ids]}`` fixes node variables.
    ///     limit (int, optional): Stop after this many matches.
    ///
    /// Raises:
    ///     ValueError: An invalid pattern or an unknown variable name
    #[pyo3(name = "match", signature = (pattern, *, r#where=None, ids=None, limit=None))]
    fn match_pattern(
        slf: &Bound<'_, Self>,
        pattern: &str,
        r#where: Option<Bound<'_, PyDict>>,
        ids: Option<Bound<'_, PyDict>>,
        limit: Option<usize>,
    ) -> PyResult<Py<PyList>> {
        super::query::match_pattern(slf, pattern, r#where.as_ref(), ids.as_ref(), limit)
    }

    /// Index node attribute ``name`` for fast ``find`` / ``find_range``
    /// lookups; ``filter(where=...)`` and ``match`` use it too. Kept up to
    /// date as attributes change; not saved with the graph.
    ///
    /// Returns:
    ///     bool: False if the index already existed
    fn create_index(slf: &Bound<'_, Self>, name: String) -> PyResult<bool> {
        super::index::create_index(slf.py(), &slf.clone().unbind(), name)
    }

    /// Drop the index on ``name``; returns whether there was one.
    fn drop_index(&mut self, name: String) -> bool {
        self.graph.drop_index(&[name])
    }

    /// Names of the indexed node attributes, in creation order.
    #[getter]
    fn indexes(&self) -> Vec<String> {
        self.graph.index_paths().into_iter().map(|p| p.join(".")).collect()
    }

    /// Nodes whose attribute ``name`` equals ``value`` (numbers compare
    /// across int and float), in graph order. Uses the index on ``name`` if
    /// there is one, else scans every node.
    fn find(slf: &Bound<'_, Self>, name: String, value: &Bound<'_, PyAny>) -> PyResult<Vec<Py<Node>>> {
        super::index::find(slf.py(), &slf.clone().unbind(), name, value)
    }

    /// Nodes whose attribute ``name`` lies between ``low`` and ``high``
    /// (either may be None for no bound), in graph order. ``inclusive`` is
    /// "both" (default), "left", "right" or "neither". Bounds are numbers,
    /// strings, bools, bytes, dates or datetimes, of one kind; values of
    /// other kinds are never in range. Uses the index on ``name`` if any.
    #[pyo3(signature = (name, low=None, high=None, *, inclusive="both"))]
    fn find_range(
        slf: &Bound<'_, Self>,
        name: String,
        low: Option<Bound<'_, PyAny>>,
        high: Option<Bound<'_, PyAny>>,
        inclusive: &str,
    ) -> PyResult<Vec<Py<Node>>> {
        super::index::find_range(slf.py(), &slf.clone().unbind(), name, low.as_ref(), high.as_ref(), inclusive)
    }

    /// Nodes carrying ``label``, in graph order (uses the label index)
    fn nodes_with_label(slf: &Bound<'_, Self>, label: &str) -> PyResult<Vec<Py<Node>>> {
        let vertex = slf.clone().unbind();
        let ixs = slf.try_borrow()?.graph.nodes_with_label(label);
        ixs.into_iter().map(|ix| Node::handle(slf.py(), &vertex, ix)).collect()
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

    /// Approximate memory used by the graph, in bytes: its structure (nodes,
    /// edges, ids, adjacency, labels, indexes). With ``deep=True``, plus
    /// the attribute and meta dicts and the values in them
    /// (``sys.getsizeof``, containers recursively, shared objects once).
    #[pyo3(signature = (*, deep=false))]
    fn memory_usage(slf: &Bound<'_, Self>, deep: bool) -> PyResult<usize> {
        analysis::memory_usage(slf, deep)
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
    /// Deprecated: use ``shortest_path(root, target, method="bfs")``.
    ///
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
        deprecated(py, "shortest_path_bfs", "bfs")?;
        let options = PyDict::new(py);
        options.set_item("max_depth", max_depth)?;
        pathfinding::shortest_path(
            self,
            py,
            root_node_id,
            target_node_id,
            Some("bfs"),
            None,
            None,
            None,
            direction,
            Some(options),
        )
    }

    /// Deprecated: use ``shortest_path(root, target, method="dijkstra")``.
    ///
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
    #[allow(clippy::too_many_arguments)]
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
        deprecated(py, "shortest_path_dijkstra", "dijkstra")?;
        pathfinding::shortest_path(
            self,
            py,
            root_node_id,
            target_node_id,
            Some("dijkstra"),
            weight,
            default_weight,
            max_cost,
            direction,
            None,
        )
    }

    /// Find a shortest path between two nodes with the chosen algorithm
    ///
    /// Args:
    ///     source (str): ID of the start node
    ///     target (str): ID of the end node
    ///     method (str, optional): "bfs" (fewest edges), "dijkstra" (cheapest by
    ///         edge weight) or "astar" (cheapest, guided by a heuristic); see
    ///         `Vertex.path_methods()`. None picks "astar" if heuristic, coords or
    ///         distances is given, "dijkstra" if weight is given, else "bfs".
    ///     weight (str, optional): Edge attribute holding the edge cost. Defaults to "weight".
    ///     default_weight (float, optional): Cost of edges without that attribute. Defaults to 1.0.
    ///     max_cost (float, optional): Ignore paths more expensive than this (for "bfs": more edges).
    ///     direction (str, optional): "out" (default), "in" or "both".
    ///     **options: Method-specific options:
    ///         bfs: max_depth (int).
    ///         astar: heuristic ("euclidean" default, or "manhattan") with coords, where
    ///             node coordinates live: a list of attribute paths, one per dimension
    ///             (default ["x", "y"]; "pos.lat" reads attr["pos"]["lat"]), or one
    ///             attribute holding a sequence ("pos"). Or distances="<key>":
    ///             precomputed estimates in vertex.meta[key], either {node_id: estimate}
    ///             or {node_id: {target_id: estimate}}. Nodes without coordinates or
    ///             estimates count as 0. Estimates must not overestimate the remaining
    ///             cost, or the path may not be the cheapest.
    ///
    /// Returns:
    ///     Vertex: The path's nodes (copies) and the edges between them, with
    ///         meta["nodelist"] (path in order), meta["cost"] (total weight, or
    ///         number of edges for "bfs"), meta["method"] and, for "dijkstra" and
    ///         "astar", meta["expanded"] (nodes settled)
    ///
    /// Raises:
    ///     ValueError: Unknown method, missing node, unreachable target, negative
    ///         weight, or target without coordinates
    ///     TypeError: Option not accepted by the method, or a non-numeric weight,
    ///         coordinate or estimate
    #[pyo3(signature = (source, target, method=None, *, weight=None, default_weight=None, max_cost=None, direction=None, **options))]
    #[allow(clippy::too_many_arguments)]
    fn shortest_path(
        &self,
        py: Python<'_>,
        source: String,
        target: String,
        method: Option<&str>,
        weight: Option<String>,
        default_weight: Option<f64>,
        max_cost: Option<f64>,
        direction: Option<&str>,
        options: Option<Bound<'_, PyDict>>,
    ) -> PyResult<Py<Vertex>> {
        pathfinding::shortest_path(
            self,
            py,
            source,
            target,
            method,
            weight,
            default_weight,
            max_cost,
            direction,
            options,
        )
    }

    /// The available `shortest_path` methods, as {name: description}
    #[staticmethod]
    fn path_methods(py: Python<'_>) -> PyResult<Py<PyDict>> {
        pathfinding::path_methods(py)
    }

    /// A compact, read-only copy of (part of) the graph for analytics
    ///
    /// Copies the structure (sorted neighbour lists in both directions), at
    /// most one weight per edge and the node ids, but no attributes. Build it
    /// once and run many queries on it (`shortest_paths`, `distances`, ...):
    /// they run on every core with the GIL released. The projection is a
    /// snapshot; later changes to the graph don't affect it.
    ///
    /// Args:
    ///     weight (str, optional): Edge attribute holding the weight. Without
    ///         weight and default_weight the projection is unweighted.
    ///     default_weight (float, optional): Weight of edges without the
    ///         attribute. Defaults to 1.0 (implies weight="weight").
    ///     direction (str, optional): "out" (default) keeps edges as they are,
    ///         "in" reverses them, "both" makes them undirected.
    ///     nodes (list[str], optional): Only these nodes (and edges between them).
    ///     node_filter (dict | callable, optional): Keep nodes whose attributes
    ///         equal the dict's values / for which the callable is true.
    ///     edge_filter (dict | callable, optional): The same for edges between kept nodes.
    ///
    /// Returns:
    ///     Projection
    ///
    /// Raises:
    ///     ValueError: Unknown node id or direction, or a negative edge weight
    ///     TypeError: A non-numeric edge weight, or a filter of the wrong type
    #[pyo3(signature = (weight=None, default_weight=None, *, direction=None, nodes=None, node_filter=None, edge_filter=None))]
    #[allow(clippy::too_many_arguments)]
    fn project(
        slf: &Bound<'_, Self>,
        weight: Option<String>,
        default_weight: Option<f64>,
        direction: Option<&str>,
        nodes: Option<Vec<String>>,
        node_filter: Option<Bound<'_, PyAny>>,
        edge_filter: Option<Bound<'_, PyAny>>,
    ) -> PyResult<crate::projection::Projection> {
        batch::project(slf, weight, default_weight, direction, nodes, node_filter.as_ref(), edge_filter.as_ref())
    }

    /// Shortest paths for many (source, target) pairs, computed in parallel
    ///
    /// The graph and its edge costs are copied into a projection once, then
    /// all queries run on every core with the GIL released. Use it for
    /// batches; for a single query `shortest_path` is cheaper, and to run
    /// several batches, build the projection once with `project`.
    ///
    /// Args:
    ///     pairs (list[tuple[str, str]]): (source_id, target_id) pairs
    ///     method (str, optional): "bfs" (fewest edges) or "dijkstra" (cheapest by
    ///         weight). None picks "dijkstra" if weight is given, else "bfs".
    ///     weight (str, optional): Edge attribute holding the cost. Defaults to "weight".
    ///     default_weight (float, optional): Cost of edges without it. Defaults to 1.0.
    ///     max_cost (float, optional): Ignore paths more expensive than this (for
    ///         "bfs": with more edges).
    ///     direction (str, optional): "out" (default), "in" or "both".
    ///
    /// Returns:
    ///     list: One entry per pair, in order: {"nodelist": [...], "cost": ...}, or
    ///         None if the target is not reachable
    ///
    /// Raises:
    ///     ValueError: Unknown node or method ("astar" is not available here), or a
    ///         negative edge weight anywhere in the graph
    ///     TypeError: A non-numeric edge weight anywhere in the graph
    #[pyo3(signature = (pairs, method=None, *, weight=None, default_weight=None, max_cost=None, direction=None))]
    #[allow(clippy::too_many_arguments)]
    fn shortest_paths(
        &self,
        py: Python<'_>,
        pairs: Vec<(String, String)>,
        method: Option<&str>,
        weight: Option<String>,
        default_weight: Option<f64>,
        max_cost: Option<f64>,
        direction: Option<&str>,
    ) -> PyResult<Py<PyList>> {
        batch::shortest_paths(self, py, pairs, method, weight, default_weight, max_cost, direction)
    }

    /// Costs from each source to every node it reaches, computed in parallel
    ///
    /// Like `shortest_paths`, runs on a compact snapshot with the GIL released.
    ///
    /// Args:
    ///     sources (list[str]): Start node ids
    ///     targets (list[str], optional): Only report these nodes
    ///     method, weight, default_weight, max_cost, direction: As in `shortest_paths`
    ///
    /// Returns:
    ///     dict: {source_id: {node_id: cost}} for every node reached within max_cost
    ///         (cost is the number of edges for "bfs"), including the source itself
    ///
    /// Raises:
    ///     ValueError / TypeError: As in `shortest_paths`
    #[pyo3(signature = (sources, targets=None, method=None, *, weight=None, default_weight=None, max_cost=None, direction=None))]
    #[allow(clippy::too_many_arguments)]
    fn distances(
        &self,
        py: Python<'_>,
        sources: Vec<String>,
        targets: Option<Vec<String>>,
        method: Option<&str>,
        weight: Option<String>,
        default_weight: Option<f64>,
        max_cost: Option<f64>,
        direction: Option<&str>,
    ) -> PyResult<Py<PyDict>> {
        batch::distances(self, py, sources, targets, method, weight, default_weight, max_cost, direction)
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
    fn filter(&self, py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<Vertex>> {
        algorithms::filter(self, py, kwargs)
    }

    /// Remove a node and all edges attached to it
    ///
    /// Args:
    ///     id (str): ID of the node to remove
    ///
    /// Returns:
    ///     Node: The removed node, detached: a copy in a new one-node Vertex
    ///         (same id, attr and meta, no edges)
    ///
    /// Raises:
    ///     KeyError: If no node with the given ID exists
    fn remove_node(slf: &Bound<'_, Self>, id: &str) -> PyResult<Py<Node>> {
        manipulation::remove_node(slf, id)
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
    fn remove_edge(slf: &Bound<'_, Self>, from_id: &str, to_id: &str, attr: Option<AttrMap>) -> PyResult<usize> {
        manipulation::remove_edge(slf, from_id, to_id, attr)
    }

    /// Remove edges that reference nodes not present in the vertex.
    ///
    /// Kept for compatibility: edges always connect two nodes of their own
    /// graph, so there is never anything to remove.
    ///
    /// Returns:
    ///     int: The number of edges removed (always 0)
    fn prune(&self) -> usize {
        0
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
    #[allow(clippy::too_many_arguments)]
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

/// Warn that a legacy method is deprecated in favour of `shortest_path`.
fn deprecated(py: Python<'_>, name: &str, method: &str) -> PyResult<()> {
    let message = format!(
        "{name}() is deprecated and will be removed in a future release; \
         use shortest_path(root, target, method=\"{method}\") instead"
    );
    let message = std::ffi::CString::new(message).expect("no NUL bytes");
    PyErr::warn(py, &py.get_type::<pyo3::exceptions::PyDeprecationWarning>(), &message, 1)
}
