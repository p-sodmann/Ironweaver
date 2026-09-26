// vertex/pathfinding/heuristic.rs
//
// Estimates of the remaining cost from a node to the target, for A*.
// All sources are read in Rust; `best_first::search` asks for each node's
// estimate once, when the node is first reached.

use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use super::super::core::Vertex;
use super::PathQuery;
use crate::Node;

#[derive(Clone, Copy)]
pub enum Metric {
    Euclidean,
    Manhattan,
}

impl Metric {
    fn parse(name: Option<String>) -> PyResult<Self> {
        match name.as_deref().unwrap_or("euclidean") {
            "euclidean" => Ok(Metric::Euclidean),
            "manhattan" => Ok(Metric::Manhattan),
            other => Err(PyValueError::new_err(format!(
                "unknown heuristic '{}'; use 'euclidean' or 'manhattan' (with coords=...), \
                 or distances='<meta key>' for precomputed estimates",
                other
            ))),
        }
    }

    fn distance(self, a: &[f64], b: &[f64]) -> f64 {
        match self {
            Metric::Euclidean => a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>().sqrt(),
            Metric::Manhattan => a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum(),
        }
    }
}

/// Where a node's coordinates live. A path is an attribute name followed by
/// dict keys (`"pos.lat"` -> attr["pos"]["lat"]).
pub enum Coords {
    /// One path to a sequence of numbers (`coords="pos"`).
    Sequence(Vec<String>),
    /// One path per dimension (`coords=["x", "y"]`).
    PerDimension(Vec<Vec<String>>),
}

fn split_path(path: &str) -> Vec<String> {
    path.split('.').map(str::to_string).collect()
}

fn show(path: &[String]) -> String {
    path.join(".")
}

impl Coords {
    fn parse(value: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let value = match value {
            None => return Ok(Coords::PerDimension(vec![vec!["x".into()], vec!["y".into()]])),
            Some(v) => v,
        };
        if let Ok(s) = value.downcast::<PyString>() {
            return Ok(Coords::Sequence(split_path(&s.to_cow()?)));
        }
        let paths: Vec<String> = value.extract().map_err(|_| {
            PyTypeError::new_err(
                "coords must be an attribute name holding a sequence (e.g. \"pos\") \
                 or a list of attribute paths, one per dimension (e.g. [\"x\", \"y\"] or [\"pos.lat\", \"pos.lon\"])",
            )
        })?;
        if paths.is_empty() {
            return Err(PyValueError::new_err("coords must name at least one dimension"));
        }
        Ok(Coords::PerDimension(paths.iter().map(|p| split_path(p)).collect()))
    }

    fn describe(&self) -> String {
        match self {
            Coords::Sequence(p) => format!("'{}'", show(p)),
            Coords::PerDimension(ps) => {
                format!("[{}]", ps.iter().map(|p| format!("'{}'", show(p))).collect::<Vec<_>>().join(", "))
            }
        }
    }

    /// The node's coordinates, or `None` if any part is missing.
    fn read(&self, py: Python<'_>, node: &Py<Node>) -> PyResult<Option<Vec<f64>>> {
        match self {
            Coords::Sequence(path) => match lookup(py, node, path)? {
                None => Ok(None),
                Some(v) => v.extract::<Vec<f64>>().map(Some).map_err(|_| {
                    PyTypeError::new_err(format!(
                        "coordinates at '{}' of node '{}' must be a sequence of numbers",
                        show(path),
                        node.borrow(py).id
                    ))
                }),
            },
            Coords::PerDimension(paths) => {
                let mut out = Vec::with_capacity(paths.len());
                for path in paths {
                    match lookup(py, node, path)? {
                        None => return Ok(None),
                        Some(v) => out.push(v.extract::<f64>().map_err(|_| {
                            PyTypeError::new_err(format!(
                                "coordinate '{}' of node '{}' must be a number",
                                show(path),
                                node.borrow(py).id
                            ))
                        })?),
                    }
                }
                Ok(Some(out))
            }
        }
    }
}

