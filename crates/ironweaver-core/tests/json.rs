// The serde form of values, expressions and patterns in JSON: read with
// `from_json_str` up to the core's own depth limits, and crafted documents
// refused without overflowing the stack.

use ironweaver_core::expr::MAX_EXPR_DEPTH;
use ironweaver_core::format::MAX_DEPTH;
use ironweaver_core::query::Pattern;
use ironweaver_core::{CmpOp, Expr, Value};

/// A value `depth` levels deep (a scalar is depth 1).
fn value(depth: usize) -> Value {
    (1..depth).fold(Value::Int(1), |v, _| Value::List(vec![v]))
}

/// An expression `depth` levels deep comparing with `v`.
fn expr(depth: usize, v: Value) -> Expr {
    let e = Expr::Compare { path: vec!["a".into()], op: CmpOp::Eq, value: v };
    (1..depth).fold(e, |e, i| if i % 2 == 0 { Expr::Not(Box::new(e)) } else { Expr::And(vec![e, Expr::Const(true)]) })
}

/// `inner` wrapped in `n` levels of `open` ... `close`.
fn wrap(open: &str, inner: &str, close: &str, n: usize) -> String {
    format!("{}{inner}{}", open.repeat(n), close.repeat(n))
}

#[test]
fn values_round_trip_up_to_the_limit() {
    let deepest = value(MAX_DEPTH);
    let json = serde_json::to_string(&deepest).unwrap();
    assert!(serde_json::from_str::<Value>(&json).is_err()); // serde_json alone stops at 128 levels
    assert_eq!(Value::from_json_str(&json).unwrap(), deepest);

    let dict = Value::Dict([("k".to_string(), value(MAX_DEPTH - 1))].into());
    assert_eq!(Value::from_json_str(&serde_json::to_string(&dict).unwrap()).unwrap(), dict);

    let too_deep = wrap(r#"{"List":["#, &json, "]}", 1);
    let err = Value::from_json_str(&too_deep).unwrap_err().to_string();
    assert!(err.contains(&format!("attribute values nested more than {MAX_DEPTH} levels deep")), "{err}");
    assert!(Value::from_json_str(r#"{"Int": 1} x"#).is_err()); // trailing characters
}

#[test]
fn expressions_and_patterns_round_trip_up_to_the_limit() {
    // The deepest expression, comparing with the deepest value: 400 JSON levels
    let deepest = expr(MAX_EXPR_DEPTH, value(MAX_DEPTH));
    assert_eq!(deepest.depth(), MAX_EXPR_DEPTH);
    let json = serde_json::to_string(&deepest).unwrap();
    assert_eq!(Expr::from_json_str(&json).unwrap(), deepest);

    let too_deep = wrap(r#"{"Not":"#, &json, "}", 1);
    let err = Expr::from_json_str(&too_deep).unwrap_err().to_string();
    assert!(err.contains(&format!("expression nested more than {MAX_EXPR_DEPTH} levels deep")), "{err}");

    let mut p = Pattern::parse("(a:Person)-[:KNOWS*1..3]->(b)").unwrap();
    p.nodes[0].filter = Some(deepest.clone());
    p.edges[0].filter = Some(deepest);
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(Pattern::from_json_str(&json).unwrap(), p);
}

#[test]
fn unknown_fields_are_refused_not_skipped() {
    // Skipping a field recurses without the depth counters
    let junk = wrap("[", "1", "]", 100_000);
    let exists = format!(r#"{{"Exists":{{"path":["a"],"junk":{junk}}}}}"#);
    for err in [
        Expr::from_json_str(&exists).unwrap_err().to_string(),
        sonic_rs::from_str::<Expr>(&exists).unwrap_err().to_string(),
    ] {
        assert!(err.contains("unknown field `junk`"), "{err}");
    }
    let cmp = r#"{"Compare":{"path":["a"],"op":"Eq","value":{"Int":1},"x":1}}"#;
    assert!(Expr::from_json_str(cmp).unwrap_err().to_string().contains("unknown field `x`"));

    let p = Pattern::parse("(a)-[*1..2]->(b)").unwrap();
    let json = serde_json::to_string(&p).unwrap();
    for (from, to) in [
        (r#"{"nodes":"#, r#"{"junk":0,"nodes":"#),
        (r#"{"name":"a","#, r#"{"name":"a","junk":0,"#),
        (r#""directed":"#, r#""junk":0,"directed":"#),
        (r#""min":"#, r#""junk":0,"min":"#),
    ] {
        assert!(json.contains(from), "{json}");
        let err = Pattern::from_json_str(&json.replacen(from, to, 1)).unwrap_err().to_string();
        assert!(err.contains("unknown field `junk`"), "{from}: {err}");
    }
}

#[test]
fn crafted_nesting_is_refused() {
    let list = wrap(r#"{"List":["#, r#"{"Int":1}"#, "]}", 100_000);
    let err = Value::from_json_str(&list).unwrap_err().to_string();
    assert!(err.contains("attribute values nested more than"), "{err}");
    let not = wrap(r#"{"Not":"#, r#"{"Const":true}"#, "}", 100_000);
    let err = Expr::from_json_str(&not).unwrap_err().to_string();
    assert!(err.contains("expression nested more than"), "{err}");
    let and = wrap(r#"{"And":["#, r#"{"Const":true}"#, "]}", 100_000);
    assert!(Expr::from_json_str(&and).is_err());
    // Scalars given a deep array instead
    let junk = wrap("[", "1", "]", 100_000);
    for tag in ["Int", "Float", "Half", "String", "Bytes", "Date", "DateTime"] {
        assert!(Value::from_json_str(&format!(r#"{{"{tag}":{junk}}}"#)).is_err(), "{tag}");
    }
}
