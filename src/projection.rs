// projection.rs
//
// The Python `Projection` class (a core `Projection`: compact, read-only
// copy of a graph for analytics) and the helpers `Vertex.project`,
// `Vertex.shortest_paths` and `Vertex.distances` share.
//
// Building reads attribute values and may call Python filters, so it runs
// with the GIL held (and the Vertex borrowed); sorting and every query then
// run with the GIL released. A Projection owns its data, so it stays valid
// (a snapshot) when the graph changes afterwards.

use ironweaver_core::algo;
use ironweaver_core::batch::{self, DensePath};
use ironweaver_core::pathfinding::{resolve, EdgeCost, MethodKind};
use ironweaver_core::projection::RawProjection;
use ironweaver_core::{Direction, NodeIx};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use std::collections::{HashMap, HashSet};

use crate::data::PyAttrs;
use crate::errors::{graph_error, Error};
use crate::expr::PyExpr;
use crate::gc_pause::GcPause;
use crate::{Edge, Node, Vertex};

/// A node or edge filter: attribute equality (a dict), an `Expr`
/// (evaluated in Rust) or a callable taking the `Node` / `Edge` (Python's
/// `project` wrapper passes views).
pub(crate) struct Filter<'py> {
    wanted: Vec<(Bound<'py, PyString>, Bound<'py, PyAny>)>,
    callable: Option<Bound<'py, PyAny>>,
    expr: Option<ironweaver_core::Expr>,
}

impl<'py> Filter<'py> {
    /// `None`, a dict (attribute equality), an `Expr` or a callable.
    pub(crate) fn parse(py: Python<'py>, spec: Option<&Bound<'py, PyAny>>, what: &str) -> PyResult<Self> {
        let mut filter = Filter { wanted: Vec::new(), callable: None, expr: None };
        match spec {
            None => {}
            Some(s) if s.is_none() => {}
            Some(s) => {
                if let Ok(d) = s.cast::<PyDict>() {
                    for (k, v) in d.iter() {
                        let key: String = k.extract()?;
                        filter.wanted.push((PyString::intern(py, &key), v));
                    }
                } else if let Ok(e) = s.cast::<PyExpr>() {
                    filter.expr = Some(e.get().inner.clone());
                } else if s.is_callable() {
                    filter.callable = Some(s.clone());
                } else {
                    return Err(pyo3::exceptions::PyTypeError::new_err(format!(
                        "{} must be a dict of attribute values, an Expr or a callable",
                        what
                    )));
                }
            }
        }
        Ok(filter)
    }

    fn is_empty(&self) -> bool {
        self.wanted.is_empty() && self.callable.is_none() && self.expr.is_none()
    }

