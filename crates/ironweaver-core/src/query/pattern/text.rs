//! Text rendering for graph patterns, including markers for parts the text cannot express.

use std::fmt::Write as _;

use super::{EdgePattern, Pattern};
use crate::query::paths::Hops;
use crate::{CmpOp, Expr, GraphError, Value};

pub(super) fn write(pattern: &Pattern, strict: bool) -> Result<String, GraphError> {
    let mut out = String::new();
    TextWriter { pattern, out: &mut out, strict, introduced: 0 }.write()?;
    Ok(out)
}

/// Writes a pattern as text. Node variables get their numbers from the
/// order in which the text first mentions them, so the writer mentions
/// node `k` before node `k + 1`, and every mention after the first is a
/// reference by name.
struct TextWriter<'a> {
    pattern: &'a Pattern,
    out: &'a mut String,
    /// Fail on what the text can't express; otherwise write it in `<...>`.
    strict: bool,
    /// Nodes `0..introduced` have been mentioned.
    introduced: usize,
}

/// Whether `s` is written without backticks.
fn is_bare(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

impl TextWriter<'_> {
    /// Something the text can't express: an error, or `<what>` in the text.
    fn unwritable(&mut self, what: &str) -> Result<(), GraphError> {
        if self.strict {
            return Err(GraphError::InvalidArgument(format!("the pattern can't be written as text: {}", what)));
        }
        write!(self.out, "<{}>", what).expect("writing to a String");
        Ok(())
    }

    fn write(mut self) -> Result<(), GraphError> {
        let p = self.pattern;
        let mut names: Vec<&str> = p
            .nodes
            .iter()
            .filter_map(|n| n.name.as_deref())
            .chain(p.edges.iter().filter_map(|e| e.name.as_deref()))
            .collect();
        names.sort_unstable();
        if let Some([name, _]) = names.array_windows().find(|[a, b]| a == b) {
            let what = format!("the name '{}' is used twice", name);
            self.unwritable(&what)?;
        }

        let n = p.nodes.len();
        // Last node of the path being written, if one is open
        let mut end: Option<usize> = None;
        for (i, e) in p.edges.iter().enumerate() {
            if e.from >= n || e.to >= n {
                self.new_path(&mut end);
                self.unwritable(&format!("edge {} refers to a missing node", i))?;
                continue;
            }
            // Continue the open path if the edge touches its end
            let next = end.and_then(|c| {
                let right = if c == e.from {
                    e.to
                } else if c == e.to && e.directed {
                    e.from
                } else {
                    return None;
                };
                self.can_mention(right, self.introduced).then_some((c, right))
            });
            let (left, right) = match next {
                Some(pair) => pair,
                None => {
                    // A new path from `l` to `r`, after mentioning nodes
                    // `introduced..flushed` as paths of their own (only
                    // named ones, or ones no later edge needs)
                    let mut candidates = vec![(e.from, e.to)];
                    if e.directed && e.from != e.to {
                        candidates.push((e.to, e.from));
                    }
                    let next_edge = p.edges.get(i + 1);
                    let intro = self.introduced;
                    let mut best = None;
                    for (l, r) in candidates {
                        for flushed in intro..=l.max(r).max(intro).min(n) {
                            if flushed > intro && !self.can_stand_alone(flushed - 1, i) {
                                break;
                            }
                            let after = if l == flushed { flushed + 1 } else { flushed };
                            if self.can_mention(l, flushed) && self.can_mention(r, after) {
                                // End on a node the next edge needs, so it can
                                // continue the path: above all an anonymous
                                // one, which can't be mentioned again. Then
                                // fewest nodes on paths of their own.
                                let continues = next_edge.is_some_and(|x| x.from == r || x.to == r);
                                let anonymous = self.pattern.nodes[r].name.is_none();
                                let key = (!(continues && anonymous), !continues, flushed - intro);
                                if best.is_none_or(|(k, _)| key < k) {
                                    best = Some((key, (l, r, flushed)));
                                }
                            }
                        }
                    }
                    if best.is_none() && self.strict {
                        return self.unwritable(&format!("no text order numbers the nodes of edge {} as given", i));
                    }
                    let (left, right, flushed) = best.map_or((e.from, e.to, intro), |(_, c)| c);
                    while self.introduced < flushed {
                        self.new_path(&mut end);
                        self.node(self.introduced)?;
                    }
                    self.new_path(&mut end);
                    self.node(left)?;
                    (left, right)
                }
            };
            self.edge(e, left != e.from)?;
            self.node(right)?;
            end = Some(right);
        }
        while self.introduced < n {
            self.new_path(&mut end);
            self.node(self.introduced)?;
        }
        Ok(())
    }

    /// Start a new comma-separated path.
    fn new_path(&mut self, end: &mut Option<usize>) {
        if !self.out.is_empty() {
            self.out.push_str(", ");
        }
        *end = None;
    }

    /// Whether node `x` can be written as a path of its own before edge
    /// `i`: named (so edges can refer to it later), or on no edge from `i`
    /// on.
    fn can_stand_alone(&self, x: usize, i: usize) -> bool {
        self.pattern.nodes[x].name.is_some() || self.pattern.edges[i..].iter().all(|e| e.from != x && e.to != x)
    }

    /// Whether node `x` can be mentioned next, with nodes `0..introduced`
    /// mentioned so far: it is the next new node, or a named earlier one.
    fn can_mention(&self, x: usize, introduced: usize) -> bool {
        x == introduced || (x < introduced && self.pattern.nodes[x].name.is_some())
    }

    fn name(&mut self, s: &str) -> Result<(), GraphError> {
        if is_bare(s) {
            self.out.push_str(s);
        } else if !s.contains('`') {
            write!(self.out, "`{}`", s).expect("writing to a String");
        } else {
            self.unwritable(&format!("the name {:?} contains a backtick", s))?;
        }
        Ok(())
    }

    /// A mention of node `x`: in full the first time, by name after that.
    fn node(&mut self, x: usize) -> Result<(), GraphError> {
        let node = &self.pattern.nodes[x];
        self.out.push('(');
        if x < self.introduced {
            match &node.name {
                Some(name) => self.name(name)?,
                None => self.unwritable(&format!("anonymous node {} again", x))?,
            }
        } else if x > self.introduced {
            self.unwritable(&format!("node {} before node {}", x, self.introduced))?;
        } else {
            self.introduced += 1;
            if let Some(name) = &node.name {
                self.name(name)?;
            }
            for (i, label) in node.labels.iter().enumerate() {
                self.out.push(':');
                self.name(label)?;
                if node.labels[..i].contains(label) {
                    self.unwritable(&format!("label {:?} repeated", label))?;
                }
            }
            self.properties(node.filter.as_ref())?;
            if node.ids.is_some() {
                self.out.push(' ');
                self.unwritable("bound ids")?;
            }
        }
        self.out.push(')');
        Ok(())
    }

    fn edge(&mut self, e: &EdgePattern, leftward: bool) -> Result<(), GraphError> {
        self.out.push_str(if leftward { "<-" } else { "-" });
        if e.name.is_some() || !e.types.is_empty() || e.hops.is_some() || e.filter.is_some() {
            self.out.push('[');
            if let Some(name) = &e.name {
                self.name(name)?;
            }
            for (i, ty) in e.types.iter().enumerate() {
                self.out.push_str(if i == 0 { ":" } else { "|" });
                self.name(ty)?;
            }
            if let Some(Hops { min, max }) = e.hops {
                self.out.push('*');
                match max {
                    Some(max) if max == min => write!(self.out, "{}", min),
                    Some(max) => write!(self.out, "{}..{}", min, max),
                    None if min == 1 => Ok(()),
                    None => write!(self.out, "{}..", min),
                }
                .expect("writing to a String");
            }
            self.properties(e.filter.as_ref())?;
            self.out.push(']');
        }
        self.out.push_str(if e.directed && !leftward { "->" } else { "-" });
        Ok(())
    }

    /// ` {key: value, ...}` for an equality filter.
    fn properties(&mut self, filter: Option<&Expr>) -> Result<(), GraphError> {
        let Some(filter) = filter else { return Ok(()) };
        let items: &[Expr] = match filter {
            Expr::And(items) if items.len() >= 2 => items,
            Expr::And(_) => return self.unwritable("an And of fewer than two conditions"),
            single => std::slice::from_ref(single),
        };
        self.out.push_str(" {");
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            match item {
                Expr::Compare { path, op: CmpOp::Eq, value } if path.len() == 1 => {
                    self.name(&path[0])?;
                    self.out.push_str(": ");
                    self.value(value)?;
                }
                other => self.unwritable(&format!("{:?}", other))?,
            }
        }
        self.out.push('}');
        Ok(())
    }

    fn value(&mut self, v: &Value) -> Result<(), GraphError> {
        match v {
            Value::String(s) => {
                self.out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => self.out.push_str("\\\""),
                        '\\' => self.out.push_str("\\\\"),
                        '\n' => self.out.push_str("\\n"),
                        '\t' => self.out.push_str("\\t"),
                        c => self.out.push(c),
                    }
                }
                self.out.push('"');
            }
            Value::Int(i) => write!(self.out, "{}", i).expect("writing to a String"),
            Value::Bool(b) => write!(self.out, "{}", b).expect("writing to a String"),
            // `{:?}` keeps a `.0` or an exponent, so the text reads back as
            // a float, and is exact
            Value::Float(f) if f.is_finite() => write!(self.out, "{:?}", f).expect("writing to a String"),
            other => self.unwritable(&format!("{:?}", other))?,
        }
        Ok(())
    }
}
