// gc_pause.rs

use pyo3::prelude::*;
use pyo3::types::PyModule;

/// Pauses Python's cyclic garbage collector for the guard's lifetime.
///
/// Graph objects take part in cyclic GC (see `Node::__traverse__`). When an
/// operation allocates many objects at once (loading, building a subgraph,
/// converting to networkx), allocation-triggered collections would walk the
/// whole object graph again and again although nothing created can be garbage
/// yet. This is the same trick CPython's own bulk loaders rely on. The
/// collector is re-enabled on drop, including on error, and left alone if it
/// was already disabled.
pub struct GcPause<'py> {
    gc: Option<Bound<'py, PyModule>>,
}

impl<'py> GcPause<'py> {
    pub fn new(py: Python<'py>) -> Self {
        if let Ok(gc) = py.import("gc") {
            let enabled = gc
                .call_method0("isenabled")
                .and_then(|r| r.is_truthy())
                .unwrap_or(false);
            if enabled && gc.call_method0("disable").is_ok() {
                return GcPause { gc: Some(gc) };
            }
        }
        GcPause { gc: None }
    }
}

impl Drop for GcPause<'_> {
    fn drop(&mut self) {
        if let Some(gc) = &self.gc {
            let _ = gc.call_method0("enable");
        }
    }
}
