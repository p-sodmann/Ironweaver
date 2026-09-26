// vertex/pathfinding/mod.rs
//
// Shortest-path algorithms behind one entry point, `Vertex.shortest_path(...,
// method=...)`. Each algorithm is a `PathMethod` in `METHODS`; the entry point
// resolves the method, validates its options, builds a `PathQuery` and turns
// the algorithm's `PathResult` into the result `Vertex`.
//
// Adding an algorithm:
//   1. Write `my_algo.rs` with a `pub const METHOD: PathMethod` whose `find`
//      returns `Ok(Some(PathResult))`, or `Ok(None)` if the target is not
//      reachable. Reuse `EdgeCost` (edge weights), `Heuristic` (estimates)
//      and `best_first::search` where they fit.
//   2. Add `mod my_algo;` below and `my_algo::METHOD` to `METHODS`.
//   3. List its options in `METHOD.options`, read them with
//      `query.options.get(...)`; unknown options are rejected for you.
//   4. Update the `.pyi` stubs, docs/traversal.md, llms.txt and add tests.

mod astar;
mod best_first;
mod bfs;
mod cost;
mod dijkstra;
mod heuristic;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashSet;

use super::core::Vertex;
use super::subgraph::{build_subgraph, Direction};
use crate::Node;

pub use cost::EdgeCost;

/// A registered shortest-path algorithm.
pub struct PathMethod {
    /// Value of the `method` argument.
    pub name: &'static str,
    /// One-line description, returned by `Vertex.path_methods()`.
    pub description: &'static str,
    /// Algorithm-specific keyword options it accepts.
    pub options: &'static [&'static str],
    /// Whether it uses edge weights (`weight` / `default_weight`).
    pub weighted: bool,
    pub find: for<'py> fn(Python<'py>, &Vertex, &PathQuery<'py>) -> PyResult<Option<PathResult>>,
}

/// All algorithms, in the order `path_methods()` lists them.
pub const METHODS: &[PathMethod] = &[bfs::METHOD, dijkstra::METHOD, astar::METHOD];

/// Options whose presence makes `method=None` pick A*.
const ASTAR_OPTIONS: &[&str] = &["heuristic", "coords", "distances"];

/// Everything an algorithm needs to answer one query.
pub struct PathQuery<'py> {
    pub source: Py<Node>,
    pub target: Py<Node>,
    pub source_id: String,
    pub target_id: String,
    pub direction: Direction,
    pub cost: EdgeCost,
    pub max_cost: Option<f64>,
    pub options: Options<'py>,
}

/// Algorithm-specific keyword options (entries set to `None` are absent).
pub struct Options<'py>(Option<Bound<'py, PyDict>>);

impl<'py> Options<'py> {
    pub fn raw(&self, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        match &self.0 {
            Some(d) => Ok(d.get_item(name)?.filter(|v| !v.is_none())),
            None => Ok(None),
        }
    }

    pub fn get<T: for<'a> FromPyObject<'a>>(&self, name: &str) -> PyResult<Option<T>> {
        match self.raw(name)? {
            Some(v) => v.extract::<T>().map(Some).map_err(|e| {
                PyTypeError::new_err(format!("invalid value for option '{}': {}", name, e))
            }),
            None => Ok(None),
        }
    }

    fn names(&self) -> PyResult<Vec<String>> {
        let mut out = Vec::new();
        if let Some(d) = &self.0 {
            for (k, v) in d.iter() {
                if !v.is_none() {
                    out.push(k.extract::<String>()?);
                }
            }
        }
        Ok(out)
    }
}

/// A found path: node ids in source -> target order and its total cost.
pub struct PathResult {
    pub node_ids: Vec<String>,
    pub cost: f64,
    /// Nodes the algorithm settled, if it keeps count.
    pub expanded: Option<usize>,
}

pub fn method_names() -> String {
    METHODS.iter().map(|m| format!("'{}'", m.name)).collect::<Vec<_>>().join(", ")
}

/// `Vertex.path_methods()`: {name: description}.
pub fn path_methods(py: Python<'_>) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    for m in METHODS {
        d.set_item(m.name, m.description)?;
    }
    Ok(d.into())
}

