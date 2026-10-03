// expr.rs
//
// Filter expressions evaluated in Rust: comparisons on attribute paths,
// membership, existence, node labels, edge types, and boolean combinations.
// They read payloads only through `Attributes::with_value`, so the Python
// bindings can filter without calling Python functions, and a query engine
// can use them as its predicate representation.
//
// The attribute names "labels" (nodes) and "type" (edges) are the graph
// fields: `labels` is the list of the node's label names, `type` the edge's
// type (missing if it has none), whatever the payload holds under that name.
//
// Missing attributes (or none values) make every comparison false, like
// SQL NULL: `attr("x") != 1` is false when `x` is missing; use `Not` /
// `Exists` to say otherwise. Numbers compare across `Int` / `Float`.
//
// Expressions serialize with serde (externally tagged, like `Value` and
// `Op`), so they can travel over the network or be stored. Nesting of
// `And` / `Or` / `Not` is limited to `MAX_EXPR_DEPTH` levels both ways, and
// unknown fields are refused rather than skipped (skipping recurses without
// the depth counters), so a crafted document can't overflow the stack.
// `Expr::from_json_str` reads JSON relying on that alone.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{Attributes, EdgeIx, Graph, NodeIx, Value};

/// Deepest [`Expr::depth`] that serde (de)serializes; deeper expressions
/// are an error instead of a stack overflow.
pub const MAX_EXPR_DEPTH: usize = 100;