/// Follow `path` from the node's attributes; `None` if a step is missing.
fn lookup<'py>(py: Python<'py>, node: &Py<Node>, path: &[String]) -> PyResult<Option<Bound<'py, PyAny>>> {
    let mut value = match node.borrow(py).attr.get(&path[0]) {
        Some(v) => v.bind(py).clone(),
        None => return Ok(None),
    };
    for key in &path[1..] {
        value = if let Ok(d) = value.downcast::<PyDict>() {
            match d.get_item(key)? {
                Some(v) => v,
                None => return Ok(None),
            }
        } else {
            match value.get_item(key) {
                Ok(v) => v,
                Err(e) if e.is_instance_of::<PyKeyError>(py) => return Ok(None),
                Err(e) => return Err(e),
            }
        };
        if value.is_none() {
            return Ok(None);
        }
    }
    Ok(if value.is_none() { None } else { Some(value) })
}

pub enum Heuristic<'py> {
    /// No estimate (Dijkstra).
    Zero,
    /// Distance between node and target coordinates.
    Coords { metric: Metric, coords: Coords, target: Vec<f64> },
    /// Precomputed estimates from `vertex.meta[name]`:
    /// `{node_id: estimate}` or `{node_id: {target_id: estimate}}`.
    Table { name: String, table: Bound<'py, PyDict>, target_id: String },
}

impl<'py> Heuristic<'py> {
    /// Build the heuristic from A*'s `heuristic` / `coords` / `distances` options.
    pub fn from_query(py: Python<'py>, vertex: &Vertex, q: &PathQuery<'py>) -> PyResult<Self> {
        let metric = q.options.get::<String>("heuristic")?;
        let coords = q.options.raw("coords")?;

        if let Some(name) = q.options.get::<String>("distances")? {
            if metric.is_some() || coords.is_some() {
                return Err(PyValueError::new_err(
                    "use either distances=... or heuristic/coords=..., not both",
                ));
            }
            let table = vertex.meta.bind(py).get_item(&name)?.ok_or_else(|| {
                PyValueError::new_err(format!("distances: vertex.meta has no key '{}'", name))
            })?;
            let table = table.downcast_into::<PyDict>().map_err(|_| {
                PyTypeError::new_err(format!(
                    "distances: vertex.meta['{}'] must be a dict {{node_id: estimate}} \
                     or {{node_id: {{target_id: estimate}}}}",
                    name
                ))
            })?;
            return Ok(Heuristic::Table { name, table, target_id: q.target_id.clone() });
        }

        let metric = Metric::parse(metric)?;
        let coords = Coords::parse(coords)?;
        let target = coords.read(py, &q.target)?.ok_or_else(|| {
            PyValueError::new_err(format!(
                "target node '{}' has no coordinates at coords={}",
                q.target_id,
                coords.describe()
            ))
        })?;
        Ok(Heuristic::Coords { metric, coords, target })
    }

    /// Estimated cost from `node` to the target (0 when unknown).
    pub fn estimate(&self, py: Python<'py>, node: &Py<Node>) -> PyResult<f64> {
        match self {
            Heuristic::Zero => Ok(0.0),
            Heuristic::Coords { metric, coords, target } => match coords.read(py, node)? {
                None => Ok(0.0),
                Some(c) if c.len() != target.len() => Err(PyValueError::new_err(format!(
                    "node '{}' has {} coordinates but the target has {}",
                    node.borrow(py).id,
                    c.len(),
                    target.len()
                ))),
                Some(c) => Ok(metric.distance(&c, target)),
            },
            Heuristic::Table { name, table, target_id } => {
                let n = node.borrow(py);
                let entry = match table.get_item(&n.id)? {
                    Some(v) if !v.is_none() => v,
                    _ => return Ok(0.0),
                };
                let value = match entry.downcast::<PyDict>() {
                    Ok(per_target) => match per_target.get_item(target_id)? {
                        Some(v) if !v.is_none() => v,
                        _ => return Ok(0.0),
                    },
                    Err(_) => entry,
                };
                value.extract::<f64>().map_err(|_| {
                    PyTypeError::new_err(format!(
                        "distances: vertex.meta['{}'] entry for node '{}' must be a number \
                         or a dict {{target_id: number}}",
                        name, n.id
                    ))
                })
            }
        }
    }
}
