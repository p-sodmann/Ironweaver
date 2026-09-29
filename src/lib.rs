// lib.rs
//
// PyO3 bindings: the `_ironweaver` extension module. The graph itself and
// every algorithm live in the pure-Rust `ironweaver-core` crate
// (crates/ironweaver-core); this crate wraps them in Python classes and
// converts Python values, callbacks and errors.

mod convert;
mod data;
mod edge;
mod errors;
mod expr;
mod gc_pause;
mod interrupt;
mod node;
mod observed_dictionary;
mod path;
mod projection;
mod vertex;

pub use edge::Edge;
pub use node::Node;
pub use observed_dictionary::ObservedDictionary;
pub use path::Path;
pub use projection::Projection;
pub use vertex::Vertex;

use pyo3::prelude::*;
use pyo3::types::PyModule;

#[pymodule]
fn _ironweaver(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ObservedDictionary>()?;
    m.add_class::<Edge>()?;
    m.add_class::<Node>()?;
    m.add_class::<Path>()?;
    m.add_class::<Projection>()?;
    m.add_class::<Vertex>()?;
    m.add_class::<expr::PyExpr>()?;
    m.add_class::<expr::PyAttr>()?;
    m.add_function(wrap_pyfunction!(expr::attr, m)?)?;
    m.add_function(wrap_pyfunction!(expr::label, m)?)?;
    m.add_function(wrap_pyfunction!(expr::edge_type, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