/// Comparison operator of [`Expr::Compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A predicate over a node or an edge.
///
/// Serialized externally tagged, e.g. `{"Compare": {"path": ["age"], "op":
/// "Ge", "value": {"Int": 18}}}` or `{"Label": "Person"}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Expr {
    /// Always true / false.
    Const(bool),
    /// The attribute at `path` (name, then keys into nested dicts) compared
    /// with `value`. `Lt`..`Ge` only hold between two numbers, two strings
    /// or two bools.
    Compare {
        path: Vec<String>,
        op: CmpOp,
        value: Value,
    },
    /// The attribute equals one of `values`.
    In {
        path: Vec<String>,
        values: Vec<Value>,
    },
    /// The attribute exists and is not none.
    Exists {
        path: Vec<String>,
    },
    /// Nodes: carries this label. Always false for edges.
    Label(String),
    /// Edges: has this type. Always false for nodes.
    Type(String),
    And(#[serde(with = "nested")] Vec<Expr>),
    Or(#[serde(with = "nested")] Vec<Expr>),
    Not(#[serde(with = "nested")] Box<Expr>),
}

/// Serde helpers for the operands of `And` / `Or` / `Not`: count the
/// nesting depth (per thread) and refuse to go deeper than `MAX_EXPR_DEPTH`.
mod nested {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::cell::Cell;

    use super::MAX_EXPR_DEPTH;
    use crate::value::nested::enter_level;

    thread_local! {
        // `And` / `Or` / `Not` entered on this thread
        static DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    fn message() -> String {
        format!("expression nested more than {MAX_EXPR_DEPTH} levels deep")
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        enter_level(&DEPTH, MAX_EXPR_DEPTH, || crate::format::ser_error::<S::Error>(message()), || v.serialize(s))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
        enter_level(&DEPTH, MAX_EXPR_DEPTH, || crate::format::de_error::<D::Error>(message()), || T::deserialize(d))
    }
}

/// What evaluation needs besides the payload.
struct Context<'a> {
    label: &'a dyn Fn(&str) -> bool,
    ty: &'a dyn Fn(&str) -> bool,
    /// The value of a reserved attribute path, if `path` is one.
    reserved: &'a dyn Fn(&[String]) -> Option<Option<Value>>,
}

impl CmpOp {
    fn holds(self, attr: Option<&Value>, value: &Value) -> bool {
        let Some(a) = attr else { return false };
        match self {
            CmpOp::Eq => a.loose_eq(value),
            CmpOp::Ne => !a.loose_eq(value),
            CmpOp::Lt => a.loose_cmp(value) == Some(Ordering::Less),
            CmpOp::Le => matches!(a.loose_cmp(value), Some(Ordering::Less | Ordering::Equal)),
            CmpOp::Gt => a.loose_cmp(value) == Some(Ordering::Greater),
            CmpOp::Ge => matches!(a.loose_cmp(value), Some(Ordering::Greater | Ordering::Equal)),
        }
    }
}

impl Expr {
    /// Read an expression from its serde form in JSON, up to
    /// [`MAX_EXPR_DEPTH`] levels of `And` / `Or` / `Not` (and values up to
    /// [`MAX_DEPTH`](crate::format::MAX_DEPTH) levels deep). Prefer it to
    /// `serde_json::from_str`, which stops at 64 levels.
    pub fn from_json_str(json: &str) -> Result<Expr, crate::GraphError> {
        crate::value::from_json_str(json)
    }

    /// Whether the node matches.
    pub fn matches_node<N: Attributes, E>(&self, g: &Graph<N, E>, ix: NodeIx) -> Result<bool, N::Error> {
        let Some(node) = g.node(ix) else { return Ok(false) };
        let ctx = Context {
            label: &|name| g.symbol(name).is_some_and(|s| node.has_label(s)),
            ty: &|_| false,
            reserved: &|path| {
                (path.len() == 1 && path[0] == "labels").then(|| {
                    let names = node.labels().iter().map(|&s| Value::String(g.symbol_name(s).to_owned()));
                    Some(Value::List(names.collect()))
                })
            },
        };
        self.eval(&node.data, &ctx)
    }

    /// Whether the edge matches.
    pub fn matches_edge<N, E: Attributes>(&self, g: &Graph<N, E>, e: EdgeIx) -> Result<bool, E::Error> {
        let Some(edge) = g.edge(e) else { return Ok(false) };
        let ty = edge.edge_type();
        let ctx = Context {
            label: &|_| false,
            ty: &|name| ty.is_some() && g.symbol(name) == ty,
            reserved: &|path| {
                (path.len() == 1 && path[0] == "type").then(|| ty.map(|t| Value::String(g.symbol_name(t).to_owned())))
            },
        };
        self.eval(&edge.data, &ctx)
    }

    fn eval<P: Attributes>(&self, data: &P, ctx: &Context<'_>) -> Result<bool, P::Error> {
        // An attribute's value: a graph field for reserved names
        let with = |path: &[String], f: &dyn Fn(Option<&Value>) -> bool| -> Result<bool, P::Error> {
            match (ctx.reserved)(path) {
                Some(v) => Ok(f(v.as_ref().filter(|v| !v.is_none()))),
                None => data.with_value(path, f),
            }
        };
        Ok(match self {
            Expr::Const(b) => *b,
            Expr::Compare { path, op, value } => with(path, &|a| op.holds(a, value))?,
            Expr::In { path, values } => with(path, &|a| a.is_some_and(|a| values.iter().any(|v| a.loose_eq(v))))?,
            Expr::Exists { path } => with(path, &|a| a.is_some())?,
            Expr::Label(name) => (ctx.label)(name),
            Expr::Type(name) => (ctx.ty)(name),
            Expr::And(items) => {
                for item in items {
                    if !item.eval(data, ctx)? {
                        return Ok(false);
                    }
                }
                true
            }
            Expr::Or(items) => {
                for item in items {
                    if item.eval(data, ctx)? {
                        return Ok(true);
                    }
                }
                false
            }
            Expr::Not(inner) => !inner.eval(data, ctx)?,
        })
    }

    /// Nesting depth (1 for a leaf).
    pub fn depth(&self) -> usize {
        match self {
            Expr::And(items) | Expr::Or(items) => 1 + items.iter().map(Expr::depth).max().unwrap_or(0),
            Expr::Not(inner) => 1 + inner.depth(),
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{format, GraphError, Record};

    fn path(p: &str) -> Vec<String> {
        p.split('.').map(str::to_owned).collect()
    }

    fn cmp(p: &str, op: CmpOp, v: impl Into<Value>) -> Expr {
        Expr::Compare { path: path(p), op, value: v.into() }
    }

    #[test]
    fn nodes_and_edges() {
        let mut g: Graph<Record, Record> = Graph::new();
        let mut pos = std::collections::HashMap::new();
        pos.insert("lat".to_owned(), Value::from(52.5));
        let a = g
            .add_node(
                "a",
                Record::with_attr([
                    ("age", Value::from(30)),
                    ("name", Value::from("Ann")),
                    ("pos", Value::Dict(pos)),
                    ("gone", Value::None),
                ]),
            )
            .unwrap();
        let b = g.add_node("b", Record::with_attr([("age", Value::from(17.5))])).unwrap();
        g.add_label(a, "Person").unwrap();
        let e = g.insert_edge(a, b, None, Some("knows"), Record::with_attr([("w", Value::from(2))])).unwrap();

        let node = |x: &Expr, ix| x.matches_node::<_, _>(&g, ix).map_err(|e: GraphError| e).unwrap();
        assert!(node(&cmp("age", CmpOp::Ge, 30.0), a) && !node(&cmp("age", CmpOp::Ge, 30), b));
        assert!(node(&cmp("age", CmpOp::Eq, 30.0), a)); // int == float
        assert!(node(&cmp("name", CmpOp::Lt, "Bob"), a));
        assert!(!node(&cmp("name", CmpOp::Lt, 5), a)); // incomparable
        assert!(node(&cmp("pos.lat", CmpOp::Gt, 50), a));
        // Missing / none: every comparison false, even !=
        assert!(!node(&cmp("name", CmpOp::Ne, "x"), b) && !node(&cmp("gone", CmpOp::Eq, Value::None), a));
        assert!(node(&Expr::Not(Box::new(cmp("name", CmpOp::Eq, "x"))), b));
        assert!(node(&Expr::Exists { path: path("name") }, a) && !node(&Expr::Exists { path: path("gone") }, a));
        let within = Expr::In { path: path("age"), values: vec![Value::from(1), Value::from(30)] };
        assert!(node(&within, a) && !node(&within, b));
        assert!(node(&Expr::Label("Person".into()), a) && !node(&Expr::Label("Person".into()), b));
        assert!(!node(&Expr::Label("Unused".into()), a) && !node(&Expr::Type("knows".into()), a));
        let both = Expr::And(vec![Expr::Label("Person".into()), cmp("age", CmpOp::Lt, 40)]);
        let either = Expr::Or(vec![Expr::Label("Person".into()), cmp("age", CmpOp::Lt, 18)]);
        assert!(node(&both, a) && !node(&both, b) && node(&either, b));
        assert_eq!(Expr::Not(Box::new(both)).depth(), 3);

        // "labels" / "type" are the graph fields
        let labels = Value::List(vec![Value::from("Person")]);
        assert!(node(&Expr::Compare { path: path("labels"), op: CmpOp::Eq, value: labels.clone() }, a));
        assert!(!node(&Expr::Compare { path: path("labels"), op: CmpOp::Eq, value: labels }, b));
        assert!(node(&Expr::Exists { path: path("labels") }, b)); // an empty list
        let edge = |x: &Expr| x.matches_edge::<_, _>(&g, e).map_err(|e: GraphError| e).unwrap();
        assert!(
            edge(&cmp("type", CmpOp::Eq, "knows"))
                && edge(&Expr::In { path: path("type"), values: vec![Value::from("knows")] })
        );
        assert!(edge(&Expr::Type("knows".into())) && !edge(&Expr::Type("likes".into())));
        assert!(edge(&cmp("w", CmpOp::Eq, 2)) && !edge(&Expr::Label("Person".into())));
    }

    #[test]
    fn serde_round_trip() {
        let e = Expr::And(vec![
            Expr::Label("Person".into()),
            Expr::Not(Box::new(cmp("age", CmpOp::Lt, 18))),
            Expr::Or(vec![
                Expr::In { path: path("pos.city"), values: vec![Value::from("Berlin"), Value::from(1.5)] },
                Expr::Exists { path: path("x") },
                Expr::Type("KNOWS".into()),
                Expr::Const(false),
            ]),
        ]);
        let json = sonic_rs::to_string(&e).unwrap();
        assert!(json.contains(r#"{"Compare":{"path":["age"],"op":"Lt","value":{"Int":18}}}"#), "{json}");
        assert_eq!(sonic_rs::from_str::<Expr>(&json).unwrap(), e);
        let bytes = postcard::to_stdvec(&e).unwrap();
        assert_eq!(postcard::from_bytes::<Expr>(&bytes).unwrap(), e);
    }

    #[test]
    fn serde_depth_limit() {
        let nest = |depth: usize| {
            let mut e = Expr::Const(true);
            for _ in 1..depth {
                e = Expr::Not(Box::new(e));
            }
            e
        };
        let ok = nest(MAX_EXPR_DEPTH);
        assert_eq!(ok.depth(), MAX_EXPR_DEPTH);
        let json = sonic_rs::to_string(&ok).unwrap();
        assert_eq!(sonic_rs::from_str::<Expr>(&json).unwrap(), ok);
        let err = sonic_rs::to_string(&nest(MAX_EXPR_DEPTH + 1)).unwrap_err();
        assert!(err.to_string().contains("nested more than"), "{err}");

        // Crafted input: far deeper than the limit, rejected without
        // overflowing the stack (postcard has no recursion limit of its own)
        let deeper = format!("{}{json}{}", r#"{"Not":"#.repeat(10), "}".repeat(10));
        let err = sonic_rs::from_str::<Expr>(&deeper).unwrap_err();
        assert!(err.to_string().contains("nested more than"), "{err}");
        let mut bytes = vec![8u8; 100_000]; // `Not` is variant 8
        bytes.extend(postcard::to_stdvec(&Expr::Const(true)).unwrap());
        format::take_error();
        assert!(postcard::from_bytes::<Expr>(&bytes).is_err());
        assert!(format::take_error().unwrap().contains("expression nested more than"));

        // Postcard drops custom messages: `take_error` has it
        format::take_error();
        assert!(postcard::to_stdvec(&nest(MAX_EXPR_DEPTH + 1)).is_err());
        let msg = format::take_error().unwrap();
        assert_eq!(msg, format!("expression nested more than {MAX_EXPR_DEPTH} levels deep"));
        assert_eq!(format::take_error(), None);
    }
}
