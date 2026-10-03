// query/pattern.rs
//
// Graph patterns: node variables with labels and filters, joined by edge
// patterns with types, a direction and optionally a variable length. They
// can be built directly or parsed from a Cypher-like text:
//
//   (a:Person {name: "Ann"})-[k:KNOWS|LIKES*1..3]->(b)<-[:WORKS_AT]-(c:Company), (b)--(d)
//
// - `(name:Label1:Label2 {key: value, ...})`: every part optional; a name
//   used twice is the same node. Labels must all be present.
// - `-[name:TYPE1|TYPE2*min..max {key: value}]->`, `<-[...]-` or `-[...]-`
//   (either direction); `-->`, `<--` and `--` without a body. Any of the
//   types matches; no types match any edge.
// - Lengths: `*` (1 or more), `*n` (exactly n), `*n..m`, `*..m`, `*n..`.
// - `{key: value}` requires the attribute to equal a string ('..' or ".."),
//   number, true or false. Other conditions are added as `Expr`s.
// - Names and labels are identifiers (`[A-Za-z_][A-Za-z0-9_]*`) or quoted
//   with backticks.
//
// `Display` writes the text form back: `Pattern::parse(&p.to_string())`
// equals `p` for every pattern `parse` returns, and for every pattern
// `to_text` accepts. Patterns the text can't express (filters other than
// property equality, bound ids, ...) are written with those parts in
// `<...>`, which `parse` rejects. Patterns also (de)serialize with serde,
// with nothing lost (unknown fields are refused; `Pattern::from_json_str`).

use std::collections::HashMap;
use std::fmt::{self, Write as _};

use serde::{Deserialize, Serialize};

use super::paths::Hops;
use crate::{CmpOp, Expr, GraphError, Value};

/// A node variable of a [`Pattern`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePattern {
    /// `None` for anonymous nodes (not reported in matches).
    pub name: Option<String>,
    /// Labels the node must all carry.
    pub labels: Vec<String>,
    pub filter: Option<Expr>,
    /// If set, the node must have one of these ids.
    pub ids: Option<Vec<String>>,
}

/// An edge between two node variables of a [`Pattern`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgePattern {
    pub name: Option<String>,
    /// Node variables (indices into `Pattern::nodes`). A directed pattern
    /// goes from `from` to `to`.
    pub from: usize,
    pub to: usize,
    /// False: an edge in either direction matches.
    pub directed: bool,
    /// The edge must have one of these types; any edge if empty.
    pub types: Vec<String>,
    pub filter: Option<Expr>,
    /// `None`: exactly one edge. Otherwise a path of edges that each match
    /// the pattern.
    pub hops: Option<Hops>,
}

/// A pattern to match against a graph; see the module comment.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub nodes: Vec<NodePattern>,
    pub edges: Vec<EdgePattern>,
}

/// `a` and `b`, as one flat `And` if either is one.
fn and(a: Option<Expr>, b: Expr) -> Expr {
    let Some(a) = a else { return b };
    let mut items = match a {
        Expr::And(items) => items,
        a => vec![a],
    };
    match b {
        Expr::And(more) => items.extend(more),
        b => items.push(b),
    }
    Expr::And(items)
}

impl Pattern {
    /// Read a pattern from its serde form in JSON, with filters as deep as
    /// [`Expr::from_json_str`] reads them.
    pub fn from_json_str(json: &str) -> Result<Pattern, GraphError> {
        crate::value::from_json_str(json)
    }

    /// Parse the text form (see the module comment).
    pub fn parse(text: &str) -> Result<Pattern, GraphError> {
        Parser { chars: text.chars().collect(), i: 0, pattern: Pattern::default(), names: HashMap::new() }.parse()
    }