fn resolve<'a>(
    method: Option<&str>,
    weight: &Option<String>,
    options: &Options<'_>,
) -> PyResult<&'static PathMethod> {
    let name = match method {
        Some(m) => m,
        None => {
            let names = options.names()?;
            if names.iter().any(|n| ASTAR_OPTIONS.contains(&n.as_str())) {
                "astar"
            } else if weight.is_some() {
                "dijkstra"
            } else {
                "bfs"
            }
        }
    };
    METHODS.iter().find(|m| m.name == name).ok_or_else(|| {
        PyValueError::new_err(format!(
            "unknown path method '{}'; available: {}",
            name,
            method_names()
        ))
    })
}

/// Entry point behind `Vertex.shortest_path` (and the `shortest_path_bfs` /
/// `shortest_path_dijkstra` shorthands).
#[allow(clippy::too_many_arguments)]
pub fn shortest_path<'py>(
    vertex: &Vertex,
    py: Python<'py>,
    source_id: String,
    target_id: String,
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
    options: Option<Bound<'py, PyDict>>,
) -> PyResult<Py<Vertex>> {
    let options = Options(options);
    let method = resolve(method, &weight, &options)?;

    for name in options.names()? {
        if !method.options.contains(&name.as_str()) {
            let accepted = if method.options.is_empty() {
                "none".to_string()
            } else {
                method.options.join(", ")
            };
            return Err(PyTypeError::new_err(format!(
                "method '{}' does not accept option '{}' (accepted options: {})",
                method.name, name, accepted
            )));
        }
    }

    let cost = if method.weighted {
        EdgeCost::weighted(weight, default_weight)
    } else {
        if weight.is_some() || default_weight.is_some() {
            return Err(PyValueError::new_err(format!(
                "method '{}' ignores edge weights; use method='dijkstra' or 'astar' for weighted paths",
                method.name
            )));
        }
        EdgeCost::Unit
    };
    if let Some(c) = max_cost {
        if !(c >= 0.0) {
            return Err(PyValueError::new_err(format!("max_cost must be non-negative, got {}", c)));
        }
    }

    let direction = Direction::parse(direction)?;
    let source = vertex.nodes.get(&source_id).ok_or_else(|| {
        PyValueError::new_err(format!("Root node with id '{}' not found", source_id))
    })?.clone_ref(py);
    let target = vertex.nodes.get(&target_id).ok_or_else(|| {
        PyValueError::new_err(format!("Target node with id '{}' not found", target_id))
    })?.clone_ref(py);

    let query = PathQuery { source, target, source_id, target_id, direction, cost, max_cost, options };
    match (method.find)(py, vertex, &query)? {
        Some(result) => path_vertex(py, vertex, method, result),
        None => Err(not_reachable(method, &query)),
    }
}

fn not_reachable(method: &PathMethod, q: &PathQuery<'_>) -> PyErr {
    let mut msg = format!("Target node '{}' not reachable from '{}'", q.target_id, q.source_id);
    if let Ok(Some(d)) = q.options.get::<usize>("max_depth") {
        msg += &format!(" within max_depth {}", d);
    }
    if let Some(c) = q.max_cost {
        msg += &format!(" within max_cost {}", c);
    }
    msg += &format!(" (method '{}')", method.name);
    PyValueError::new_err(msg)
}

/// Build the result vertex for a path: the path nodes, every edge between
/// them, and meta with `nodelist` (source -> target order), `cost`, `method`
/// and, when known, `expanded`.
fn path_vertex(
    py: Python<'_>,
    vertex: &Vertex,
    method: &PathMethod,
    result: PathResult,
) -> PyResult<Py<Vertex>> {
    let path_set: HashSet<String> = result.node_ids.iter().cloned().collect();
    let meta = PyDict::new(py);
    meta.set_item("nodelist", &result.node_ids)?;
    if method.weighted {
        meta.set_item("cost", result.cost)?;
    } else {
        meta.set_item("cost", result.node_ids.len() - 1)?;
    }
    meta.set_item("method", method.name)?;
    if let Some(n) = result.expanded {
        meta.set_item("expanded", n)?;
    }
    build_subgraph(py, vertex, &path_set, meta.into(), false)
}
