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
use std::fmt;

use serde::{Deserialize, Serialize};

use super::paths::Hops;
use crate::{CmpOp, Expr, GraphError, Value};

mod text;

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
        crate::serde_support::from_json_str(json)
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
        text::write(self, true)
    }
}

impl fmt::Display for Pattern {
    /// The text form; parts that [`Pattern::to_text`] rejects are written
    /// in `<...>` (which `Pattern::parse` rejects in turn).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let out = text::write(self, false).expect("lenient writing does not fail");
        f.write_str(&out)
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

    #[test]
    fn display_preserves_quoted_references_and_merged_properties() {
        let p = Pattern::parse(
            r#"(`node a`:`Label A` {`key a`: "line\n\t\\\""})<-[`edge a`:`Type A`|`Type B`*0..2 {`weight a`: 1.0}]-(b), (b)--(`node a`:`Label B` {flag: true})"#,
        )
        .unwrap();
        assert_eq!(p.nodes.len(), 2);
        assert_eq!(p.nodes[0].labels, ["Label A", "Label B"]);
        assert_eq!(p.nodes[0].filter, Some(Expr::And(vec![eq("key a", "line\n\t\\\""), eq("flag", true)])));
        assert_eq!((p.edges[0].from, p.edges[0].to), (1, 0));
        let written = p.to_text().unwrap();
        assert_eq!(
            written,
            r#"(`node a`:`Label A`:`Label B` {`key a`: "line\n\t\\\"", flag: true})<-[`edge a`:`Type A`|`Type B`*0..2 {`weight a`: 1.0}]-(b)--(`node a`)"#
        );
        assert_eq!(p.to_string(), written);
        assert_eq!(Pattern::parse(&written).unwrap(), p);
    }

    /// A random pattern text: node names from a small pool (so they
    /// repeat), anonymous nodes, labels, properties, edges of every shape.
    fn random_text(rng: &mut rand::rngs::StdRng) -> String {
        use rand::RngExt;
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
        use rand::{RngExt, SeedableRng};
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
