// value.rs

use half::f16;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An attribute value, as stored in graph documents.
///
/// Serialized externally tagged (`{"Float": 1.5}`, `"None"`, ...). The
/// variant order is part of the binary format (bincode writes the variant
/// index), so never reorder the variants.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Float(f64),
    /// A float stored at half precision (`save_to_binary_f16`).
    Half(f16),
    Bool(bool),
    None,
    List(Vec<Value>),
    Dict(HashMap<String, Value>),
}

impl Value {
    /// The value as a float, for `Int`, `Float` and `Half`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Half(h) => Some(h.to_f64()),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Value::None)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::None, Into::into)
    }
}
