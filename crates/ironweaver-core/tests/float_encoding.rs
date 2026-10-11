use half::f16;
use ironweaver_core::format::tagged;
use ironweaver_core::Value;
use serde::{Serialize, Serializer};

/// A codec writing floats directly, without constructing a `Value`.
struct TaggedFloat {
    value: f64,
    half: bool,
}

impl Serialize for TaggedFloat {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        tagged::float(s, self.value, self.half)
    }
}

fn float_cases() -> [f64; 12] {
    [
        0.0,
        -0.0,
        1.5,
        -1.5,
        f64::MAX,
        -f64::MAX,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::from_bits(0x7ff8_0000_0000_1234),
        f64::from_bits(0xfff8_0000_0000_5678),
    ]
}

#[test]
fn float_json_encoding_matches_for_values_and_codecs() {
    for (value, expected) in [
        (0.0, r#"{"Float":0.0}"#),
        (-0.0, r#"{"Float":-0.0}"#),
        (1.5, r#"{"Float":1.5}"#),
        (-1.5, r#"{"Float":-1.5}"#),
        (f64::INFINITY, r#"{"Float":"Infinity"}"#),
        (f64::NEG_INFINITY, r#"{"Float":"-Infinity"}"#),
        (f64::from_bits(0x7ff8_0000_0000_1234), r#"{"Float":"NaN"}"#),
        (f64::from_bits(0xfff8_0000_0000_5678), r#"{"Float":"NaN"}"#),
    ] {
        let tagged = TaggedFloat { value, half: false };
        assert_eq!(serde_json::to_string(&Value::Float(value)).unwrap(), expected);
        assert_eq!(serde_json::to_string(&tagged).unwrap(), expected);
        assert_eq!(sonic_rs::to_string(&Value::Float(value)).unwrap(), expected);
        assert_eq!(sonic_rs::to_string(&tagged).unwrap(), expected);
    }
    // Large and subnormal finite values must remain numbers, too.
    for value in float_cases().into_iter().filter(|value| value.is_finite()) {
        let tagged = TaggedFloat { value, half: false };
        let json = serde_json::to_string(&tagged).unwrap();
        assert_eq!(json, format!(r#"{{"Float":{}}}"#, serde_json::to_string(&value).unwrap()));
        assert_eq!(json, serde_json::to_string(&Value::Float(value)).unwrap());
        assert_eq!(sonic_rs::to_string(&tagged).unwrap(), sonic_rs::to_string(&Value::Float(value)).unwrap());
    }
}

#[test]
fn binary_floats_preserve_variant_and_all_bits() {
    for value in float_cases() {
        // Format v2: variant 2 followed by the original IEEE-754 bits.
        let mut expected = vec![2];
        expected.extend(value.to_le_bytes());
        let tagged = TaggedFloat { value, half: false };
        assert_eq!(postcard::to_stdvec(&Value::Float(value)).unwrap(), expected);
        assert_eq!(postcard::to_stdvec(&tagged).unwrap(), expected);
        let Value::Float(back) = postcard::from_bytes(&expected).unwrap() else { panic!("expected Float") };
        assert_eq!(back.to_bits(), value.to_bits());
    }
}

#[test]
fn half_precision_encoding_matches_the_half_variant() {
    for value in float_cases() {
        let half = f16::from_f64(value);
        let tagged = TaggedFloat { value, half: true };
        let direct = Value::Half(half);
        assert_eq!(serde_json::to_string(&tagged).unwrap(), serde_json::to_string(&direct).unwrap());
        assert_eq!(sonic_rs::to_string(&tagged).unwrap(), sonic_rs::to_string(&direct).unwrap());
        let bytes = postcard::to_stdvec(&tagged).unwrap();
        assert_eq!(bytes[0], 3, "Half's variant index must remain 3");
        assert_eq!(bytes, postcard::to_stdvec(&direct).unwrap());
        let Value::Half(back) = postcard::from_bytes(&bytes).unwrap() else { panic!("expected Half") };
        assert_eq!(back.to_bits(), half.to_bits());
    }
}
