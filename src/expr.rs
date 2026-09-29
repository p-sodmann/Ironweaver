// expr.rs
//
// Python front end of the core filter expressions (`ironweaver_core::Expr`):
// `(attr("age") > 30) & label("Person")` builds an `Expr` that projections
// and `Vertex.filter` evaluate in Rust, without calling Python functions.
// Python's `&` / `|` bind tighter than comparisons, so comparisons need
// parentheses; `30 & label(...)` raises a TypeError that says so.

use ironweaver_core::{CmpOp, Expr as Core, Value};
use pyo3::basic::CompareOp;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;

use crate::convert::to_value;

/// Deepest `&` / `|` / `~` nesting allowed (chains of `&` or `|` flatten).
const MAX_EXPR_DEPTH: usize = 100;

/// A filter expression over a node or an edge, evaluated in Rust. Build it
/// with `attr`, `label` and `edge_type`, and combine with `&`, `|`, `~`.
#[pyclass(frozen, module = "ironweaver", name = "Expr")]
pub struct PyExpr {
    pub(crate) inner: Core,
}

/// An attribute reference; comparing it gives an `Expr`.
#[pyclass(frozen, module = "ironweaver", name = "Attr")]
pub struct PyAttr {
    path: Vec<String>,
}

fn parse_path(path: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    let parts: Vec<String> = if let Ok(s) = path.cast::<PyString>() {
        s.to_str()?.split('.').map(str::to_owned).collect()
    } else {
        path.extract().map_err(|_| PyTypeError::new_err("attr() takes a name like 'pos.lat' or a list of keys"))?
    };
    if parts.is_empty() || parts.iter().any(String::is_empty) {
        return Err(PyValueError::new_err("attribute path must not be empty"));
    }
    Ok(parts)
}

fn describe(v: &Value) -> String {
    match v {
        Value::String(s) => format!("{:?}", s),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{:?}", f),
        Value::Half(h) => format!("{:?}", h.to_f64()),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::None => "None".into(),
        Value::List(items) => format!("[{}]", items.iter().map(describe).collect::<Vec<_>>().join(", ")),
        Value::Dict(_) => "{...}".into(),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        Value::Date(d) => format!("date({d})"),
        Value::DateTime(t) => format!("datetime({t})"),
    }
}

fn repr(e: &Core) -> String {
    let path = |p: &[String]| format!("attr({:?})", p.join("."));
    match e {
        Core::Const(b) => if *b { "True" } else { "False" }.into(),
        Core::Compare { path: p, op, value } => {
            let op = match op {
                CmpOp::Eq => "==",
                CmpOp::Ne => "!=",
                CmpOp::Lt => "<",
                CmpOp::Le => "<=",
                CmpOp::Gt => ">",
                CmpOp::Ge => ">=",
            };
            format!("({} {} {})", path(p), op, describe(value))
        }
        Core::In { path: p, values } => {
            format!("{}.is_in([{}])", path(p), values.iter().map(describe).collect::<Vec<_>>().join(", "))
        }
        Core::Exists { path: p } => format!("{}.exists()", path(p)),
        Core::Label(l) => format!("label({:?})", l),
        Core::Type(t) => format!("edge_type({:?})", t),
        Core::And(items) => format!("({})", items.iter().map(repr).collect::<Vec<_>>().join(" & ")),
        Core::Or(items) => format!("({})", items.iter().map(repr).collect::<Vec<_>>().join(" | ")),
        Core::Not(inner) => format!("~{}", repr(inner)),
    }
}

fn precedence() -> PyErr {
    PyTypeError::new_err(
        "put comparisons in parentheses: (attr(\"age\") > 30) & label(\"Person\") \
         (& and | bind tighter than > in Python)",
    )
}

fn checked(e: Core) -> PyResult<PyExpr> {
    if e.depth() > MAX_EXPR_DEPTH {
        return Err(PyValueError::new_err(format!("expression nested more than {} levels deep", MAX_EXPR_DEPTH)));
    }
    Ok(PyExpr { inner: e })
}

