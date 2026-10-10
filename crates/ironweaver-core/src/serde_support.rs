//! Shared serde safeguards and float encoding for values and graph documents.

use std::cell::Cell;

/// Serde helpers for [`Value::Float`](crate::Value::Float): JSON has no literals for NaN and the
/// infinities, so human-readable formats write them as the strings `"NaN"`,
/// `"Infinity"` and `"-Infinity"` (as the file format does, see
/// `format::tagged::float`) and accept those strings as well as numbers.
/// Binary formats store the float itself.
pub(crate) mod float {
    use serde::de::{self, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt;

    /// A float payload for serializers that write their own tagged variant.
    pub(crate) struct Float(pub f64);

    impl Serialize for Float {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            serialize(&self.0, s)
        }
    }

    pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if v.is_finite() || !s.is_human_readable() {
            s.serialize_f64(*v)
        } else if v.is_nan() {
            s.serialize_str("NaN")
        } else if *v > 0.0 {
            s.serialize_str("Infinity")
        } else {
            s.serialize_str("-Infinity")
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        if !d.is_human_readable() {
            return f64::deserialize(d);
        }
        struct Float;
        impl Visitor<'_> for Float {
            type Value = f64;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, \"NaN\", \"Infinity\" or \"-Infinity\"")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<f64, E> {
                Ok(v)
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<f64, E> {
                Ok(v as f64)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<f64, E> {
                Ok(v as f64)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<f64, E> {
                match v {
                    "NaN" => Ok(f64::NAN),
                    "Infinity" => Ok(f64::INFINITY),
                    "-Infinity" => Ok(f64::NEG_INFINITY),
                    _ => Err(E::invalid_value(de::Unexpected::Str(v), &self)),
                }
            }
        }
        d.deserialize_any(Float)
    }
}

/// Run `f` one level deeper on the per-thread counter `depth`: the
/// value inside `depth` containers is `depth + 1` levels deep, so this
/// fails with `err()` once `max` containers are open.
/// Each recursive type supplies its own counter and limit. The counter
/// is restored on success, errors and unwinding.
pub(crate) fn enter_level<T, E>(
    depth: &'static std::thread::LocalKey<Cell<usize>>,
    max: usize,
    err: impl FnOnce() -> E,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    struct Level(&'static std::thread::LocalKey<Cell<usize>>);
    impl Drop for Level {
        fn drop(&mut self) {
            self.0.with(|c| c.set(c.get() - 1));
        }
    }
    let open = depth.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    let _level = Level(depth);
    if open >= max {
        return Err(err());
    }
    f()
}

/// Read the serde form of `T` from JSON with no recursion limit of the
/// JSON parser's own: for types whose every level of nesting is counted by
/// their serde impls (with a limit) and that refuse unknown fields, so
/// nothing is skipped ([`Value`](crate::Value), [`Expr`](crate::Expr),
/// [`Pattern`](crate::query::Pattern)). serde_json alone stops at 128 JSON
/// levels, 64 levels of `{"List": [...]}`.
pub(crate) fn from_json_str<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, crate::GraphError> {
    let mut de = serde_json::Deserializer::from_str(json);
    de.disable_recursion_limit();
    let value = T::deserialize(&mut de).and_then(|v| de.end().map(|()| v));
    value.map_err(|e| crate::GraphError::Format(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::enter_level;
    use std::cell::Cell;

    thread_local! {
        static DEPTH: Cell<usize> = const { Cell::new(0) };
        static OTHER_DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    #[test]
    fn restores_depth_after_success_and_errors() {
        assert_eq!(enter_level(&DEPTH, 2, || "limit", || Ok(42)), Ok(42));
        assert_eq!(DEPTH.get(), 0);
        assert_eq!(enter_level(&DEPTH, 2, || "limit", || Err::<(), _>("invalid input")), Err("invalid input"));
        assert_eq!(DEPTH.get(), 0);
        let result = enter_level(
            &DEPTH,
            2,
            || "limit",
            || {
                assert_eq!(DEPTH.get(), 1);
                let result = enter_level(
                    &DEPTH,
                    2,
                    || "limit",
                    || -> Result<(), _> {
                        panic!("must reject before entering the closure");
                    },
                );
                assert_eq!(DEPTH.get(), 1);
                result
            },
        );
        assert_eq!(result, Err("limit"));
        assert_eq!(DEPTH.get(), 0);
    }

    #[test]
    fn restores_depth_after_panics() {
        assert!(std::panic::catch_unwind(|| {
            let _: Result<(), ()> = enter_level(&DEPTH, 2, || (), || panic!("serializer panicked"));
        })
        .is_err());
        assert_eq!(DEPTH.get(), 0);
        assert!(std::panic::catch_unwind(|| {
            let _: Result<(), ()> = enter_level(&DEPTH, 1, || panic!("error construction panicked"), || Ok(()));
        })
        .is_err());
        assert_eq!(DEPTH.get(), 0);
        assert_eq!(enter_level(&DEPTH, 2, || (), || Ok(())), Ok(()));
    }

    #[test]
    fn counters_are_independent_between_types_and_threads() {
        enter_level(
            &DEPTH,
            2,
            || (),
            || {
                enter_level(
                    &OTHER_DEPTH,
                    2,
                    || (),
                    || {
                        assert_eq!(DEPTH.get(), 1);
                        assert_eq!(OTHER_DEPTH.get(), 1);
                        std::thread::spawn(|| {
                            assert_eq!(DEPTH.get(), 0);
                            assert_eq!(OTHER_DEPTH.get(), 0);
                            assert_eq!(enter_level(&DEPTH, 2, || (), || Ok(())), Ok(()));
                        })
                        .join()
                        .unwrap();
                        assert_eq!(DEPTH.get(), 1);
                        Ok(())
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(DEPTH.get(), 0);
        assert_eq!(OTHER_DEPTH.get(), 0);
    }
}
