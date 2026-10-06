// heap_size.rs
//
// `HeapSize`: the heap memory a payload owns, so `Graph::memory_usage` can
// count payloads too (see `Graph::count_payloads`).

use std::collections::HashMap;
use std::mem::size_of;

use crate::graph::hash_table_bytes;
use crate::{Record, Value};

/// Heap bytes a value owns beyond its inline size (`size_of`): strings,
/// lists, maps and what they hold.
///
/// Computed from lengths, not capacities, so a graph and a copy of it
/// replayed from ops or loaded from a file report the same figure. Must not
/// change unless the value does.
///
/// ```
/// use ironweaver_core::{HeapSize, Record, Value};
///
/// assert_eq!(Value::from("abc").heap_bytes(), 3);
/// assert!(Record::with_attr([("name", Value::from("abc"))]).heap_bytes() > 3);
/// ```
pub trait HeapSize {
    fn heap_bytes(&self) -> usize;
}

impl HeapSize for () {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl HeapSize for String {
    fn heap_bytes(&self) -> usize {
        self.len()
    }
}

impl HeapSize for Value {
    fn heap_bytes(&self) -> usize {
        match self {
            Value::String(s) => s.len(),
            Value::Bytes(b) => b.len(),
            Value::List(items) => items.len() * size_of::<Value>() + items.iter().map(Value::heap_bytes).sum::<usize>(),
            Value::Dict(d) => d.heap_bytes(),
            Value::Int(_)
            | Value::Float(_)
            | Value::Half(_)
            | Value::Bool(_)
            | Value::None
            | Value::Date(_)
            | Value::DateTime(_) => 0,
        }
    }
}

/// The table (as if sized for exactly its entries) plus the keys and values.
impl<V: HeapSize, S> HeapSize for HashMap<String, V, S> {
    fn heap_bytes(&self) -> usize {
        hash_table_bytes(self.len(), size_of::<(String, V)>())
            + self.iter().map(|(k, v)| k.len() + v.heap_bytes()).sum::<usize>()
    }
}

impl HeapSize for Record {
    fn heap_bytes(&self) -> usize {
        self.attr.heap_bytes() + self.meta.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Attrs;

    #[test]
    fn counts_what_values_own() {
        assert_eq!(Value::Int(3).heap_bytes(), 0);
        assert_eq!(Value::from("abcd").heap_bytes(), 4);
        assert_eq!(Value::Bytes(vec![0; 10]).heap_bytes(), 10);
        let list = Value::List(vec![Value::from("ab"), Value::Int(1)]);
        assert_eq!(list.heap_bytes(), 2 * size_of::<Value>() + 2);
        assert_eq!(Attrs::new().heap_bytes(), 0);
        let one = Attrs::from([("key".to_string(), Value::from("value"))]);
        assert_eq!(one.heap_bytes(), hash_table_bytes(1, size_of::<(String, Value)>()) + 3 + 5);
        let nested = Value::Dict(one.clone());
        assert_eq!(nested.heap_bytes(), one.heap_bytes());
        let rec = Record { attr: one.clone(), meta: one.clone() };
        assert_eq!(rec.heap_bytes(), 2 * one.heap_bytes());
    }

    #[test]
    fn ignores_spare_capacity() {
        let mut s = String::with_capacity(100);
        s.push_str("ab");
        assert_eq!(Value::String(s).heap_bytes(), Value::from("ab").heap_bytes());
    }
}