/// `a & b` / `a | b`, flattening chains so they stay shallow.
fn combine(a: &Core, b: &Core, and: bool) -> Core {
    let mut items = Vec::new();
    for e in [a, b] {
        match (e, and) {
            (Core::And(xs), true) | (Core::Or(xs), false) => items.extend(xs.iter().cloned()),
            _ => items.push(e.clone()),
        }
    }
    if and {
        Core::And(items)
    } else {
        Core::Or(items)
    }
}

#[pymethods]
impl PyExpr {
    fn __and__(&self, other: &PyExpr) -> PyResult<PyExpr> {
        checked(combine(&self.inner, &other.inner, true))
    }

    fn __or__(&self, other: &PyExpr) -> PyResult<PyExpr> {
        checked(combine(&self.inner, &other.inner, false))
    }

    /// `30 & label(...)`: what `attr("x") > 30 & label(...)` turns into.
    fn __rand__(&self, _other: &Bound<'_, PyAny>) -> PyResult<PyExpr> {
        Err(precedence())
    }

    fn __ror__(&self, _other: &Bound<'_, PyAny>) -> PyResult<PyExpr> {
        Err(precedence())
    }

    fn __invert__(&self) -> PyResult<PyExpr> {
        checked(match &self.inner {
            Core::Not(inner) => (**inner).clone(),
            e => Core::Not(Box::new(e.clone())),
        })
    }

    /// Guards against `a and b` / `1 < attr("x") < 5`, which Python would
    /// evaluate with truthiness instead of building an expression.
    fn __bool__(&self) -> PyResult<bool> {
        Err(PyTypeError::new_err("use & | ~ to combine expressions (not and / or / not, or chained comparisons)"))
    }

    fn __repr__(&self) -> String {
        repr(&self.inner)
    }
}

#[pymethods]
impl PyAttr {
    fn __richcmp__(&self, other: &Bound<'_, PyAny>, op: CompareOp) -> PyResult<PyExpr> {
        let op = match op {
            CompareOp::Eq => CmpOp::Eq,
            CompareOp::Ne => CmpOp::Ne,
            CompareOp::Lt => CmpOp::Lt,
            CompareOp::Le => CmpOp::Le,
            CompareOp::Gt => CmpOp::Gt,
            CompareOp::Ge => CmpOp::Ge,
        };
        Ok(PyExpr { inner: Core::Compare { path: self.path.clone(), op, value: to_value(other)? } })
    }

    /// True if the attribute equals one of `values`.
    fn is_in(&self, values: Vec<Bound<'_, PyAny>>) -> PyResult<PyExpr> {
        let values = values.iter().map(to_value).collect::<PyResult<_>>()?;
        Ok(PyExpr { inner: Core::In { path: self.path.clone(), values } })
    }

    /// True if the attribute exists and is not None.
    fn exists(&self) -> PyExpr {
        PyExpr { inner: Core::Exists { path: self.path.clone() } }
    }

    fn __bool__(&self) -> PyResult<bool> {
        Err(PyTypeError::new_err("compare attr(...) with a value to build an expression"))
    }

    fn __repr__(&self) -> String {
        format!("attr({:?})", self.path.join("."))
    }
}

/// An attribute of the node / edge: `attr("age") > 30`, `attr("pos.lat")`
/// (keys into nested dicts), `attr(["a.b"])` for names containing dots.
#[pyfunction]
pub fn attr(path: &Bound<'_, PyAny>) -> PyResult<PyAttr> {
    Ok(PyAttr { path: parse_path(path)? })
}

/// Nodes carrying this label (always false for edges).
#[pyfunction]
pub fn label(name: String) -> PyExpr {
    PyExpr { inner: Core::Label(name) }
}

/// Edges of this type (always false for nodes).
#[pyfunction]
pub fn edge_type(name: String) -> PyExpr {
    PyExpr { inner: Core::Type(name) }
}
