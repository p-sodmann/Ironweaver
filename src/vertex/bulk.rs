// vertex/bulk.rs
//
// `Vertex.add_nodes` / `Vertex.add_edges`: many nodes or edges in one call.
// Every item is read and checked first (with the Vertex only borrowed, since
// reading a user iterable can run Python code), then all are inserted at
// once, so a bad item leaves the graph unchanged. Handles are created only
// if add-callbacks are registered; they fire after the whole batch is in.

use ironweaver_core::NodeIx;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};
use std::collections::HashSet;

use super::callbacks::fire;
use super::Vertex;
use crate::data::{split_reserved, EdgeData, NodeData, PyAttrs};
use crate::errors::graph_error;
use crate::gc_pause::GcPause;
use crate::{Edge, Node};

/// The parts of one item: a tuple / list of `min..=max` elements, or (for
/// nodes) a bare id.
fn parts<'py>(item: &Bound<'py, PyAny>, what: &str, min: usize, max: usize) -> PyResult<Vec<Bound<'py, PyAny>>> {
    let parts: Vec<Bound<'py, PyAny>> = if let Ok(t) = item.cast::<PyTuple>() {
        t.iter().collect()
    } else if let Ok(l) = item.cast::<PyList>() {
        l.iter().collect()
    } else {
        return Err(PyTypeError::new_err(format!("each {} must be a tuple or list", what)));
    };
    if parts.len() < min || parts.len() > max {
        return Err(PyValueError::new_err(format!(
            "each {} must have {} to {} elements, got {}",
            what,
            min,
            max,
            parts.len()
        )));
    }
    Ok(parts)
}

fn attrs<'py>(value: Option<&Bound<'py, PyAny>>) -> PyResult<Option<Bound<'py, PyDict>>> {
    match value {
        None => Ok(None),
        Some(v) if v.is_none() => Ok(None),
        Some(v) => Ok(Some(v.cast::<PyDict>().map_err(|_| PyTypeError::new_err("attributes must be a dict"))?.clone())),
    }
}

/// Attribute columns: `{name: sequence}`, one value per item (None: the
/// item doesn't get that attribute). The reserved name (`"type"` for
/// edges, `"labels"` for nodes) is kept apart: it sets the field.
struct Columns<'py> {
    len: usize,
    plain: Vec<(Bound<'py, PyString>, Vec<Bound<'py, PyAny>>)>,
    reserved: Option<Vec<Bound<'py, PyAny>>>,
}

impl<'py> Columns<'py> {
    fn parse(attrs: Option<&Bound<'py, PyDict>>, reserved: &str) -> PyResult<Option<Self>> {
        let Some(attrs) = attrs else { return Ok(None) };
        let mut columns = Columns { len: usize::MAX, plain: Vec::new(), reserved: None };
        for (key, values) in attrs.iter() {
            let key = key.cast_into::<PyString>().map_err(|_| PyTypeError::new_err("attribute names must be str"))?;
            let values: Vec<Bound<'py, PyAny>> = values.try_iter()?.collect::<PyResult<_>>()?;
            if columns.len != usize::MAX && values.len() != columns.len {
                return Err(PyValueError::new_err("attrs columns must all have the same length"));
            }
            columns.len = values.len();
            if key.to_str()? == reserved {
                columns.reserved = Some(values);
            } else {
                columns.plain.push((key, values));
            }
        }
        Ok(Some(columns))
    }

    /// Set item `i`'s column values in `attr`.
    fn apply(&self, py: Python<'py>, i: usize, attr: &mut PyAttrs) -> PyResult<()> {
        if i >= self.len {
            return Err(PyValueError::new_err("attrs columns are shorter than the items"));
        }
        for (key, values) in &self.plain {
            if !values[i].is_none() {
                attr.unique(py)?.bind(py).set_item(key, &values[i])?;
            }
        }
        Ok(())
    }

    /// Item `i`'s value of the reserved column, if set.
    fn reserved(&self, i: usize) -> Option<&Bound<'py, PyAny>> {
        self.reserved.as_ref().map(|r| &r[i]).filter(|v| !v.is_none())
    }

    fn check_len(&self, items: usize) -> PyResult<()> {
        if self.len != usize::MAX && self.len != items {
            return Err(PyValueError::new_err(format!("attrs columns have {} values for {} items", self.len, items)));
        }
        Ok(())
    }
}

fn node_id(value: &Bound<'_, PyAny>) -> PyResult<String> {
    match value.cast::<PyString>() {
        Ok(s) => Ok(s.to_str()?.to_owned()),
        Err(_) => Err(PyTypeError::new_err(format!("node ids must be str, got {}", value.get_type().name()?))),
    }
}

