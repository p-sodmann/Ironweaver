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

use std::cmp::Ordering;

use crate::{Attributes, EdgeIx, Graph, NodeIx, Value};

/// Comparison operator of [`Expr::Compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A predicate over a node or an edge.
#[derive(Clone, Debug, PartialEq)]
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
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
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
    use crate::{GraphError, Record};

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
}
