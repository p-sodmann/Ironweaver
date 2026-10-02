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

    /// Call `f` with the value at `path` (`None` if missing or none), for
    /// filter expressions. Payloads that don't store `Value`s convert.
    fn with_value<R>(&self, path: &[String], f: impl FnOnce(Option<&Value>) -> R) -> Result<R, Self::Error>;
}

/// Attribute map of a [`Record`].
pub type Attrs = HashMap<String, Value>;

/// The value at `path` in `attrs`, with the [`Attributes`] path rules: the
/// first element names the attribute, the rest are keys into nested dicts.
/// Returns `None` if anything on the path is missing, if an intermediate
/// value is not a dict, or if the value found is none.
///
/// This is what [`Record`] uses; other payload types that store [`Attrs`]
/// can call it to get identical semantics.
///
/// ```
/// use ironweaver_core::{record::lookup, Attrs, Value};
///
/// let pos = Attrs::from([("lat".to_string(), Value::Float(52.5))]);
/// let attrs = Attrs::from([("pos".to_string(), Value::Dict(pos))]);
///
/// let path = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// assert_eq!(lookup(&attrs, &path(&["pos", "lat"])), Some(&Value::Float(52.5)));
/// assert_eq!(lookup(&attrs, &path(&["pos", "lon"])), None);
/// ```
pub fn lookup<'a>(attrs: &'a Attrs, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut value = attrs.get(first)?;
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

/// Node / edge payload for pure-Rust graphs: user attributes (`attr`) and
/// free-form metadata (`meta`), mirroring the Python API.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    // Keys sorted when serialized, so equal records give equal bytes
    #[serde(serialize_with = "crate::value::serialize_sorted")]
    pub attr: Attrs,
    #[serde(serialize_with = "crate::value::serialize_sorted")]
    pub meta: Attrs,
}

impl Record {
    /// A record with the given attributes and no metadata.
    pub fn with_attr<K: Into<String>>(attr: impl IntoIterator<Item = (K, Value)>) -> Self {
        Record { attr: attr.into_iter().map(|(k, v)| (k.into(), v)).collect(), meta: Attrs::new() }
    }
}

impl Attributes for Record {
    type Error = GraphError;

    fn with_value<R>(&self, path: &[String], f: impl FnOnce(Option<&Value>) -> R) -> Result<R, GraphError> {
        Ok(f(lookup(&self.attr, path)))
    }

    fn number(&self, path: &[String]) -> Result<Lookup<f64>, GraphError> {
        Ok(match lookup(&self.attr, path) {
            None => Lookup::Missing,
            Some(v) => v.as_f64().map_or(Lookup::Invalid, Lookup::Found),
        })
    }

    fn numbers(&self, path: &[String]) -> Result<Lookup<Vec<f64>>, GraphError> {
        Ok(match lookup(&self.attr, path) {
            None => Lookup::Missing,
            Some(Value::List(items)) => {
                items.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>().map_or(Lookup::Invalid, Lookup::Found)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lookup_follows_the_path_rules() {
        let inner = Attrs::from([("lat".to_string(), Value::Float(1.0)), ("gone".to_string(), Value::None)]);
        let attrs = Attrs::from([
            ("pos".to_string(), Value::Dict(inner)),
            ("n".to_string(), Value::Int(3)),
            ("none".to_string(), Value::None),
        ]);
        assert_eq!(lookup(&attrs, &path(&["n"])), Some(&Value::Int(3)));
        assert_eq!(lookup(&attrs, &path(&["pos", "lat"])), Some(&Value::Float(1.0)));
        assert!(matches!(lookup(&attrs, &path(&["pos"])), Some(Value::Dict(_))));
        for missing in [&[][..], &["x"], &["none"], &["pos", "gone"], &["pos", "x"], &["n", "x"], &["pos", "lat", "x"]]
        {
            assert_eq!(lookup(&attrs, &path(missing)), None, "{missing:?}");
        }
        let record = Record { attr: attrs.clone(), meta: Attrs::new() };
        assert_eq!(record.number(&path(&["pos", "lat"])).unwrap(), Lookup::Found(1.0));
        assert_eq!(record.number(&path(&["none"])).unwrap(), Lookup::Missing);
    }
}
