// vertex/index.rs
//
// Property indexes (`Vertex.create_index`, `find`, `find_range`) and keeping
// them current. The core graph owns the indexes; reading a node's value may
// run Python code, so keys are computed here while the vertex is *not*
// borrowed mutably, then stored (`set_index_keys`). A node whose keys could
// not be computed stays marked dirty, and lookups re-read it.

use std::ops::Bound as Limit;

use ironweaver_core::{CmpOp, Expr, Key, NodeIx, Value};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use super::Vertex;
use crate::convert::to_value;
use crate::data::{index_key, PyAttrs};
use crate::errors::graph_error;
use crate::Node;

/// Recompute the index keys of `nodes` after their attributes changed.
/// Errors while reading values are not raised: those nodes stay dirty.
pub fn reindex(py: Python<'_>, vertex: &Py<Vertex>, nodes: &[NodeIx]) -> PyResult<()> {
    let (paths, attrs) = {
        let v = vertex.try_borrow(py)?;
        let paths: Vec<Vec<String>> = v.graph.index_paths().into_iter().map(<[String]>::to_vec).collect();
        if paths.is_empty() {
            return Ok(());
        }
        let attrs: Vec<(NodeIx, PyAttrs)> =
            nodes.iter().filter_map(|&ix| v.graph.node(ix).map(|n| (ix, n.data.attr.share(py)))).collect();
        (paths, attrs)
    };
    let mut keyed = Vec::with_capacity(attrs.len());
    for (ix, attr) in &attrs {
        let keys: PyResult<Vec<Option<Key>>> = paths.iter().map(|p| index_key(attr, p)).collect();
        if let Ok(keys) = keys {
            keyed.push((*ix, keys));
        }
    }
    // Release the shared dicts first (they'd make the next write copy)
    drop(attrs);
    let mut v = vertex.try_borrow_mut(py)?;
    for (ix, keys) in keyed {
        // Fails only if the indexes changed meanwhile: the node stays dirty
        let _ = v.graph.set_index_keys(ix, keys);
    }
    Ok(())
}

/// `Vertex.create_index(name)`.
pub fn create_index(py: Python<'_>, vertex: &Py<Vertex>, name: String) -> PyResult<bool> {
    let path = vec![name];
    let attrs: Vec<(NodeIx, PyAttrs)> = {
        let v = vertex.try_borrow(py)?;
        if v.graph.has_index(&path) {
            return Ok(false);
        }
        v.graph.nodes().map(|(ix, n)| (ix, n.data.attr.share(py))).collect()
    };
    let mut keys = Vec::with_capacity(attrs.len());
    for (ix, attr) in &attrs {
        keys.push((*ix, index_key(attr, &path)?));
    }
    drop(attrs);
    vertex.try_borrow_mut(py)?.graph.create_index_with_keys(&path, keys).map_err(graph_error)
}

fn handles(py: Python<'_>, vertex: &Py<Vertex>, found: Vec<NodeIx>) -> PyResult<Vec<Py<Node>>> {
    found.into_iter().map(|ix| Node::handle(py, vertex, ix)).collect()
}

/// Nodes matching `expr`: through the index if there is one, else a scan.
fn select(py: Python<'_>, vertex: &Py<Vertex>, expr: &Expr) -> PyResult<Vec<Py<Node>>> {
    let found = {
        let v = vertex.try_borrow(py)?;
        let g = &v.graph;
        let mut out = Vec::new();
        match g.index_candidates(expr)? {
            Some(candidates) => {
                for ix in candidates {
                    if expr.matches_node(g, ix)? {
                        out.push(ix);
                    }
                }
            }
            None => {
                for ix in g.node_indices() {
                    if expr.matches_node(g, ix)? {
                        out.push(ix);
                    }
                }
            }
        }
        out
    };
    handles(py, vertex, found)
}

/// `Vertex.find(name, value)`.
pub fn find(py: Python<'_>, vertex: &Py<Vertex>, name: String, value: &Bound<'_, PyAny>) -> PyResult<Vec<Py<Node>>> {
    let expr = Expr::Compare { path: vec![name], op: CmpOp::Eq, value: to_value(value)? };
    select(py, vertex, &expr)
}

/// `Vertex.find_range(name, low, high, inclusive)`.
pub fn find_range(
    py: Python<'_>,
    vertex: &Py<Vertex>,
    name: String,
    low: Option<&Bound<'_, PyAny>>,
    high: Option<&Bound<'_, PyAny>>,
    inclusive: &str,
) -> PyResult<Vec<Py<Node>>> {
    let (low_in, high_in) = match inclusive {
        "both" => (true, true),
        "left" => (true, false),
        "right" => (false, true),
        "neither" => (false, false),
        other => {
            return Err(PyValueError::new_err(format!(
                "inclusive must be \"both\", \"left\", \"right\" or \"neither\", got {other:?}"
            )))
        }
    };
    let bound = |b: Option<&Bound<'_, PyAny>>| -> PyResult<Option<Value>> {
        b.filter(|b| !b.is_none()).map(to_value).transpose()
    };
    let (low, high) = (bound(low)?, bound(high)?);
    let path = vec![name];
    // Validate the bounds (scalars of one kind) the way the index does
    let check = |b: &Option<Value>, incl: bool| match b {
        Some(v) if incl => Limit::Included(v.clone()),
        Some(v) => Limit::Excluded(v.clone()),
        None => Limit::Unbounded,
    };
    let (lo, hi) = (check(&low, low_in), check(&high, high_in));
    let key = |b: &Limit<Value>| -> PyResult<Option<Key>> {
        match b {
            Limit::Included(v) | Limit::Excluded(v) => Key::of(v).map(Some).ok_or_else(|| {
                PyValueError::new_err("range bounds must be numbers, strings, bools, bytes, dates or datetimes")
            }),
            Limit::Unbounded => Ok(None),
        }
    };
    match (key(&lo)?, key(&hi)?) {
        (None, None) => return Err(PyValueError::new_err("find_range needs low or high")),
        (Some(a), Some(b)) if !a.same_kind(&b) => {
            return Err(PyValueError::new_err("low and high must be of the same kind"))
        }
        _ => {}
    }
    let mut parts = Vec::new();
    let mut compare = |op, value: Value| parts.push(Expr::Compare { path: path.clone(), op, value });
    match lo {
        Limit::Included(v) => compare(CmpOp::Ge, v),
        Limit::Excluded(v) => compare(CmpOp::Gt, v),
        Limit::Unbounded => {}
    }
    match hi {
        Limit::Included(v) => compare(CmpOp::Le, v),
        Limit::Excluded(v) => compare(CmpOp::Lt, v),
        Limit::Unbounded => {}
    }
    // Both halves narrow through the index; the tighter one is scanned
    let expr = if parts.len() == 1 { parts.pop().expect("one") } else { Expr::And(parts) };
    select(py, vertex, &expr)
}
