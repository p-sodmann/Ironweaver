// value.rs

use half::f16;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

use crate::serde_support::float;
use crate::temporal::{self, Date, DateTime};

/// An attribute value, as stored in graph documents.
///
/// Serialized externally tagged (`{"Float": 1.5}`, `"None"`, ...), like the
/// file format: in human-readable formats (JSON), a NaN or infinite `Float`
/// is written as the string `"NaN"`, `"Infinity"` or `"-Infinity"` and read
/// back from it. The variant order is part of the binary format (postcard writes the variant
/// index), so never reorder the variants. Serde (de)serialization fails for
/// values nested more than [`MAX_DEPTH`](crate::format::MAX_DEPTH) levels
/// instead of overflowing the stack, counted like the file format: a value
/// is depth 1 and a container's items are one deeper (an empty container
/// counts like a scalar).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Float(#[serde(with = "float")] f64),
    /// A float stored at half precision (`save_to_binary_f16`).
    Half(f16),
    Bool(bool),
    None,
    List(#[serde(serialize_with = "nested::serialize_list", deserialize_with = "nested::deserialize_list")] Vec<Value>),
    Dict(
        #[serde(serialize_with = "nested::serialize_dict", deserialize_with = "nested::deserialize_dict")]
        HashMap<String, Value>,
    ),
    /// A byte string (base64 in JSON).
    Bytes(#[serde(with = "temporal::bytes")] Vec<u8>),
    /// A calendar date ("YYYY-MM-DD" in JSON).
    Date(Date),
    /// A date-time, with or without UTC offset (ISO 8601 in JSON).
    DateTime(DateTime),
}

/// Serde helpers for the contents of a list / dict value: count the nesting
/// depth (per thread) and refuse values deeper than `MAX_DEPTH`, counted
/// like the file format: the outermost value is depth 1 and a container's
/// items are one deeper, so an empty container counts like a scalar.
mod nested {
    use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::fmt;

    use super::Value;
    use crate::format::MAX_DEPTH;
    use crate::serde_support::enter_level;

    thread_local! {
        // Containers entered on this thread
        static DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    fn message() -> String {
        format!("attribute values nested more than {MAX_DEPTH} levels deep")
    }

    /// A container's item: entered one level deeper than the container, so
    /// only containers that have items count towards the limit.
    struct Item<'a>(&'a Value);

    impl Serialize for Item<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            enter_level(&DEPTH, MAX_DEPTH, || crate::format::ser_error::<S::Error>(message()), || self.0.serialize(s))
        }
    }

    struct ItemSeed;

    impl<'de> DeserializeSeed<'de> for ItemSeed {
        type Value = Value;

        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
            enter_level(&DEPTH, MAX_DEPTH, || crate::format::de_error::<D::Error>(message()), || Value::deserialize(d))
        }
    }

    /// Preallocate at most this many items from an untrusted size hint.
    fn cautious(hint: Option<usize>) -> usize {
        hint.unwrap_or(0).min(4096)
    }

    pub fn serialize_list<S: Serializer>(v: &[Value], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(Item))
    }

    /// A dict's entries, sorted by key.
    pub fn serialize_dict<S: Serializer>(v: &HashMap<String, Value>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(super::sorted_entries(v).into_iter().map(|(k, v)| (k, Item(v))))
    }

    pub fn deserialize_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Value>, D::Error> {
        struct List;
        impl<'de> Visitor<'de> for List {
            type Value = Vec<Value>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a list of values")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Value>, A::Error> {
                let mut items = Vec::with_capacity(cautious(seq.size_hint()));
                while let Some(item) = seq.next_element_seed(ItemSeed)? {
                    items.push(item);
                }
                Ok(items)
            }
        }
        d.deserialize_seq(List)
    }

    pub fn deserialize_dict<'de, D: Deserializer<'de>>(d: D) -> Result<HashMap<String, Value>, D::Error> {
        struct Dict;
        impl<'de> Visitor<'de> for Dict {
            type Value = HashMap<String, Value>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of values")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<HashMap<String, Value>, A::Error> {
                let mut items = HashMap::with_capacity(cautious(map.size_hint()));
                while let Some(key) = map.next_key::<String>()? {
                    let value = map.next_value_seed(ItemSeed)?;
                    items.insert(key, value);
                }
                Ok(items)
            }
        }
        d.deserialize_map(Dict)
    }
}

/// The entries of an attribute map sorted by key: what savers write, so
/// that equal maps give equal bytes.
pub fn sorted_entries(map: &HashMap<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
    entries
}

