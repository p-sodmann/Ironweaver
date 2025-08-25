// lib.rs
mod node;
mod edge;
mod observed_dictionary;
mod path;
mod vertex;
pub mod serialization;
pub use vertex::Vertex;
pub use path::Path;
pub use node::Node;
pub use edge::Edge;
pub use observed_dictionary::ObservedDictionary;

use pyo3::prelude::*;
use pyo3::types::PyModule;

#[pymodule]
fn _ironweaver(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ObservedDictionary>()?;
    m.add_class::<Edge>()?;
    m.add_class::<Node>()?;
    m.add_class::<Path>()?;
    m.add_class::<Vertex>()?;
    Ok(())
}

