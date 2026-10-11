// Python result conversion shared by Projection methods and Vertex batch queries.
// Guards and the node-id string cache live with the objects they create.

use ironweaver_core::batch::DensePath;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use std::collections::HashSet;

use crate::gc_pause::GcPause;

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
pub(super) fn per_node<'py, T: IntoPyObject<'py> + Copy>(
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

/// `{id: [floats]}`, one row of `values` per node.
pub(super) fn rows_to_py(
    py: Python<'_>,
    p: &ironweaver_core::Projection,
    values: &[f32],
    width: usize,
) -> PyResult<Py<PyDict>> {
    let _gc = GcPause::new(py);
    let out = PyDict::new(py);
    if width > 0 {
        for (u, row) in values.chunks(width).enumerate() {
            let list = PyList::new(py, row.iter().map(|&x| x as f64))?;
            out.set_item(p.id(u as u32), list)?;
        }
    }
    Ok(out.unbind())
}

/// Python strings for node ids, each created once (walks repeat ids).
struct Ids<'py, 'p> {
    py: Python<'py>,
    p: &'p ironweaver_core::Projection,
    cache: Vec<Option<Bound<'py, PyString>>>,
}

impl<'py, 'p> Ids<'py, 'p> {
    fn new(py: Python<'py>, p: &'p ironweaver_core::Projection) -> Self {
        Ids { py, p, cache: vec![None; p.node_count()] }
    }

    fn get(&mut self, u: u32) -> Bound<'py, PyString> {
        self.cache[u as usize].get_or_insert_with(|| PyString::new(self.py, self.p.id(u))).clone()
    }
}

/// Groups of nodes (or walks) as lists of ids.
pub(super) fn groups_to_py(
    py: Python<'_>,
    p: &ironweaver_core::Projection,
    groups: &[Vec<u32>],
) -> PyResult<Py<PyList>> {
    let _gc = GcPause::new(py);
    let mut ids = Ids::new(py, p);
    let out = PyList::empty(py);
    for g in groups {
        out.append(PyList::new(py, g.iter().map(|&u| ids.get(u)))?)?;
    }
    Ok(out.unbind())
}