/// Serialize an attribute map with its keys sorted (serde
/// `serialize_with` helper; `Record` and `Value::Dict` use it).
pub fn serialize_sorted<S: serde::Serializer>(map: &HashMap<String, Value>, s: S) -> Result<S::Ok, S::Error> {
    s.collect_map(sorted_entries(map))
}

impl Value {
    /// Read a value from its serde form in JSON (e.g. `{"Int": 30}`), up to
    /// [`MAX_DEPTH`](crate::format::MAX_DEPTH) levels deep. Prefer it to
    /// `serde_json::from_str`, which stops at 64 levels of lists / dicts.
    pub fn from_json_str(json: &str) -> Result<Value, crate::GraphError> {
        crate::serde_support::from_json_str(json)
    }

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

    /// Order of two numbers, strings, bools, byte strings, dates or
    /// date-times (instants with instants, wall-clock times with wall-clock
    /// times); `None` for other pairs (and NaN).
    pub fn loose_cmp(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            (Value::Bytes(a), Value::Bytes(b)) => Some(a.cmp(b)),
            (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
            (Value::DateTime(a), Value::DateTime(b)) => a.partial_order(b),
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            _ => self.as_f64()?.partial_cmp(&other.as_f64()?),
        }
    }
}

/// A scalar value as an index key: hashable and totally ordered, with
/// `Key::of(a) == Key::of(b)` exactly when `a.loose_eq(b)`, and the order
/// of `loose_cmp` within a kind. Numbers are normalized (an integral float
/// is the same key as the integer); keys of different kinds order by kind.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Bool(bool),
    Int(i64),
    /// A float that is not an integer in `i64` range (never NaN).
    Float(u64),
    String(String),
    Bytes(Vec<u8>),
    Date(Date),
    /// A wall-clock date-time (microseconds).
    LocalDateTime(i64),
    /// An instant (UTC microseconds).
    DateTime(i64),
}

impl Key {
    /// The key of a value; `None` for none, lists, dicts and NaN.
    pub fn of(v: &Value) -> Option<Key> {
        Some(match v {
            Value::Bool(b) => Key::Bool(*b),
            Value::Int(i) => Key::Int(*i),
            Value::Float(_) | Value::Half(_) => Key::number(v.as_f64()?)?,
            Value::String(s) => Key::String(s.clone()),
            Value::Bytes(b) => Key::Bytes(b.clone()),
            Value::Date(d) => Key::Date(*d),
            Value::DateTime(t) if t.offset.is_some() => Key::DateTime(t.micros),
            Value::DateTime(t) => Key::LocalDateTime(t.micros),
            Value::None | Value::List(_) | Value::Dict(_) => return None,
        })
    }

    fn number(f: f64) -> Option<Key> {
        if f.is_nan() {
            None
        } else if f.fract() == 0.0 && (-TWO_63..TWO_63).contains(&f) {
            Some(Key::Int(f as i64))
        } else {
            Some(Key::Float(f.to_bits()))
        }
    }

    /// Keys of the same kind compare (for range bounds): numbers with
    /// numbers, strings with strings, and so on.
    pub fn same_kind(&self, other: &Key) -> bool {
        self.rank() == other.rank()
    }

    fn rank(&self) -> u8 {
        match self {
            Key::Bool(_) => 0,
            Key::Int(_) | Key::Float(_) => 1,
            Key::String(_) => 2,
            Key::Bytes(_) => 3,
            Key::Date(_) => 4,
            Key::LocalDateTime(_) => 5,
            Key::DateTime(_) => 6,
        }
    }
}

/// 2^63: floats in `-TWO_63..TWO_63` convert to `i64` exactly (if integral).
const TWO_63: f64 = 9_223_372_036_854_775_808.0;

