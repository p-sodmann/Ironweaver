// value.rs

use half::f16;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An attribute value, as stored in graph documents.
///
/// Serialized externally tagged (`{"Float": 1.5}`, `"None"`, ...). The
/// variant order is part of the binary format (bincode writes the variant
/// index), so never reorder the variants. Serde (de)serialization fails for
/// values nested more than [`MAX_DEPTH`](crate::format::MAX_DEPTH) levels
/// instead of overflowing the stack.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Float(f64),
    /// A float stored at half precision (`save_to_binary_f16`).
    Half(f16),
    Bool(bool),
    None,
    List(#[serde(with = "nested")] Vec<Value>),
    Dict(#[serde(with = "nested")] HashMap<String, Value>),
}

/// Serde helpers for the contents of a list / dict value: count the nesting
/// depth (per thread) and refuse to go deeper than `MAX_DEPTH`.
mod nested {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::cell::Cell;

    use crate::format::MAX_DEPTH;

    thread_local! {
        // Containers entered on this thread
        static DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    fn enter<T, E>(err: impl FnOnce() -> E, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        struct Level;
        impl Drop for Level {
            fn drop(&mut self) {
                DEPTH.with(|c| c.set(c.get() - 1));
            }
        }
        let depth = DEPTH.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        let _level = Level;
        // A value inside `depth` containers is `depth + 1` levels deep
        if depth >= MAX_DEPTH {
            return Err(err());
        }
        f()
    }

    fn message() -> String {
        format!("attribute values nested more than {MAX_DEPTH} levels deep")
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        enter(|| crate::format::ser_error::<S::Error>(message()), || v.serialize(s))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
        enter(|| crate::format::de_error::<D::Error>(message()), || T::deserialize(d))
    }
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

    /// Equality across numeric types (`Int(1)` equals `Float(1.0)`);
    /// otherwise the same variant with equal contents.
    pub fn loose_eq(&self, other: &Value) -> bool {
        match (self.as_f64(), other.as_f64()) {
            (Some(a), Some(b)) => a == b,
            (Some(_), None) | (None, Some(_)) => false,
            (None, None) => match (self, other) {
                (Value::List(a), Value::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.loose_eq(y)),
                (Value::Dict(a), Value::Dict(b)) => {
                    a.len() == b.len() && a.iter().all(|(k, x)| b.get(k).is_some_and(|y| x.loose_eq(y)))
                }
                _ => self == other,
            },
        }
    }

    /// Order of two numbers, two strings or two bools; `None` for other
    /// pairs (and NaN).
    pub fn loose_cmp(&self, other: &Value) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            _ => self.as_f64()?.partial_cmp(&other.as_f64()?),
        }
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