/// `Vertex.add_nodes(nodes, *, labels=None, attrs=None)`: each item is an
/// id or `(id, attrs)`; `attrs` columns add one value per item. Returns the
/// number of nodes added.
pub fn add_nodes(
    slf: &Bound<'_, Vertex>,
    nodes: &Bound<'_, PyAny>,
    labels: Option<Vec<String>>,
    columns: Option<&Bound<'_, PyDict>>,
) -> PyResult<usize> {
    let py = slf.py();
    // Many new dicts: without this, Python's cyclic GC rescans them (and the
    // caller's list) over and over
    let _gc = GcPause::new(py);
    let labels = labels.unwrap_or_default();
    let columns = Columns::parse(columns, "labels")?;
    let mut items: Vec<(String, NodeData, Vec<String>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for item in nodes.try_iter()? {
        let item = item?;
        let (id, attr) = if item.is_instance_of::<PyString>() {
            (node_id(&item)?, None)
        } else {
            let p = parts(&item, "node", 1, 2)?;
            (node_id(&p[0])?, attrs(p.get(1))?)
        };
        let (mut attr, from_attr) = split_reserved(attr.as_ref(), "labels")?;
        let mut own = labels.clone();
        if let Some(l) = from_attr {
            own.extend(l.extract::<Vec<String>>()?);
        }
        if let Some(c) = &columns {
            c.apply(py, items.len(), &mut attr)?;
            if let Some(l) = c.reserved(items.len()) {
                own.extend(l.extract::<Vec<String>>()?);
            }
        }
        if !seen.insert(id.clone()) || slf.try_borrow()?.graph.contains_node(&id) {
            return Err(PyValueError::new_err(format!("Node with id '{}' already exists", id)));
        }
        items.push((id, NodeData::new(attr, PyAttrs::default()), own));
    }
    if let Some(c) = &columns {
        c.check_len(items.len())?;
    }
    let count = items.len();
    let (added, callbacks) = {
        let mut v = slf.try_borrow_mut()?;
        let mut added = Vec::with_capacity(count);
        for (id, data, labels) in items {
            let ix = v.graph.add_node(id, data).map_err(graph_error)?;
            for label in &labels {
                v.graph.add_label(ix, label).map_err(graph_error)?;
            }
            added.push(ix);
        }
        (added, v.on_node_add_callbacks.clone_ref(py))
    };
    super::index::reindex(py, &slf.clone().unbind(), &added)?;
    let callbacks = callbacks.bind(py);
    if !callbacks.is_empty() {
        let vertex = slf.clone().unbind();
        for ix in added {
            let node = Node::handle(py, &vertex, ix)?;
            fire(callbacks, PyTuple::new(py, [vertex.clone_ref(py).into_any(), node.into_any()])?)?;
        }
    }
    Ok(count)
}

/// `Vertex.add_edges(edges, *, type=None, attrs=None)`: each item is
/// `(from_id, to_id)` or `(from_id, to_id, attrs)`; `attrs` columns add one
/// value per item. Returns the number of edges added.
pub fn add_edges(
    slf: &Bound<'_, Vertex>,
    edges: &Bound<'_, PyAny>,
    r#type: Option<String>,
    columns: Option<&Bound<'_, PyDict>>,
) -> PyResult<usize> {
    let py = slf.py();
    let _gc = GcPause::new(py);
    let columns = Columns::parse(columns, "type")?;
    let mut items: Vec<(NodeIx, NodeIx, EdgeData, Option<String>)> = Vec::new();
    {
        for item in edges.try_iter()? {
            let item = item?;
            let p = parts(&item, "edge", 2, 3)?;
            let (mut attr, from_attr) = split_reserved(attrs(p.get(2))?.as_ref(), "type")?;
            let i = items.len();
            let column_type = match &columns {
                Some(c) => {
                    c.apply(py, i, &mut attr)?;
                    c.reserved(i).cloned()
                }
                None => None,
            };
            let ty = match (&r#type, column_type.or(from_attr)) {
                (Some(t), _) => Some(t.clone()),
                (None, Some(t)) => Some(t.extract::<String>()?),
                (None, None) => None,
            };
            // Borrowed only for the lookups: the iterable may run Python code
            let v = slf.try_borrow()?;
            let lookup = |value: &Bound<'_, PyAny>| -> PyResult<NodeIx> {
                let s = value.cast::<PyString>().map_err(|_| PyTypeError::new_err("node ids must be str"))?;
                let id = s.to_str()?;
                v.graph.node_ix(id).ok_or_else(|| PyValueError::new_err(format!("Node with id '{}' not found", id)))
            };
            let (from, to) = (lookup(&p[0])?, lookup(&p[1])?);
            items.push((from, to, EdgeData::new(attr, PyAttrs::default()), ty));
        }
    }
    if let Some(c) = &columns {
        c.check_len(items.len())?;
    }
    let count = items.len();
    let (added, callbacks) = {
        let mut v = slf.try_borrow_mut()?;
        // Reading the iterable may have run code that removed a node
        if items.iter().any(|(from, to, _, _)| v.graph.node(*from).is_none() || v.graph.node(*to).is_none()) {
            return Err(PyValueError::new_err("a node was removed while the edges were being read"));
        }
        let mut added = Vec::with_capacity(count);
        for (from, to, data, ty) in items {
            added.push(v.graph.insert_edge(from, to, None, ty.as_deref(), data).map_err(graph_error)?);
        }
        (added, v.on_edge_add_callbacks.clone_ref(py))
    };
    let callbacks = callbacks.bind(py);
    if !callbacks.is_empty() {
        let vertex = slf.clone().unbind();
        for ix in added {
            let edge = Edge::handle(py, &vertex, ix)?;
            fire(callbacks, PyTuple::new(py, [vertex.clone_ref(py).into_any(), edge.into_any()])?)?;
        }
    }
    Ok(count)
}