    /// Index of the named node variable.
    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name.as_deref() == Some(name))
    }

    /// Index of the named edge variable.
    pub fn edge_index(&self, name: &str) -> Option<usize> {
        self.edges.iter().position(|e| e.name.as_deref() == Some(name))
    }

    /// Add a condition to the named node or edge variable (and-ed with
    /// what it has).
    pub fn add_filter(&mut self, name: &str, filter: Expr) -> Result<(), GraphError> {
        if let Some(i) = self.node_index(name) {
            let n = &mut self.nodes[i];
            n.filter = Some(and(n.filter.take(), filter));
        } else if let Some(i) = self.edge_index(name) {
            let e = &mut self.edges[i];
            e.filter = Some(and(e.filter.take(), filter));
        } else {
            return Err(unknown(name));
        }
        Ok(())
    }

    /// Restrict the named node variable to these ids.
    pub fn bind_ids(&mut self, name: &str, ids: Vec<String>) -> Result<(), GraphError> {
        let i = self.node_index(name).ok_or_else(|| unknown(name))?;
        self.nodes[i].ids = Some(ids);
        Ok(())
    }
}

impl Pattern {
    /// The text form (see the module comment): `Pattern::parse` of it
    /// gives back an equal pattern. Fails, naming the first obstacle, for
    /// patterns the text can't express: filters other than equality of
    /// top-level attributes with strings, integers, finite floats or bools
    /// (one condition, or an `And` of two or more), bound `ids`, repeated
    /// labels, names used twice or containing a backtick, anonymous nodes
    /// that would have to be mentioned twice, or node numbering that no
    /// text order produces.
    pub fn to_text(&self) -> Result<String, GraphError> {
        let mut out = String::new();
        TextWriter { pattern: self, out: &mut out, strict: true, introduced: 0 }.write()?;
        Ok(out)
    }
}

impl fmt::Display for Pattern {
    /// The text form; parts that [`Pattern::to_text`] rejects are written
    /// in `<...>` (which `Pattern::parse` rejects in turn).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        TextWriter { pattern: self, out: &mut out, strict: false, introduced: 0 }
            .write()
            .expect("lenient writing does not fail");
        f.write_str(&out)
    }
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

fn unknown(name: &str) -> GraphError {
    GraphError::InvalidArgument(format!("the pattern has no variable '{}'", name))
}

struct Parser {
    chars: Vec<char>,
    i: usize,
    pattern: Pattern,
    /// Name -> node index, or edge index (`Err`).
    names: HashMap<String, Result<usize, usize>>,
}

impl Parser {
    fn error(&self, what: &str) -> GraphError {
        let near: String = self.chars[self.i.min(self.chars.len())..].iter().take(12).collect();
        let near = if near.is_empty() { "the end".to_owned() } else { format!("'{}'", near) };
        GraphError::InvalidArgument(format!("pattern: {} at position {} (near {})", what, self.i, near))
    }

    fn ws(&mut self) {
        while self.chars.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    /// Skip whitespace, then consume `s` if it comes next.
    fn eat(&mut self, s: &str) -> bool {
        self.ws();
        let n = s.chars().count();
        if self.chars.len() >= self.i + n && self.chars[self.i..self.i + n].iter().copied().eq(s.chars()) {
            self.i += n;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), GraphError> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", s)))
        }
    }

