// vertex/query.rs
//
// `Vertex.match` (pattern matching) and `Node.paths` (variable-length
// paths): Python options -> core `Pattern` / `expand_paths`, results ->
// `Node` / `Edge` / `Path` handles. Matching reads attributes through `Expr`
// filters, so it runs with the GIL held and the Vertex borrowed (no Python
// callbacks are called).

use ironweaver_core::query::{expand_paths, find_matches, Bound as Binding, Hops, Pattern, Uniqueness};
use ironweaver_core::{Direction, EdgeIx, Expr, NodeIx};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use super::Vertex;
use crate::errors::{graph_error, Error};
use crate::expr::PyExpr;
use crate::gc_pause::GcPause;
use crate::interrupt;
use crate::{Edge, Node, Path};

fn to_expr(value: &Bound<'_, PyAny>, what: &str) -> PyResult<Expr> {
    match value.cast::<PyExpr>() {
        Ok(e) => Ok(e.get().inner.clone()),
        Err(_) => Err(PyTypeError::new_err(format!(
            "{} takes Expr conditions (attr(...), label(...), edge_type(...)), got {}",
            what,
            value.get_type().name()?
        ))),
    }
}

/// `str` or a list of `str`.
fn one_or_many(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    match value.extract::<String>() {
        Ok(s) => Ok(vec![s]),
        Err(_) => value.extract::<Vec<String>>(),
    }
}

/// `Vertex.match(pattern, *, where=None, ids=None, limit=None)`.
pub fn match_pattern(
    slf: &Bound<'_, Vertex>,
    pattern: &str,
    r#where: Option<&Bound<'_, PyDict>>,
    ids: Option<&Bound<'_, PyDict>>,
    limit: Option<usize>,
) -> PyResult<Py<PyList>> {
    let py = slf.py();
    let mut p = Pattern::parse(pattern).map_err(graph_error)?;
    if let Some(w) = r#where {
        for (name, cond) in w.iter() {
            let name: String = name.extract()?;
            p.add_filter(&name, to_expr(&cond, "where")?).map_err(graph_error)?;
        }
    }
    if let Some(ids) = ids {
        for (name, value) in ids.iter() {
            let name: String = name.extract()?;
            p.bind_ids(&name, one_or_many(&value)?).map_err(graph_error)?;
        }
    }
    let matches = {
        let v = slf.try_borrow()?;
        interrupt::polling(py, || find_matches::<_, _, Error>(&v.graph, &p, limit))??
    };

    let _gc = GcPause::new(py);
    let handle = slf.clone().unbind();
    let node_keys: Vec<Option<Bound<'_, PyString>>> =
        p.nodes.iter().map(|n| n.name.as_deref().map(|s| PyString::intern(py, s))).collect();
    let edge_keys: Vec<Option<Bound<'_, PyString>>> =
        p.edges.iter().map(|e| e.name.as_deref().map(|s| PyString::intern(py, s))).collect();
    let out = PyList::empty(py);
    for m in matches {
        let row = PyDict::new(py);
        for (key, &ix) in node_keys.iter().zip(&m.nodes) {
            if let Some(key) = key {
                row.set_item(key, Node::handle(py, &handle, ix)?)?;
            }
        }
        for (key, bound) in edge_keys.iter().zip(&m.edges) {
            let Some(key) = key else { continue };
            match bound {
                Binding::Edge(e) => row.set_item(key, Edge::handle(py, &handle, *e)?)?,
                Binding::Path(edges) => {
                    let list: Vec<Py<Edge>> =
                        edges.iter().map(|&e| Edge::handle(py, &handle, e)).collect::<PyResult<_>>()?;
                    row.set_item(key, list)?
                }
            }
        }
        out.append(row)?;
    }
    Ok(out.unbind())
}

/// Options of `Node.paths`.
pub struct PathOptions<'py> {
    pub min_hops: usize,
    pub max_hops: Option<usize>,
    pub direction: Option<&'py str>,
    pub types: Option<Bound<'py, PyAny>>,
    pub r#where: Option<Bound<'py, PyAny>>,
    pub uniqueness: &'py str,
    pub limit: Option<usize>,
}

/// `Node.paths(...)`: variable-length paths from `start`.
pub fn node_paths(py: Python<'_>, vertex: &Py<Vertex>, start: NodeIx, opts: PathOptions<'_>) -> PyResult<Py<PyList>> {
    let direction = Direction::parse(opts.direction).map_err(graph_error)?;
    let uniqueness: Uniqueness = opts.uniqueness.parse().map_err(graph_error)?;
    let types: Option<Vec<String>> = match &opts.types {
        Some(t) if !t.is_none() => Some(one_or_many(t)?),
        _ => None,
    };
    let filter = match &opts.r#where {
        Some(w) if !w.is_none() => Some(to_expr(w, "where")?),
        _ => None,
    };
    if opts.limit == Some(0) {
        return Ok(PyList::empty(py).unbind());
    }
    let hops = Hops { min: opts.min_hops, max: opts.max_hops };
    let mut found: Vec<(Vec<EdgeIx>, Vec<NodeIx>)> = Vec::new();
    {
        let v = vertex.try_borrow(py)?;
        let g = &v.graph;
        let wanted: Option<Vec<_>> = types.as_ref().map(|t| t.iter().filter_map(|name| g.symbol(name)).collect());
        interrupt::polling(py, || {
            expand_paths::<_, _, Error>(
                g,
                start,
                direction,
                hops,
                uniqueness,
                |e, edge| {
                    if let Some(w) = &wanted
                        && !edge.edge_type().is_some_and(|t| w.contains(&t))
                    {
                        return Ok(false);
                    }
                    match &filter {
                        Some(f) => Ok(f.matches_edge(g, e)?),
                        None => Ok(true),
                    }
                },
                |edges, nodes| {
                    found.push((edges.to_vec(), nodes.to_vec()));
                    Ok(opts.limit.is_none_or(|l| found.len() < l))
                },
            )
        })??;
    }
    let _gc = GcPause::new(py);
    let out = PyList::empty(py);
    for (edges, nodes) in found {
        let path = Path {
            nodes: nodes.iter().map(|&ix| Node::handle(py, vertex, ix)).collect::<PyResult<_>>()?,
            edges: edges.iter().map(|&e| Edge::handle(py, vertex, e)).collect::<PyResult<_>>()?,
        };
        out.append(Py::new(py, path)?)?;
    }
    Ok(out.unbind())
}