/// Order of an integer and a float (neither NaN), exactly.
fn cmp_int_float(i: i64, f: f64) -> Ordering {
    if f >= TWO_63 {
        Ordering::Less
    } else if f < -TWO_63 {
        Ordering::Greater
    } else {
        let floor = f.floor();
        // `floor` is an integer in range, so the cast is exact
        match i.cmp(&(floor as i64)) {
            Ordering::Equal if f > floor => Ordering::Less,
            o => o,
        }
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Key) -> Ordering {
        match (self, other) {
            (Key::Bool(a), Key::Bool(b)) => a.cmp(b),
            (Key::Int(a), Key::Int(b)) => a.cmp(b),
            (Key::Float(a), Key::Float(b)) => f64::from_bits(*a).total_cmp(&f64::from_bits(*b)),
            (Key::Int(a), Key::Float(b)) => cmp_int_float(*a, f64::from_bits(*b)),
            (Key::Float(a), Key::Int(b)) => cmp_int_float(*b, f64::from_bits(*a)).reverse(),
            (Key::String(a), Key::String(b)) => a.cmp(b),
            (Key::Bytes(a), Key::Bytes(b)) => a.cmp(b),
            (Key::Date(a), Key::Date(b)) => a.cmp(b),
            (Key::LocalDateTime(a), Key::LocalDateTime(b)) | (Key::DateTime(a), Key::DateTime(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Key) -> Option<Ordering> {
        Some(self.cmp(other))
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

impl From<Date> for Value {
    fn from(v: Date) -> Self {
        Value::Date(v)
    }
}

impl From<DateTime> for Value {
    fn from(v: DateTime) -> Self {
        Value::DateTime(v)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_loose_equality_and_order() {
        let values = [
            Value::Int(-3),
            Value::Float(-2.5),
            Value::Int(0),
            Value::Float(-0.0),
            Value::Float(1.0),
            Value::Int(1),
            Value::Half(f16::from_f64(1.5)),
            Value::Float(1.5),
            Value::Int(i64::MAX),
            Value::Float(9.3e18),
            Value::Float(f64::INFINITY),
            Value::Float(f64::NEG_INFINITY),
            Value::Int(i64::MIN),
            Value::from("a"),
            Value::from("b"),
            Value::Bool(true),
            Value::Bytes(vec![1, 2]),
            Value::Date(Date(3)),
            Value::Date(Date(-3)),
        ];
        for a in &values {
            for b in &values {
                let (ka, kb) = (Key::of(a).unwrap(), Key::of(b).unwrap());
                assert_eq!(ka == kb, a.loose_eq(b), "{a:?} {b:?}");
                if let Some(o) = a.loose_cmp(b) {
                    assert_eq!(ka.cmp(&kb), o, "{a:?} {b:?}");
                }
            }
        }
        assert_eq!(Key::of(&Value::Float(f64::NAN)), None);
        assert_eq!(Key::of(&Value::None), None);
        assert_eq!(Key::of(&Value::List(vec![])), None);
        assert!(Key::of(&Value::Int(1)).unwrap().same_kind(&Key::of(&Value::Float(0.5)).unwrap()));
    }

    #[test]
    fn serde_roundtrip() {
        let t: DateTime = "2024-05-01T12:30:00.5+02:00".parse().unwrap();
        let v = Value::List(vec![Value::Bytes(b"hi".to_vec()), Value::Date(Date(19000)), Value::DateTime(t)]);
        let json = sonic_rs::to_string(&v).unwrap();
        assert_eq!(
            json,
            r#"{"List":[{"Bytes":"aGk="},{"Date":"2022-01-08"},{"DateTime":"2024-05-01T12:30:00.500000+02:00"}]}"#
        );
        assert_eq!(sonic_rs::from_str::<Value>(&json).unwrap(), v);
        let bin = postcard::to_allocvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<Value>(&bin).unwrap(), v);
    }

    #[test]
    fn serde_non_finite_floats() {
        fn float(v: Value) -> f64 {
            match v {
                Value::Float(f) => f,
                other => panic!("not a Float: {other:?}"),
            }
        }
        for (f, text) in [(f64::NAN, "NaN"), (f64::INFINITY, "Infinity"), (f64::NEG_INFINITY, "-Infinity")] {
            let json = sonic_rs::to_string(&Value::Float(f)).unwrap();
            assert_eq!(json, format!(r#"{{"Float":"{text}"}}"#));
            let back = float(sonic_rs::from_str(&json).unwrap());
            assert_eq!(back.to_bits(), f.to_bits());
            let bin = postcard::to_allocvec(&Value::Float(f)).unwrap();
            assert_eq!(float(postcard::from_bytes(&bin).unwrap()).to_bits(), f.to_bits());
        }
        assert_eq!(sonic_rs::to_string(&Value::Float(1.5)).unwrap(), r#"{"Float":1.5}"#);
        assert_eq!(sonic_rs::from_str::<Value>(r#"{"Float":2}"#).unwrap(), Value::Float(2.0));
        assert_eq!(sonic_rs::from_str::<Value>(r#"{"Float":-3}"#).unwrap(), Value::Float(-3.0));
        assert!(sonic_rs::from_str::<Value>(r#"{"Float":"nan"}"#).is_err());
        assert!(sonic_rs::from_str::<Value>(r#"{"Float":null}"#).is_err());
    }
}