    /// Whether every wanted attribute equals the value in `attrs`; the key
    /// `reserved` ("labels" / "type") is answered by `field` instead.
    fn attrs_match(
        &self,
        py: Python<'py>,
        attrs: &PyAttrs,
        reserved: &str,
        field: impl Fn() -> PyResult<Option<Bound<'py, PyAny>>>,
    ) -> PyResult<bool> {
        for (key, expected) in &self.wanted {
            let value = if key.to_str()? == reserved {
                field()?
            } else {
                match attrs.dict(py) {
                    Some(d) => d.get_item(key)?,
                    None => None,
                }
            };
            match value {
                Some(value) if value.eq(expected)? => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    fn call(&self, arg: Py<PyAny>) -> PyResult<bool> {
        match &self.callable {
            Some(f) => f.call1((arg,))?.is_truthy(),
            None => Ok(true),
        }
    }
}

/// Everything `Vertex.project` takes besides the vertex.
pub(crate) struct Spec<'py> {
    pub direction: Direction,
    pub cost: EdgeCost,
    pub nodes: Option<Vec<String>>,
    pub node_filter: Filter<'py>,
    pub edge_filter: Filter<'py>,
}

/// The weight options of `project`: unweighted unless `weight` or
/// `default_weight` is given.
pub(crate) fn edge_cost(weight: Option<String>, default_weight: Option<f64>) -> EdgeCost {
    if weight.is_none() && default_weight.is_none() {
        EdgeCost::Unit
    } else {
        EdgeCost::weighted(weight, default_weight)
    }
}

/// Read the projected part of the vertex (GIL held, vertex borrowed).
/// `handle` is needed only for callable filters.
pub(crate) fn collect(
    py: Python<'_>,
    vertex: &Vertex,
    handle: Option<&Py<Vertex>>,
    spec: &Spec<'_>,
) -> PyResult<RawProjection> {
    let keep: Option<HashSet<NodeIx>> = match &spec.nodes {
        None => None,
        Some(ids) => Some(
            ids.iter()
                .map(|id| {
                    vertex
                        .graph
                        .node_ix(id)
                        .ok_or_else(|| PyValueError::new_err(format!("Node with id '{}' not found", id)))
                })
                .collect::<PyResult<_>>()?,
        ),
    };
    let needs_handle = spec.node_filter.callable.is_some() || spec.edge_filter.callable.is_some();
    let handle = match handle {
        Some(h) => Some(h),
        None if needs_handle => {
            return Err(PyValueError::new_err("callable filters need the Vertex"));
        }
        None => None,
    };
    let (nf, ef) = (&spec.node_filter, &spec.edge_filter);
    let raw = ironweaver_core::Projection::collect::<_, _, Error>(
        &vertex.graph,
        spec.direction,
        &spec.cost,
        |ix, node| {
            if keep.as_ref().is_some_and(|k| !k.contains(&ix)) {
                return Ok(false);
            }
            if nf.is_empty() {
                return Ok(true);
            }
            let labels = || crate::data::node_value(py, &vertex.graph, ix, "labels");
            if !nf.attrs_match(py, &node.data.attr, "labels", labels)? {
                return Ok(false);
            }
            if let Some(x) = &nf.expr {
                if !x.matches_node(&vertex.graph, ix)? {
                    return Ok(false);
                }
            }
            Ok(match handle {
                Some(h) if nf.callable.is_some() => nf.call(Node::handle(py, h, ix)?.into_any())?,
                _ => true,
            })
        },
        |e, edge| {
            if ef.is_empty() {
                return Ok(true);
            }
            let ty = || crate::data::edge_value(py, &vertex.graph, e, "type");
            if !ef.attrs_match(py, &edge.data.attr, "type", ty)? {
                return Ok(false);
            }
            if let Some(x) = &ef.expr {
                if !x.matches_edge(&vertex.graph, e)? {
                    return Ok(false);
                }
            }
            Ok(match handle {
                Some(h) if ef.callable.is_some() => ef.call(Edge::handle(py, h, e)?.into_any())?,
                _ => true,
            })
        },
    )?;
    Ok(raw)
}

/// A compact, read-only copy of (part of) a graph for analytics, made by
/// `Vertex.project(...)`.
///
/// It keeps the structure (sorted neighbour lists, in both directions), at
/// most one edge weight per edge and the node ids, but no attributes. It is
/// a snapshot: later changes to the graph don't affect it. Queries run on all
/// cores with the GIL released; build a projection once and run many
/// queries on it.
#[pyclass(frozen, module = "ironweaver")]
pub struct Projection {
    pub(crate) inner: ironweaver_core::Projection,
}

impl Projection {
    /// Dense index of `id`; `what` names the node in the error ("Root node").
    fn index(&self, id: &str, what: &str) -> PyResult<u32> {
        self.inner
            .index_of_id(id)
            .ok_or_else(|| PyValueError::new_err(format!("{} with id '{}' not found in the projection", what, id)))
    }

    /// `method=None` picks dijkstra on a weighted projection, bfs otherwise.
    fn uses_weights(&self, method: Option<&str>) -> PyResult<bool> {
        resolved_weighted(method, self.inner.is_weighted())
    }
}

/// Whether a batch query with `method` uses weights (`dijkstra`) or hop
/// counts (`bfs`); `None` picks by `weighted`.
pub(crate) fn resolved_weighted(method: Option<&str>, weighted: bool) -> PyResult<bool> {
    let m = resolve(method, weighted, &[]).map_err(graph_error)?;
    if m.kind == MethodKind::AStar {
        return Err(PyValueError::new_err(
            "method 'astar' is not available for batch queries; use 'bfs' or 'dijkstra'",
        ));
    }
    Ok(m.weighted)
}

/// A cost as Python sees it: an int for hop counts, a float otherwise.
fn cost_object(py: Python<'_>, weighted: bool, cost: f64) -> PyResult<Py<PyAny>> {
    Ok(if weighted {
        cost.into_pyobject(py)?.into_any().unbind()
    } else {
        (cost as usize).into_pyobject(py)?.into_any().unbind()
    })
}

/// `[{"nodelist": [...], "cost": c} | None, ...]`
pub(crate) fn paths_to_py(
    py: Python<'_>,
    p: &ironweaver_core::Projection,
    results: Vec<Option<DensePath>>,
    weighted: bool,
) -> PyResult<Py<PyList>> {
    let _gc = GcPause::new(py);
    let out = PyList::empty(py);
    for result in results {
        match result {
            None => out.append(py.None())?,
            Some(path) => {
                let entry = PyDict::new(py);
                let ids: Vec<&str> = path.nodes.iter().map(|&u| p.id(u)).collect();
                entry.set_item("nodelist", ids)?;
                entry.set_item("cost", cost_object(py, weighted, path.cost)?)?;
                out.append(entry)?;
            }
        }
    }
    Ok(out.unbind())
}

/// `{source: {node: cost}}`, restricted to `targets` if given.
pub(crate) fn distances_to_py(
    py: Python<'_>,
    p: &ironweaver_core::Projection,
    sources: &[u32],
    results: Vec<Vec<(u32, f64)>>,
    targets: Option<&HashSet<u32>>,
    weighted: bool,
) -> PyResult<Py<PyDict>> {
    let _gc = GcPause::new(py);
    let out = PyDict::new(py);
    for (&source, reached) in sources.iter().zip(results) {
        let per_source = PyDict::new(py);
        for (u, c) in reached {
            if targets.is_none_or(|t| t.contains(&u)) {
                per_source.set_item(p.id(u), cost_object(py, weighted, c)?)?;
            }
        }
        out.set_item(p.id(source), per_source)?;
    }
    Ok(out.unbind())
}

/// `{id: value}` for every node.
fn per_node<'py, T: IntoPyObject<'py> + Copy>(
    py: Python<'py>,
    p: &ironweaver_core::Projection,
    values: &[T],
) -> PyResult<Py<PyDict>> {
    let _gc = GcPause::new(py);
    let out = PyDict::new(py);
    for (u, &value) in values.iter().enumerate() {
        out.set_item(p.id(u as u32), value)?;
    }
    Ok(out.unbind())
}

/// Groups of nodes as lists of ids.
fn groups_to_py(py: Python<'_>, p: &ironweaver_core::Projection, groups: &[Vec<u32>]) -> PyResult<Py<PyList>> {
    let _gc = GcPause::new(py);
    let out = PyList::empty(py);
    for g in groups {
        out.append(g.iter().map(|&u| p.id(u)).collect::<Vec<_>>())?;
    }
    Ok(out.unbind())
}

fn parse_side(direction: Option<&str>) -> PyResult<bool> {
    match direction.unwrap_or("out") {
        "out" => Ok(true),
        "in" => Ok(false),
        other => Err(PyValueError::new_err(format!("direction must be 'out' or 'in', got '{}'", other))),
    }
}

#[pymethods]
impl Projection {
    /// Number of nodes.
    fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    fn __len__(&self) -> usize {
        self.inner.node_count()
    }

    /// Number of graph edges in the projection.
    fn edge_count(&self) -> usize {
        self.inner.edge_count()
    }

    /// "out", "in" or "both": how edges were followed when projecting
    /// ("in" reverses them, "both" makes them undirected).
    #[getter]
    fn direction(&self) -> &'static str {
        match self.inner.direction() {
            Direction::Out => "out",
            Direction::In => "in",
            Direction::Both => "both",
        }
    }

    /// Whether the projection has edge weights.
    #[getter]
    fn weighted(&self) -> bool {
        self.inner.is_weighted()
    }

    /// Node ids, in the projection's order.
    fn ids(&self) -> Vec<&str> {
        self.inner.ids().iter().map(String::as_str).collect()
    }

    fn __contains__(&self, id: &str) -> bool {
        self.inner.index_of_id(id).is_some()
    }

    /// Ids of the nodes one projected edge away: `direction="out"` follows
    /// edges forwards, `"in"` backwards. One entry per edge (parallel edges
    /// repeat a neighbour).
    #[pyo3(signature = (id, direction=None))]
    fn neighbors(&self, id: &str, direction: Option<&str>) -> PyResult<Vec<&str>> {
        let u = self.index(id, "Node")?;
        let list = if parse_side(direction)? { self.inner.out_neighbors(u) } else { self.inner.in_neighbors(u) };
        Ok(list.iter().map(|&v| self.inner.id(v)).collect())
    }

    /// Number of projected edges leaving (`"out"`) or entering (`"in"`) a node.
    #[pyo3(signature = (id, direction=None))]
    fn degree(&self, id: &str, direction: Option<&str>) -> PyResult<usize> {
        let u = self.index(id, "Node")?;
        Ok(if parse_side(direction)? { self.inner.out_degree(u) } else { self.inner.in_degree(u) })
    }

    /// Approximate memory used by the projection, in bytes.
    fn memory_usage(&self) -> usize {
        self.inner.memory_usage()
    }

    /// Shortest path for each (source_id, target_id) pair, in parallel.
    ///
    /// `method`: "dijkstra" (weights; needs a weighted projection) or "bfs"
    /// (edge count); None picks dijkstra on a weighted projection. Returns
    /// one entry per pair: {"nodelist": [...], "cost": c}, or None if the
    /// target is unreachable within `max_cost`.
    #[pyo3(signature = (pairs, method=None, *, max_cost=None))]
    fn shortest_paths(
        &self,
        py: Python<'_>,
        pairs: Vec<(String, String)>,
        method: Option<&str>,
        max_cost: Option<f64>,
    ) -> PyResult<Py<PyList>> {
        let weighted = self.uses_weights(method)?;
        let dense: Vec<(u32, u32)> = pairs
            .iter()
            .map(|(s, t)| Ok((self.index(s, "Root node")?, self.index(t, "Target node")?)))
            .collect::<PyResult<_>>()?;
        let p = &self.inner;
        let results = py.detach(|| batch::shortest_paths(p, &dense, weighted, max_cost)).map_err(graph_error)?;
        paths_to_py(py, p, results, weighted)
    }

    /// Cost from each source to every node it reaches within `max_cost`
    /// (only `targets`, if given), in parallel: {source: {node: cost}}.
    #[pyo3(signature = (sources, targets=None, method=None, *, max_cost=None))]
    fn distances(
        &self,
        py: Python<'_>,
        sources: Vec<String>,
        targets: Option<Vec<String>>,
        method: Option<&str>,
        max_cost: Option<f64>,
    ) -> PyResult<Py<PyDict>> {
        let weighted = self.uses_weights(method)?;
        let sources: Vec<u32> = sources.iter().map(|s| self.index(s, "Root node")).collect::<PyResult<_>>()?;
        let targets: Option<HashSet<u32>> =
            targets.map(|ts| ts.iter().map(|t| self.index(t, "Target node")).collect::<PyResult<_>>()).transpose()?;
        let p = &self.inner;
        let results = py.detach(|| batch::distances(p, &sources, weighted, max_cost)).map_err(graph_error)?;
        distances_to_py(py, p, &sources, results, targets.as_ref(), weighted)
    }

    /// Weakly connected components (edges in either direction), as lists of
    /// ids: largest first, members in projection order.
    fn weakly_connected_components(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let p = &self.inner;
        let groups = py.detach(|| algo::weakly_connected_components(p));
        groups_to_py(py, p, &groups)
    }

    /// Strongly connected components along the projection's edges, as
    /// lists of ids: largest first, members in projection order.
    fn strongly_connected_components(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let p = &self.inner;
        let groups = py.detach(|| algo::strongly_connected_components(p));
        groups_to_py(py, p, &groups)
    }

    /// Node ids ordered so that every edge goes from an earlier to a later
    /// node (ties in projection order). Raises ValueError on a cycle.
    fn topological_sort(&self, py: Python<'_>) -> PyResult<Vec<&str>> {
        let p = &self.inner;
        let order = py.detach(|| algo::topological_sort(p)).map_err(graph_error)?;
        Ok(order.iter().map(|&u| p.id(u)).collect())
    }

    /// One cycle as a list of ids (the last has an edge back to the first),
    /// or None if the projection has no cycle.
    fn find_cycle(&self, py: Python<'_>) -> Option<Vec<&str>> {
        let p = &self.inner;
        py.detach(|| algo::find_cycle(p)).map(|c| c.iter().map(|&u| p.id(u)).collect())
    }

    /// {id: degree / (n - 1)}, counting edges leaving ("out") or entering
    /// ("in") each node.
    #[pyo3(signature = (direction=None))]
    fn degree_centrality(&self, py: Python<'_>, direction: Option<&str>) -> PyResult<Py<PyDict>> {
        let incoming = !parse_side(direction)?;
        let p = &self.inner;
        let values = py.detach(|| algo::degree_centrality(p, incoming));
        per_node(py, p, &values)
    }

    /// {id: PageRank}, like networkx.pagerank. Uses the projection's
    /// weights if it has them. `personalization` ({id: weight}) biases the
    /// random jumps towards some nodes (personalized PageRank).
    #[pyo3(signature = (alpha=0.85, *, personalization=None, max_iter=100, tol=1e-6))]
    fn pagerank(
        &self,
        py: Python<'_>,
        alpha: f64,
        personalization: Option<HashMap<String, f64>>,
        max_iter: usize,
        tol: f64,
    ) -> PyResult<Py<PyDict>> {
        let p = &self.inner;
        let personalization = match personalization {
            None => None,
            Some(map) => {
                let mut v = vec![0.0; p.node_count()];
                for (id, weight) in map {
                    v[self.index(&id, "Node")? as usize] = weight;
                }
                Some(v)
            }
        };
        let opts = algo::PageRank { alpha, personalization, max_iter, tol };
        let ranks = py.detach(|| algo::pagerank(p, &opts)).map_err(graph_error)?;
        per_node(py, p, &ranks)
    }

    /// {id: number of triangles through the node} (edges as undirected).
    fn triangles(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let p = &self.inner;
        let values = py.detach(|| algo::triangles(p));
        per_node(py, p, &values)
    }

    /// {id: local clustering coefficient} (edges as undirected).
    fn clustering(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let p = &self.inner;
        let values = py.detach(|| algo::clustering(p));
        per_node(py, p, &values)
    }

    /// {id: core number} (k-core decomposition, edges as undirected).
    fn core_number(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let p = &self.inner;
        let values = py.detach(|| algo::core_number(p));
        per_node(py, p, &values)
    }

    /// Communities by synchronous label propagation (edges as undirected;
    /// deterministic), as lists of ids, largest first.
    #[pyo3(signature = (max_iter=20))]
    fn label_propagation(&self, py: Python<'_>, max_iter: usize) -> PyResult<Py<PyList>> {
        let p = &self.inner;
        let (groups, _) = py.detach(|| algo::label_propagation(p, max_iter));
        groups_to_py(py, p, &groups)
    }

    /// {id: hops from the nearest source} for every node reached (within
    /// `max_depth`), by parallel direction-optimizing BFS.
    #[pyo3(signature = (sources, max_depth=None))]
    fn bfs_levels(&self, py: Python<'_>, sources: Vec<String>, max_depth: Option<u32>) -> PyResult<Py<PyDict>> {
        let dense: Vec<u32> = sources.iter().map(|s| self.index(s, "Root node")).collect::<PyResult<_>>()?;
        let p = &self.inner;
        let levels = py.detach(|| algo::bfs_levels(p, &dense, max_depth)).map_err(graph_error)?;
        let _gc = GcPause::new(py);
        let out = PyDict::new(py);
        for (u, &l) in levels.iter().enumerate() {
            if l != algo::NONE {
                out.set_item(p.id(u as u32), l)?;
            }
        }
        Ok(out.unbind())
    }

    fn __repr__(&self) -> String {
        format!(
            "Projection(nodes={}, edges={}, direction='{}', weighted={})",
            self.inner.node_count(),
            self.inner.edge_count(),
            self.direction(),
            if self.inner.is_weighted() { "True" } else { "False" }
        )
    }
}