    /// An identifier or a backtick-quoted name, if one comes next.
    fn ident(&mut self) -> Result<Option<String>, GraphError> {
        self.ws();
        if self.peek() == Some('`') {
            self.i += 1;
            let start = self.i;
            while self.peek().is_some_and(|c| c != '`') {
                self.i += 1;
            }
            if self.peek().is_none() {
                return Err(self.error("unclosed '`'"));
            }
            let name: String = self.chars[start..self.i].iter().collect();
            self.i += 1;
            return Ok(Some(name));
        }
        let start = self.i;
        if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
            while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                self.i += 1;
            }
            return Ok(Some(self.chars[start..self.i].iter().collect()));
        }
        Ok(None)
    }

    fn required_ident(&mut self, what: &str) -> Result<String, GraphError> {
        self.ident()?.ok_or_else(|| self.error(&format!("expected {}", what)))
    }

    fn number(&mut self) -> Option<String> {
        self.ws();
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        (self.i > start).then(|| self.chars[start..self.i].iter().collect())
    }

    fn count(&mut self) -> Result<Option<usize>, GraphError> {
        match self.number() {
            None => Ok(None),
            Some(digits) => digits.parse().map(Some).map_err(|_| self.error("length too large")),
        }
    }

    fn parse(mut self) -> Result<Pattern, GraphError> {
        loop {
            self.path()?;
            if self.eat(",") {
                continue;
            }
            self.ws();
            if self.i < self.chars.len() {
                return Err(self.error("expected ',' or the end of the pattern"));
            }
            return Ok(self.pattern);
        }
    }

    fn path(&mut self) -> Result<(), GraphError> {
        let mut left = self.node()?;
        loop {
            self.ws();
            if !matches!(self.peek(), Some('-' | '<')) {
                return Ok(());
            }
            let (mut edge, leftward) = self.edge()?;
            let right = self.node()?;
            (edge.from, edge.to) = if leftward { (right, left) } else { (left, right) };
            self.pattern.edges.push(edge);
            left = right;
        }
    }

    fn node(&mut self) -> Result<usize, GraphError> {
        self.expect("(")?;
        let name = self.ident()?;
        let mut labels = Vec::new();
        while self.eat(":") {
            labels.push(self.required_ident("a label")?);
        }
        let filter = self.properties()?;
        self.expect(")")?;
        let index = match &name {
            Some(n) => match self.names.get(n) {
                Some(Ok(i)) => *i,
                Some(Err(_)) => return Err(self.error(&format!("'{}' is already an edge", n))),
                None => {
                    self.names.insert(n.clone(), Ok(self.pattern.nodes.len()));
                    self.pattern.nodes.push(NodePattern { name: name.clone(), ..Default::default() });
                    self.pattern.nodes.len() - 1
                }
            },
            None => {
                self.pattern.nodes.push(NodePattern::default());
                self.pattern.nodes.len() - 1
            }
        };
        let node = &mut self.pattern.nodes[index];
        for l in labels {
            if !node.labels.contains(&l) {
                node.labels.push(l);
            }
        }
        if let Some(f) = filter {
            node.filter = Some(and(node.filter.take(), f));
        }
        Ok(index)
    }

    /// An edge (its nodes are filled in by `path`), and whether it points
    /// right to left.
    fn edge(&mut self) -> Result<(EdgePattern, bool), GraphError> {
        let leftward = self.eat("<-");
        if !leftward {
            self.expect("-")?;
        }
        let mut edge =
            EdgePattern { name: None, from: 0, to: 0, directed: false, types: Vec::new(), filter: None, hops: None };
        if self.eat("[") {
            edge.name = self.ident()?;
            if let Some(name) = &edge.name {
                if self.names.contains_key(name) {
                    return Err(self.error(&format!("'{}' is already used", name)));
                }
                // The edge is pushed after its right-hand node
                self.names.insert(name.clone(), Err(self.pattern.edges.len()));
            }
            if self.eat(":") {
                edge.types.push(self.required_ident("an edge type")?);
                while self.eat("|") {
                    self.eat(":");
                    edge.types.push(self.required_ident("an edge type")?);
                }
            }
            if self.eat("*") {
                let min = self.count()?;
                edge.hops = Some(if self.eat("..") {
                    Hops { min: min.unwrap_or(1), max: self.count()? }
                } else {
                    match min {
                        Some(n) => Hops::exactly(n),
                        None => Hops { min: 1, max: None },
                    }
                });
            }
            edge.filter = self.properties()?;
            self.expect("]")?;
        }
        self.expect("-")?;
        let rightward = self.eat(">");
        if leftward && rightward {
            return Err(self.error("an edge can't point both ways"));
        }
        edge.directed = leftward || rightward;
        Ok((edge, leftward))
    }

    /// `{key: value, ...}` as an equality filter (none for `{}`).
    fn properties(&mut self) -> Result<Option<Expr>, GraphError> {
        if !self.eat("{") {
            return Ok(None);
        }
        let mut items = Vec::new();
        if !self.eat("}") {
            loop {
                let key = self.required_ident("a property name")?;
                self.expect(":")?;
                let value = self.value()?;
                items.push(Expr::Compare { path: vec![key], op: CmpOp::Eq, value });
                if self.eat("}") {
                    break;
                }
                self.expect(",")?;
            }
        }
        Ok(match items.len() {
            0 => None,
            1 => items.pop(),
            _ => Some(Expr::And(items)),
        })
    }

    fn value(&mut self) -> Result<Value, GraphError> {
        self.ws();
        match self.peek() {
            Some(q @ ('"' | '\'')) => {
                self.i += 1;
                let mut s = String::new();
                loop {
                    match self.peek() {
                        None => return Err(self.error("unclosed string")),
                        Some(c) if c == q => {
                            self.i += 1;
                            return Ok(Value::from(s));
                        }
                        Some('\\') => {
                            self.i += 1;
                            let c = self.peek().ok_or_else(|| self.error("unclosed string"))?;
                            s.push(match c {
                                'n' => '\n',
                                't' => '\t',
                                other => other,
                            });
                            self.i += 1;
                        }
                        Some(c) => {
                            s.push(c);
                            self.i += 1;
                        }
                    }
                }
            }
            Some(c) if c == '-' || c == '.' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
                    self.i += 1;
                }
                let text: String = self.chars[start..self.i].iter().collect();
                if let Ok(i) = text.parse::<i64>() {
                    Ok(Value::from(i))
                } else if let Ok(f) = text.parse::<f64>() {
                    Ok(Value::from(f))
                } else {
                    self.i = start;
                    Err(self.error("invalid number"))
                }
            }
            _ => match self.ident()?.as_deref() {
                Some("true") => Ok(Value::from(true)),
                Some("false") => Ok(Value::from(false)),
                _ => Err(self.error("expected a string, number, true or false")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eq(key: &str, v: impl Into<Value>) -> Expr {
        Expr::Compare { path: vec![key.into()], op: CmpOp::Eq, value: v.into() }
    }

    #[test]
    fn parses_paths() {
        let p =
            Pattern::parse("(a:Person {name: 'Ann', age: 30})-[k:KNOWS|:LIKES*1..3]->(b)<-[:WORKS_AT]-(c), (b)--(a)")
                .unwrap();
        assert_eq!(p.nodes.len(), 3);
        assert_eq!(p.nodes[0].labels, ["Person"]);
        assert_eq!(p.nodes[0].filter, Some(Expr::And(vec![eq("name", "Ann"), eq("age", 30)])));
        let e = &p.edges;
        assert_eq!(e.len(), 3);
        assert_eq!((e[0].from, e[0].to, e[0].directed), (0, 1, true));
        assert_eq!(e[0].types, ["KNOWS", "LIKES"]);
        assert_eq!(e[0].hops, Some(Hops { min: 1, max: Some(3) }));
        assert_eq!((e[1].from, e[1].to, e[1].directed, e[1].name.as_deref()), (2, 1, true, None));
        assert_eq!((e[2].from, e[2].to, e[2].directed), (1, 0, false));
        assert_eq!(p.edge_index("k"), Some(0));
        assert_eq!(p.node_index("c"), Some(2));
    }

    #[test]
    fn short_forms_and_lengths() {
        let p =
            Pattern::parse("()-->()<--(x)--( `odd name` :`Some Label`)-[*]->()-[*2]-()-[*..4]->()-[*2..]->()").unwrap();
        assert_eq!(p.nodes.len(), 8);
        assert_eq!(p.nodes[3].name.as_deref(), Some("odd name"));
        assert_eq!(p.nodes[3].labels, ["Some Label"]);
        let hops: Vec<Option<Hops>> = p.edges.iter().map(|e| e.hops).collect();
        assert_eq!(
            hops,
            [
                None,
                None,
                None,
                Some(Hops { min: 1, max: None }),
                Some(Hops::exactly(2)),
                Some(Hops { min: 1, max: Some(4) }),
                Some(Hops { min: 2, max: None }),
            ]
        );
        assert_eq!((p.edges[1].from, p.edges[1].to), (2, 1));
        let v = Pattern::parse("(a {x: -1.5, y: true, s: \"q\\\"\"})").unwrap();
        assert_eq!(v.nodes[0].filter, Some(Expr::And(vec![eq("x", -1.5), eq("y", true), eq("s", "q\"")])));
    }

    #[test]
    fn errors() {
        for (text, msg) in [
            ("(a", "expected ')'"),
            ("(a)-[r]->(b)-[r]->(c)", "already used"),
            ("(a)<-[r]->(b)", "both ways"),
            ("(a)-[r]->(r)", "already an edge"),
            ("(a) x", "expected ','"),
            ("(a {n: })", "expected a string"),
            ("(a:)", "expected a label"),
            ("(a {n: 'x)", "unclosed string"),
        ] {
            let err = Pattern::parse(text).unwrap_err().to_string();
            assert!(err.contains(msg), "{text}: {err}");
        }
        let mut p = Pattern::parse("(a)-[r]->(b)").unwrap();
        p.add_filter("r", Expr::Type("T".into())).unwrap();
        p.add_filter("a", Expr::Label("L".into())).unwrap();
        p.bind_ids("b", vec!["x".into()]).unwrap();
        assert!(p.add_filter("zz", Expr::Const(true)).is_err());
        assert!(p.bind_ids("r", vec![]).is_err());
    }

    #[test]
    fn display_round_trips() {
        for text in [
            "(a:Person {name: 'Ann', age: 30})-[k:KNOWS|:LIKES*1..3]->(b)<-[:WORKS_AT]-(c), (b)--(a)",
            "()-->()<--(x)--( `odd name` :`Some Label`)-[*]->()-[*2]-()-[*..4]->()-[*2..]->()",
            "(a {x: -1.5, y: true, s: \"q\\\"\\\\\\n\\t'\", z: 1e300, w: -0.0, i: -9223372036854775808})",
            "(a {})-[r {}]->(b), (c), (a)-->(a), (d)<--(b)",
            "(a {x: 1})--(a {y: 2, z: 3}), (a {w: 4})",
            "(n), (m)-[*0]-(n), (m)-[e*3..2]->(o)",
            "(`a b`)-[`r r`:`T T`]->(`c`)",
            "(b)<--(a), (c)-->(a)",
            "(x)<-[:T]-(), (y)",
        ] {
            let p = Pattern::parse(text).unwrap();
            let written = p.to_string();
            assert_eq!(p.to_text().map_err(|e| format!("{text}: {e}")).unwrap(), written);
            assert_eq!(Pattern::parse(&written).unwrap(), p, "{text} -> {written}");
        }
        let p = Pattern::parse("(a:Person {name: 'Ann'})-[k:KNOWS*1..3]->(b)<--(c:X:Y)").unwrap();
        assert_eq!(p.to_string(), r#"(a:Person {name: "Ann"})-[k:KNOWS*1..3]->(b)<--(c:X:Y)"#);
        let json = sonic_rs::to_string(&p).unwrap();
        assert_eq!(sonic_rs::from_str::<Pattern>(&json).unwrap(), p);
    }

    /// A random pattern text: node names from a small pool (so they
    /// repeat), anonymous nodes, labels, properties, edges of every shape.
    fn random_text(rng: &mut rand::rngs::StdRng) -> String {
        use rand::Rng;
        let names = ["a", "b", "c", "d", "`x y`", "é_1"];
        let values = ["1", "-2", "0.5", "-1e-7", "true", "false", "'s'", "\"q\\\"x\"", "\"\\\\\"", "''"];
        fn pick(rng: &mut rand::rngs::StdRng, items: &[&'static str]) -> &'static str {
            items[rng.random_range(0..items.len())]
        }
        let node = |rng: &mut rand::rngs::StdRng| {
            let mut t = String::from("(");
            if rng.random_bool(0.7) {
                t.push_str(pick(rng, &names));
            }
            for _ in 0..rng.random_range(0..3) {
                t.push(':');
                t.push_str(pick(rng, &["L", "M", "`N n`"]));
            }
            if rng.random_bool(0.3) {
                let props: Vec<String> = (0..rng.random_range(0..3))
                    .map(|_| format!("{}: {}", pick(rng, &["k", "v", "`w w`"]), pick(rng, &values)))
                    .collect();
                t.push_str(&format!(" {{{}}}", props.join(", ")));
            }
            t.push(')');
            t
        };
        let mut paths = Vec::new();
        for _ in 0..rng.random_range(1..4) {
            let mut t = node(rng);
            for _ in 0..rng.random_range(0..4) {
                let mut body = String::new();
                if rng.random_bool(0.3) {
                    body.push_str(pick(rng, &["r", "s", "t"]));
                }
                if rng.random_bool(0.4) {
                    body.push_str(pick(rng, &[":T", ":T|U", ":T|:U|`V v`"]));
                }
                if rng.random_bool(0.3) {
                    body.push_str(pick(rng, &["*", "*2", "*1..3", "*..4", "*2..", "*0..0"]));
                }
                if rng.random_bool(0.2) {
                    body.push_str(&format!(" {{k: {}}}", pick(rng, &values)));
                }
                let body = if body.is_empty() && rng.random_bool(0.5) { String::new() } else { format!("[{body}]") };
                t.push_str(&match rng.random_range(0..3) {
                    0 => format!("-{body}->"),
                    1 => format!("<-{body}-"),
                    _ => format!("-{body}-"),
                });
                t.push_str(&node(rng));
            }
            paths.push(t);
        }
        paths.join(", ")
    }

    #[test]
    fn display_round_trips_random_patterns() {
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let mut parsed = 0;
        for _ in 0..5000 {
            let text = random_text(&mut rng);
            // Some texts are invalid (an edge name used twice, ...)
            let Ok(p) = Pattern::parse(&text) else { continue };
            parsed += 1;
            let written = p.to_text().unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(Pattern::parse(&written).unwrap(), p, "{text} -> {written}");
        }
        assert!(parsed > 3000, "{parsed}");

        // Arbitrary structs: either written exactly or refused
        let mut exact = 0;
        for _ in 0..5000 {
            let n = rng.random_range(1..5);
            let nodes: Vec<NodePattern> = (0..n)
                .map(|i| NodePattern {
                    name: rng.random_bool(0.6).then(|| format!("n{i}")),
                    labels: if rng.random_bool(0.3) { vec!["L".into()] } else { vec![] },
                    ..Default::default()
                })
                .collect();
            let edges: Vec<EdgePattern> = (0..rng.random_range(0..4))
                .map(|_| EdgePattern {
                    name: None,
                    from: rng.random_range(0..n),
                    to: rng.random_range(0..n),
                    directed: rng.random_bool(0.6),
                    types: vec![],
                    filter: None,
                    hops: None,
                })
                .collect();
            let p = Pattern { nodes, edges };
            if let Ok(text) = p.to_text() {
                exact += 1;
                assert_eq!(Pattern::parse(&text).unwrap(), p, "{text}");
                assert_eq!(p.to_string(), text);
            } else {
                assert!(Pattern::parse(&p.to_string()).is_err(), "{p:?} {}", p);
            }
        }
        assert!(exact > 1000, "{exact}");
    }

    #[test]
    fn display_marks_what_text_cannot_express() {
        let mut p = Pattern::parse("(a)-[r]->(b)").unwrap();
        p.add_filter("a", Expr::Exists { path: vec!["x".into()] }).unwrap();
        p.bind_ids("b", vec!["id1".into()]).unwrap();
        let text = p.to_string();
        assert!(text.starts_with("(a {<Exists"), "{text}");
        assert!(text.contains("<bound ids>"), "{text}");
        assert!(Pattern::parse(&text).is_err());
        assert!(p.to_text().unwrap_err().to_string().contains("can't be written as text"));
        // An anonymous node on three edges can't be written as text
        let anon = Pattern {
            nodes: vec![NodePattern::default(), NodePattern { name: Some("x".into()), ..Default::default() }],
            edges: (0..3)
                .map(|_| EdgePattern {
                    name: None,
                    from: 0,
                    to: 1,
                    directed: true,
                    types: vec![],
                    filter: None,
                    hops: None,
                })
                .collect(),
        };
        assert!(anon.to_text().is_err());
        assert!(Pattern::parse(&anon.to_string()).is_err(), "{anon}");
    }
}
