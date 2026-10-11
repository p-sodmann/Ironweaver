//! Invalid binary date-times must return errors, including through graph loaders.

use chrono::{Datelike, NaiveDateTime, Timelike};
use ironweaver_core::format;
use ironweaver_core::temporal::Parts;
use ironweaver_core::{Attrs, DateTime, Graph, Record, Value};

fn decode(micros: i64, offset: Option<i32>) -> Result<DateTime, postcard::Error> {
    let bytes = postcard::to_stdvec(&(micros, offset)).unwrap();
    postcard::from_bytes(&bytes)
}

#[test]
fn datetime_binary_rejects_invalid_offsets() {
    for offset in [i32::MIN, i32::MAX, -86_401, -86_400, 86_400, 86_401] {
        assert!(decode(0, Some(offset)).is_err(), "offset {offset}");
    }
}

#[test]
fn datetime_binary_rejects_invalid_local_times() {
    let min = NaiveDateTime::MIN.and_utc().timestamp_micros();
    let max = NaiveDateTime::MAX.and_utc().timestamp_micros();
    for (micros, offset) in [
        (i64::MAX, Some(3600)),
        (i64::MIN, Some(-3600)),
        (i64::MAX, None),
        (i64::MIN, Some(0)),
        (min - 1, None),
        (max + 1, Some(0)),
        (min, Some(-1)),
        (max, Some(1)),
    ] {
        assert!(decode(micros, offset).is_err(), "micros {micros}, offset {offset:?}");
    }
}

#[test]
fn datetime_binary_preserves_valid_local_limits_and_wire_format() {
    for local in [NaiveDateTime::MIN, NaiveDateTime::MAX] {
        let parts = Parts {
            year: local.year(),
            month: local.month(),
            day: local.day(),
            hour: local.hour(),
            minute: local.minute(),
            second: local.second(),
            microsecond: local.nanosecond() / 1000,
        };
        for offset in [None, Some(-86_399), Some(-1), Some(0), Some(1), Some(86_399)] {
            let original = DateTime::from_parts(parts, offset).unwrap();
            let bytes = postcard::to_stdvec(&original).unwrap();
            assert_eq!(bytes, postcard::to_stdvec(&(original.micros, offset)).unwrap());
            let restored: DateTime = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(restored, original);
            assert_eq!(restored.offset, offset);
            assert_eq!(restored.parts(), parts);
            assert_eq!(restored.to_string(), original.to_string());
            let json = serde_json::to_string(&restored).unwrap();
            assert_eq!(serde_json::from_str::<DateTime>(&json).unwrap(), original);
        }
    }
}

#[test]
fn datetime_direct_out_of_range_times_use_the_existing_fallback() {
    let fallback = DateTime { micros: NaiveDateTime::MIN.and_utc().timestamp_micros(), offset: None }.parts();
    for micros in [i64::MAX, i64::MIN] {
        for offset in [Some(3600), Some(-3600), Some(i32::MIN), Some(i32::MAX), Some(0), None] {
            let datetime = DateTime { micros, offset };
            assert_eq!(datetime.parts(), fallback, "micros {micros}, offset {offset:?}");
            assert!(!datetime.to_string().is_empty());
            assert!(serde_json::to_string(&datetime).is_ok());
        }
    }
}

#[test]
fn datetime_direct_minimum_offset_formats_without_signed_negation() {
    let datetime = DateTime { micros: 0, offset: Some(i32::MIN) };
    assert!(datetime.to_string().ends_with("-596523:14:08"));
    assert!(serde_json::to_string(&datetime).is_ok());
}

#[test]
fn datetime_graph_loaders_reject_invalid_binary_values() {
    for (micros, offset, message) in [
        (0, Some(i32::MIN), "UTC offset out of range"),
        (0, Some(86_400), "UTC offset out of range"),
        (i64::MAX, Some(3600), "date-time out of range"),
        (i64::MIN, Some(-3600), "date-time out of range"),
        (i64::MAX, None, "date-time out of range"),
    ] {
        // Public fields allow raw values; binary serialization keeps the
        // tuple encoding, letting us construct checksummed malformed input.
        let mut graph = Graph::<Record, Record>::new();
        graph.add_node("n", Record::with_attr([("t", Value::DateTime(DateTime { micros, offset }))])).unwrap();
        let file = format::to_binary(&graph, &Attrs::new(), false).unwrap();
        let err = format::from_binary(&file).expect_err("slice loader accepted invalid date-time");
        assert!(err.to_string().contains(message), "{err}");
        let err = format::from_binary_reader(file.as_slice()).expect_err("stream loader accepted invalid date-time");
        assert!(err.to_string().contains(message), "{err}");
    }
}
