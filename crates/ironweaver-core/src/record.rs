// record.rs
//
// How algorithms read node and edge payloads: the `Attributes` trait, and
// `Record`, the payload for pure-Rust graphs.

use std::collections::HashMap;

use crate::{GraphError, Value};

/// Result of looking an attribute up.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup<T> {
    /// Not there (or explicitly none).
    Missing,
    Found(T),
    /// There, but not of the requested type.
    Invalid,
}

/// Read access to a payload's attributes, for the algorithms that need
/// values: edge weights (`number`), node coordinates (`number` /
/// `numbers`) and edge types (`text`).
///
/// A `path` is an attribute name followed by nested keys: `["pos", "lat"]`
/// reads `attr["pos"]["lat"]`. A none value anywhere on the path counts as
/// missing.
pub trait Attributes {
    /// Raised by the lookup itself (not for missing or ill-typed values).
    type Error;

    fn number(&self, path: &[String]) -> Result<Lookup<f64>, Self::Error>;

    fn numbers(&self, path: &[String]) -> Result<Lookup<Vec<f64>>, Self::Error>;

    fn text(&self, key: &str) -> Result<Lookup<String>, Self::Error>;
}

/// Attribute map of a [`Record`].
pub type Attrs = HashMap<String, Value>;

/// Node / edge payload for pure-Rust graphs: user attributes (`attr`) and
/// free-form metadata (`meta`), mirroring the Python API.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub attr: Attrs,
    pub meta: Attrs,
}

impl Record {
    /// A record with the given attributes and no metadata.
    pub fn with_attr<K: Into<String>>(attr: impl IntoIterator<Item = (K, Value)>) -> Self {
        Record { attr: attr.into_iter().map(|(k, v)| (k.into(), v)).collect(), meta: Attrs::new() }
    }

    fn at(&self, path: &[String]) -> Option<&Value> {
        let (first, rest) = path.split_first()?;
        let mut value = self.attr.get(first)?;
        for key in rest {
            value = match value {
                Value::Dict(d) => d.get(key)?,
                _ => return None,
            };
        }
        if value.is_none() {
            None
        } else {
            Some(value)
        }
    }
}

impl Attributes for Record {
    type Error = GraphError;

    fn number(&self, path: &[String]) -> Result<Lookup<f64>, GraphError> {
        Ok(match self.at(path) {
            None => Lookup::Missing,
            Some(v) => v.as_f64().map_or(Lookup::Invalid, Lookup::Found),
        })
    }

    fn numbers(&self, path: &[String]) -> Result<Lookup<Vec<f64>>, GraphError> {
        Ok(match self.at(path) {
            None => Lookup::Missing,
            Some(Value::List(items)) => items
                .iter()
                .map(Value::as_f64)
                .collect::<Option<Vec<f64>>>()
                .map_or(Lookup::Invalid, Lookup::Found),
            Some(_) => Lookup::Invalid,
        })
    }

    fn text(&self, key: &str) -> Result<Lookup<String>, GraphError> {
        Ok(match self.attr.get(key) {
            None | Some(Value::None) => Lookup::Missing,
            Some(Value::String(s)) => Lookup::Found(s.clone()),
            Some(_) => Lookup::Invalid,
        })
    }
}
