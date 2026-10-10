use ironweaver_core::expr::MAX_EXPR_DEPTH;
use ironweaver_core::format::MAX_DEPTH;
use ironweaver_core::query::{NodePattern, Pattern};
use ironweaver_core::{CmpOp, Expr, Value};

#[test]
fn json_readers_require_one_complete_document() {
    let value = Value::Int(42);
    let expr = Expr::Const(true);
    let pattern = Pattern::default();
    let value_json = serde_json::to_string(&value).unwrap();
    let expr_json = serde_json::to_string(&expr).unwrap();
    let pattern_json = serde_json::to_string(&pattern).unwrap();
    assert_eq!(Value::from_json_str(&format!(" {value_json}\n")).unwrap(), value);
    assert_eq!(Expr::from_json_str(&format!(" {expr_json}\n")).unwrap(), expr);
    assert_eq!(Pattern::from_json_str(&format!(" {pattern_json}\n")).unwrap(), pattern);
    for suffix in [" null", " {}", " garbage"] {
        assert!(Value::from_json_str(&format!("{value_json}{suffix}")).is_err());
        assert!(Expr::from_json_str(&format!("{expr_json}{suffix}")).is_err());
        assert!(Pattern::from_json_str(&format!("{pattern_json}{suffix}")).is_err());
    }
}

#[test]
fn expression_and_value_depth_limits_are_independent_inside_patterns() {
    let mut value = Value::List(vec![]);
    for _ in 1..MAX_DEPTH {
        value = Value::List(vec![value]);
    }
    let mut expr = Expr::Compare { path: vec!["payload".into()], op: CmpOp::Eq, value };
    for _ in 1..MAX_EXPR_DEPTH {
        expr = Expr::Not(Box::new(expr));
    }
    let pattern =
        Pattern { nodes: vec![NodePattern { filter: Some(expr.clone()), ..Default::default() }], edges: vec![] };
    // These valid documents exceed serde_json's default parser recursion limit.
    let json = serde_json::to_string(&pattern).unwrap();
    assert!(serde_json::from_str::<Pattern>(&json).is_err());
    assert_eq!(Pattern::from_json_str(&json).unwrap(), pattern);
    let expr_json = serde_json::to_string(&expr).unwrap();
    let too_deep = format!(r#"{{"Not":{expr_json}}}"#);
    assert!(Expr::from_json_str(&too_deep).unwrap_err().to_string().contains("expression nested more than"));
    assert_eq!(Expr::from_json_str(&expr_json).unwrap(), expr);
    assert_eq!(Pattern::from_json_str(&json).unwrap(), pattern);
}
