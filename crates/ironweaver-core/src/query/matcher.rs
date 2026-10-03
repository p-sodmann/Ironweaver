// query/matcher.rs
//
// Finding every occurrence of a [`Pattern`] in a graph by backtracking.
//
// Semantics (Cypher's): each pattern edge binds a different graph edge (a
// variable-length edge binds a trail, and none of its edges may be bound
// elsewhere in the match); nodes may repeat, so two node variables can bind
// the same node. An undirected pattern edge matches each graph edge from
// both ends, so `(a)--(b)` reports every connected pair both ways round.
//
// Plan: start from the most selective node variable (bound ids, then the
// rarest label, else every node; a filter counts as selective), then repeatedly take a pattern edge with
// a bound end, preferring edges whose other end is bound too (a cheap
// check) and single edges over variable-length ones. Disconnected parts of
// a pattern start again from their own most selective node (a cartesian
// product). Matches come out in a deterministic order for a given graph.
//
// Everything is streamed: candidates and steps are read one at a time and
// a variable-length edge binds each path as it is found, so work and
// memory between two matches are bounded by a `Budget`
// (`for_each_match_limited`): every node checked against a node variable
// counts as visited (as does every step of a variable-length edge), every
// edge looked at from a bound node as examined, every match as a result.

use super::paths::{Steps, Uniqueness};
use super::pattern::Pattern;
use crate::budget::{Budget, Limited, Meter};
use crate::graph::IxSet;
use crate::{Attributes, Direction, EdgeIx, Graph, GraphError, NodeIx, Symbol};

/// What a pattern edge bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    Edge(EdgeIx),
    /// A variable-length edge: the edges from the pattern edge's `from`
    /// node to its `to` node (possibly none, for a minimum length of 0).
    Path(Vec<EdgeIx>),
}

/// One occurrence of a pattern: a graph node per pattern node and a
/// binding per pattern edge, in the pattern's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub nodes: Vec<NodeIx>,
    pub edges: Vec<Bound>,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    /// Try every candidate for an unbound node variable.
    Scan(usize),
    /// Follow a pattern edge from its bound end (`from` if `forward`).
    Expand { edge: usize, forward: bool },
}

/// Pattern constraints resolved against the graph.
struct Compiled {
    labels: Vec<Vec<Symbol>>,
    ids: Vec<Option<IxSet<NodeIx>>>,
    /// `None`: any type.
    types: Vec<Option<Vec<Symbol>>>,
    /// Candidates for a node's filter from property indexes (slot order).
    indexed: Vec<Option<Vec<NodeIx>>>,
}

/// Resolve labels, types and ids; `None` if the pattern can't match (an
/// unknown label, no known type among an edge's types, no known id).
fn compile<N, E>(g: &Graph<N, E>, pattern: &Pattern) -> Option<Compiled> {
    let mut labels = Vec::new();
    let mut ids = Vec::new();
    for n in &pattern.nodes {
        labels.push(n.labels.iter().map(|l| g.symbol(l)).collect::<Option<Vec<_>>>()?);
        ids.push(match &n.ids {
            None => None,
            Some(list) => {
                let set: IxSet<NodeIx> = list.iter().filter_map(|id| g.node_ix(id)).collect();
                if set.is_empty() {
                    return None;
                }
                Some(set)
            }
        });
    }
    let mut types = Vec::new();
    for e in &pattern.edges {
        types.push(if e.types.is_empty() {
            None
        } else {
            let known: Vec<Symbol> = e.types.iter().filter_map(|t| g.symbol(t)).collect();
            if known.is_empty() && e.hops.is_none_or(|h| h.min > 0) {
                return None;
            }
            Some(known)
        });
    }
    Some(Compiled { labels, ids, types, indexed: vec![None; pattern.nodes.len()] })
}

fn plan<N, E>(g: &Graph<N, E>, pattern: &Pattern, compiled: &Compiled) -> Vec<Step> {
    let cost = |i: usize| {
        let n = &pattern.nodes[i];
        let by_label = n.labels.iter().map(|l| g.label_count(l)).min().unwrap_or(usize::MAX);
        let by_id = n.ids.as_ref().map_or(usize::MAX, Vec::len);
        if let Some(c) = &compiled.indexed[i] {
            // Index candidates are (nearly) the exact answer
            return c.len().min(by_label).min(by_id);
        }
        let c = by_label.min(by_id).min(g.node_count());
        // A filter usually rules out most candidates
        if n.filter.is_some() {
            c / 4
        } else {
            c
        }
    };
    let mut bound = vec![false; pattern.nodes.len()];
    let mut done = vec![false; pattern.edges.len()];
    let mut steps = Vec::new();
    loop {
        // Rank: both ends bound (0), single edge (1), variable length (2)
        let next = (0..pattern.edges.len())
            .filter(|&k| !done[k] && (bound[pattern.edges[k].from] || bound[pattern.edges[k].to]))
            .min_by_key(|&k| {
                let e = &pattern.edges[k];
                if bound[e.from] && bound[e.to] {
                    0
                } else if e.hops.is_none() {
                    1
                } else {
                    2
                }
            });
        if let Some(k) = next {
            let e = &pattern.edges[k];
            steps.push(Step::Expand { edge: k, forward: bound[e.from] });
            bound[e.from] = true;
            bound[e.to] = true;
            done[k] = true;
            continue;
        }
        match (0..pattern.nodes.len()).filter(|&i| !bound[i]).min_by_key(|&i| cost(i)) {
            Some(i) => {
                steps.push(Step::Scan(i));
                bound[i] = true;
            }
            None => return steps,
        }
    }
}

struct Matcher<'a, N, E, V> {
    g: &'a Graph<N, E>,
    pattern: &'a Pattern,
    compiled: Compiled,
    steps: Vec<Step>,
    nodes: Vec<Option<NodeIx>>,
    edges: Vec<Option<Bound>>,
    /// Edges bound so far (a stack; matches are small).
    used: Vec<EdgeIx>,
    visit: V,
    stop: crate::cancel::Stop,
    meter: Meter,
}

impl<N, E, X, V> Matcher<'_, N, E, V>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
    V: FnMut(&Match) -> Result<bool, X>,
{
    fn node_ok(&self, v: usize, ix: NodeIx) -> Result<bool, X> {
        let Some(node) = self.g.node(ix) else { return Ok(false) };
        if !self.compiled.labels[v].iter().all(|&l| node.has_label(l)) {
            return Ok(false);
        }
        if self.compiled.ids[v].as_ref().is_some_and(|ids| !ids.contains(&ix)) {
            return Ok(false);
        }
        match &self.pattern.nodes[v].filter {
            Some(f) => Ok(f.matches_node(self.g, ix)?),
            None => Ok(true),
        }
    }

    fn edge_ok(&self, k: usize, e: EdgeIx) -> Result<bool, X> {
        if let Some(types) = &self.compiled.types[k] {
            let ty = self.g.edge_ref(e).edge_type();
            if !ty.is_some_and(|t| types.contains(&t)) {
                return Ok(false);
            }
        }
        match &self.pattern.edges[k].filter {
            Some(f) => Ok(f.matches_edge(self.g, e)?),
            None => Ok(true),
        }
    }

    /// Bind node variable `v` to `ix` (or check it, if bound); then go on.
    fn with_node(&mut self, v: usize, ix: NodeIx, i: usize) -> Result<bool, X> {
        match self.nodes[v] {
            Some(bound) if bound == ix => self.go(i + 1),
            Some(_) => Ok(true),
            None => {
                if !self.meter.enter() {
                    return Ok(false);
                }
                if !self.node_ok(v, ix)? {
                    return Ok(true);
                }
                self.nodes[v] = Some(ix);
                let more = self.go(i + 1)?;
                self.nodes[v] = None;
                Ok(more)
            }
        }
    }

    /// Run steps `i..`; false once the visitor asked to stop.
    fn go(&mut self, i: usize) -> Result<bool, X> {
        if self.stop.poll() {
            return Ok(false);
        }
        let Some(&step) = self.steps.get(i) else {
            let m = Match {
                nodes: self.nodes.iter().map(|n| n.expect("every node variable is bound")).collect(),
                edges: self.edges.iter().map(|e| e.clone().expect("every edge variable is bound")).collect(),
            };
            return Ok(self.meter.produce() && (self.visit)(&m)?);
        };
        match step {
            Step::Scan(v) => {
                let by_index = self.compiled.indexed[v].as_ref();
                let by_label = || self.pattern.nodes[v].labels.iter().map(|l| self.g.label_count(l)).min();
                let candidates: Vec<NodeIx> = match (&self.compiled.ids[v], self.pattern.nodes[v].labels.first()) {
                    (None, _) if by_index.is_some_and(|c| by_label().is_none_or(|l| c.len() <= l)) => {
                        by_index.expect("checked").clone()
                    }
                    (Some(ids), _) if by_index.is_some_and(|c| c.len() < ids.len()) => {
                        by_index.expect("checked").clone()
                    }
                    (Some(ids), _) => {
                        let mut c: Vec<NodeIx> = ids.iter().copied().collect();
                        c.sort_unstable_by_key(|ix| ix.slot());
                        c
                    }
                    (None, Some(_)) => {
                        let rarest = self.pattern.nodes[v].labels.iter().min_by_key(|l| self.g.label_count(l));
                        self.g.nodes_with_label(rarest.expect("has a label"))
                    }
                    (None, None) => {
                        // Every node: read lazily, not copied
                        let g = self.g;
                        for ix in g.node_indices() {
                            if !self.with_node(v, ix, i)? {
                                return Ok(false);
                            }
                        }
                        return Ok(true);
                    }
                };
                for ix in candidates {
                    if !self.with_node(v, ix, i)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Step::Expand { edge: k, forward } => {
                let pe = &self.pattern.edges[k];
                let (near, far) = if forward { (pe.from, pe.to) } else { (pe.to, pe.from) };
                let u = self.nodes[near].expect("planned from a bound node");
                let direction = match (pe.directed, forward) {
                    (false, _) => Direction::Both,
                    (true, true) => Direction::Out,
                    (true, false) => Direction::In,
                };
                let g = self.g;
                let both = direction == Direction::Both;
                let Some(hops) = pe.hops else {
                    let mut next = Steps::new(g, u, direction);
                    while let Some((e, n)) = next.next(g, both) {
                        if self.stop.poll() || !self.meter.examine() {
                            return Ok(false);
                        }
                        if self.used.contains(&e) || !self.edge_ok(k, e)? {
                            continue;
                        }
                        self.used.push(e);
                        self.edges[k] = Some(Bound::Edge(e));
                        let more = self.with_node(far, n, i)?;
                        self.edges[k] = None;
                        self.used.pop();
                        if !more {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                };
                // A variable-length edge: trails from `u` enumerated
                // depth-first (like `expand_paths`), each bound as it is
                // found. The trail's edges sit on top of `used`, so they are
                // neither repeated nor bound elsewhere.
                let base = self.used.len();
                if hops.min == 0 && !self.with_path(k, far, u, base, forward, i)? {
                    return Ok(false);
                }
                if hops.max == Some(0) {
                    return Ok(true);
                }
                let mut stack = vec![Steps::new(g, u, direction)];
                while let Some(frame) = stack.last_mut() {
                    if self.stop.poll() {
                        return Ok(false);
                    }
                    let Some((e, n)) = frame.next(g, both) else {
                        stack.pop();
                        if !stack.is_empty() {
                            self.used.pop();
                        }
                        continue;
                    };
                    if !self.meter.examine() {
                        return Ok(false);
                    }
                    if self.used.contains(&e) || !self.edge_ok(k, e)? {
                        continue;
                    }
                    if !self.meter.enter() {
                        return Ok(false);
                    }
                    self.used.push(e);
                    let len = self.used.len() - base;
                    if len >= hops.min && !self.with_path(k, far, n, base, forward, i)? {
                        return Ok(false);
                    }
                    if hops.max.is_none_or(|max| len < max) {
                        stack.push(Steps::new(g, n, direction));
                    } else {
                        self.used.pop();
                    }
                }
                Ok(true)
            }
        }
    }

    /// Bind pattern edge `k` to the trail `used[base..]` (from `u`, which
    /// is turned round unless `forward`) and node variable `far` to its
    /// end `n`; then go on.
    fn with_path(&mut self, k: usize, far: usize, n: NodeIx, base: usize, forward: bool, i: usize) -> Result<bool, X> {
        let mut edges = self.used[base..].to_vec();
        if !forward {
            edges.reverse();
        }
        self.edges[k] = Some(Bound::Path(edges));
        let more = self.with_node(far, n, i)?;
        self.edges[k] = None;
        Ok(more)
    }
}

/// Call `visit` with every match of `pattern` in `g` (see the module
/// comment), until it returns false.
pub fn for_each_match<N, E, X>(
    g: &Graph<N, E>,
    pattern: &Pattern,
    visit: impl FnMut(&Match) -> Result<bool, X>,
) -> Result<(), X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    match_metered(g, pattern, Meter::new(Budget::UNLIMITED), visit)?;
    Ok(())
}

/// [`for_each_match`] under a [`Budget`] (see the module comment for what
/// counts). `visit` returning false stops the search without marking it
/// truncated.
pub fn for_each_match_limited<N, E, X>(
    g: &Graph<N, E>,
    pattern: &Pattern,
    budget: Budget,
    visit: impl FnMut(&Match) -> Result<bool, X>,
) -> Result<Limited<()>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    let meter = match_metered(g, pattern, Meter::new(budget), visit)?;
    Ok(meter.finish(())?)
}

/// Every match of `pattern` in `g` within `budget` (with `max_results`
/// as the limit on matches).
pub fn find_matches_limited<N, E, X>(
    g: &Graph<N, E>,
    pattern: &Pattern,
    budget: Budget,
) -> Result<Limited<Vec<Match>>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    let mut out = Vec::new();
    let meter = match_metered(g, pattern, Meter::new(budget), |m| {
        out.push(m.clone());
        Ok::<bool, X>(true)
    })?;
    Ok(meter.finish(out)?)
}

fn match_metered<N, E, X>(
    g: &Graph<N, E>,
    pattern: &Pattern,
    meter: Meter,
    visit: impl FnMut(&Match) -> Result<bool, X>,
) -> Result<Meter, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    for e in &pattern.edges {
        if e.from >= pattern.nodes.len() || e.to >= pattern.nodes.len() {
            return Err(GraphError::InvalidArgument("pattern edge refers to a missing node variable".into()).into());
        }
        if let Some(h) = e.hops {
            super::paths::check_hops(h, Uniqueness::Trail)?;
        }
    }
    let Some(mut compiled) = compile(g, pattern) else { return Ok(meter) };
    if !g.index_paths().is_empty() {
        for (i, n) in pattern.nodes.iter().enumerate() {
            if let Some(f) = &n.filter {
                let found = g.index_candidates(f)?;
                if found.as_ref().is_some_and(Vec::is_empty) {
                    return Ok(meter);
                }
                compiled.indexed[i] = found;
            }
        }
    }
    let steps = plan(g, pattern, &compiled);
    let mut m = Matcher {
        g,
        pattern,
        compiled,
        steps,
        nodes: vec![None; pattern.nodes.len()],
        edges: vec![None; pattern.edges.len()],
        used: Vec::new(),
        visit,
        stop: crate::cancel::stop(),
        meter,
    };
    m.go(0)?;
    Ok(m.meter)
}

/// Every match of `pattern` in `g`, at most `limit`.
pub fn find_matches<N, E, X>(g: &Graph<N, E>, pattern: &Pattern, limit: Option<usize>) -> Result<Vec<Match>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    let mut out = Vec::new();
    if limit == Some(0) {
        return Ok(out);
    }
    for_each_match(g, pattern, |m| {
        out.push(m.clone());
        Ok::<bool, X>(limit.is_none_or(|l| out.len() < l))
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CmpOp, Expr, Record, Value};

    type G = Graph<Record, Record>;

    /// People who know each other, work somewhere:
    /// ann -knows-> bob -knows-> cat -knows-> ann, bob -knows-> dan,
    /// ann/bob -works_at-> acme, cat -works_at-> init; dan -likes-> dan.
    fn graph() -> G {
        let mut g = G::new();
        for (id, label, age) in
            [("ann", "Person", 31), ("bob", "Person", 25), ("cat", "Person", 40), ("dan", "Person", 19)]
        {
            let ix = g.add_node(id, Record::with_attr([("age", Value::from(age))])).unwrap();
            g.add_label(ix, label).unwrap();
        }
        for id in ["acme", "init"] {
            let ix = g.add_node(id, Record::default()).unwrap();
            g.add_label(ix, "Company").unwrap();
        }
        let ix = |g: &G, id: &str| g.node_ix(id).unwrap();
        for (a, b, t) in [
            ("ann", "bob", "knows"),
            ("bob", "cat", "knows"),
            ("cat", "ann", "knows"),
            ("bob", "dan", "knows"),
            ("ann", "acme", "works_at"),
            ("bob", "acme", "works_at"),
            ("cat", "init", "works_at"),
            ("dan", "dan", "likes"),
        ] {
            let (x, y) = (ix(&g, a), ix(&g, b));
            g.insert_edge(x, y, None, Some(t), Record::default()).unwrap();
        }
        g
    }

    /// Matches as rows of node ids (named variables, in pattern order),
    /// sorted.
    fn rows(g: &G, text: &str) -> Vec<String> {
        rows_of(g, &Pattern::parse(text).unwrap())
    }

    fn rows_of(g: &G, p: &Pattern) -> Vec<String> {
        let found: Vec<Match> = find_matches::<_, _, GraphError>(g, p, None).unwrap();
        let mut out: Vec<String> = found
            .iter()
            .map(|m| {
                let mut parts: Vec<String> = p
                    .nodes
                    .iter()
                    .zip(&m.nodes)
                    .filter(|(n, _)| n.name.is_some())
                    .map(|(_, &ix)| g.node(ix).unwrap().id().to_owned())
                    .collect();
                for (e, b) in p.edges.iter().zip(&m.edges) {
                    if let (Some(_), Bound::Path(edges)) = (&e.name, b) {
                        parts.push(format!("{}", edges.len()));
                    }
                }
                parts.join(" ")
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn single_edges_and_labels() {
        let g = graph();
        assert_eq!(rows(&g, "(a:Person)-[:works_at]->(c:Company)"), ["ann acme", "bob acme", "cat init"]);
        assert_eq!(rows(&g, "(c:Company)<-[:works_at]-(a)"), ["acme ann", "acme bob", "init cat"]);
        // Colleagues: nodes may repeat unless the edges differ
        assert_eq!(rows(&g, "(a)-[:works_at]->(c)<-[:works_at]-(b)"), ["ann acme bob", "bob acme ann"]);
        // Types are alternatives; undirected edges match both ways
        assert_eq!(rows(&g, "(a {age: 19})-[:knows|likes]-(b)"), ["dan bob", "dan dan"]);
        // A cycle closes on a bound node
        assert_eq!(
            rows(&g, "(a)-[:knows]->(b)-[:knows]->(c)-[:knows]->(a)"),
            ["ann bob cat", "bob cat ann", "cat ann bob"]
        );
        assert_eq!(rows(&g, "(a)-->(a)"), ["dan"]);
        assert!(rows(&g, "(a:Nope)").is_empty() && rows(&g, "(a)-[:nope]->(b)").is_empty());
        // Disconnected parts: a cartesian product
        assert_eq!(rows(&g, "(a:Company), (b {age: 40})"), ["acme cat", "init cat"]);
    }

    #[test]
    fn variable_length_and_filters() {
        let g = graph();
        // Who ann reaches in 1..2 knows-hops (trails: bob, cat via bob, dan via bob)
        assert_eq!(rows(&g, "(a {age: 31})-[p:knows*1..2]->(b)"), ["ann bob 1", "ann cat 2", "ann dan 2"]);
        // Zero hops binds the start itself; the path can come back round
        assert_eq!(
            rows(&g, "(a {age: 31})-[p:knows*0..3]->(b:Person)"),
            ["ann ann 0", "ann ann 3", "ann bob 1", "ann cat 2", "ann dan 2"]
        );
        // Edges bound elsewhere in the match are not reused (ann -> bob)
        assert_eq!(
            rows(&g, "(a {age: 31})-[:knows]->(b)-[p:knows*]->(c)"),
            ["ann bob ann 2", "ann bob cat 1", "ann bob dan 1"]
        );
        // Filters and ids added from outside the text
        let mut p = Pattern::parse("(a)-[k:knows]->(b)").unwrap();
        p.add_filter("b", Expr::Compare { path: vec!["age".into()], op: CmpOp::Lt, value: Value::from(30) }).unwrap();
        assert_eq!(rows_of(&g, &p), ["ann bob", "bob dan"]);
        p.bind_ids("a", vec!["bob".into(), "zed".into()]).unwrap();
        assert_eq!(rows_of(&g, &p), ["bob dan"]);
        // Paths go from `from` to `to`, even when matching starts at `to`
        let mut p = Pattern::parse("(a)-[p:knows*2]->(b)").unwrap();
        p.bind_ids("b", vec!["dan".into()]).unwrap();
        let m = find_matches::<_, _, GraphError>(&g, &p, None).unwrap();
        assert_eq!(m.len(), 1);
        let Bound::Path(edges) = &m[0].edges[0] else { panic!("a path") };
        let sources: Vec<&str> = edges.iter().map(|&e| g.node(g.edge(e).unwrap().source()).unwrap().id()).collect();
        assert_eq!(sources, ["ann", "bob"]);
    }

    #[test]
    fn limits_and_errors() {
        let g = graph();
        let p = Pattern::parse("(a)-->(b)").unwrap();
        assert_eq!(find_matches::<_, _, GraphError>(&g, &p, Some(3)).unwrap().len(), 3);
        assert_eq!(find_matches::<_, _, GraphError>(&g, &p, None).unwrap().len(), 8);
        assert!(find_matches::<_, _, GraphError>(&g, &p, Some(0)).unwrap().is_empty());
        let bad = Pattern::parse("(a)-[*3..2]->(b)").unwrap();
        assert!(find_matches::<_, _, GraphError>(&g, &bad, None).is_err());
    }

    /// Every assignment of graph edges to the pattern's (single) edges,
    /// distinct, with node variables following from the edges; isolated
    /// node variables range over all nodes.
    fn brute(g: &G, p: &Pattern) -> Vec<Match> {
        let edges: Vec<EdgeIx> = g.edges().map(|(e, _)| e).collect();
        let nodes: Vec<NodeIx> = g.node_indices().collect();
        let node_ok = |v: usize, ix: NodeIx| {
            let n = &p.nodes[v];
            let node = g.node(ix).unwrap();
            n.labels.iter().all(|l| g.symbol(l).is_some_and(|s| node.has_label(s)))
                && n.ids.as_ref().is_none_or(|ids| ids.iter().any(|id| id == node.id()))
                && n.filter.as_ref().is_none_or(|f| f.matches_node::<_, _>(g, ix).map_err(|e: GraphError| e).unwrap())
        };
        let mut out = Vec::new();
        let k = p.edges.len();
        let mut choice = vec![0usize; k];
        loop {
            // Orientations of undirected pattern edges: every bit pattern
            for flips in 0..(1u32 << k) {
                let mut bound: Vec<Option<NodeIx>> = vec![None; p.nodes.len()];
                let mut ok = true;
                for (j, pe) in p.edges.iter().enumerate() {
                    let e = edges[choice[j]];
                    let edge = g.edge(e).unwrap();
                    let flipped = flips & (1 << j) != 0;
                    if (pe.directed && flipped) || choice[..j].contains(&choice[j]) {
                        ok = false;
                        break;
                    }
                    if !pe.directed && flipped && edge.source() == edge.target() {
                        ok = false; // a self-loop matches an undirected edge once
                        break;
                    }
                    let types_ok = pe.types.is_empty()
                        || edge.edge_type().is_some_and(|t| pe.types.iter().any(|name| g.symbol(name) == Some(t)));
                    let (s, t) = if flipped { (edge.target(), edge.source()) } else { (edge.source(), edge.target()) };
                    for (v, ix) in [(pe.from, s), (pe.to, t)] {
                        match bound[v] {
                            Some(b) if b != ix => ok = false,
                            _ => bound[v] = Some(ix),
                        }
                    }
                    ok &= types_ok;
                }
                if ok && bound.iter().enumerate().all(|(v, b)| b.is_none_or(|ix| node_ok(v, ix))) {
                    // Free node variables: every node
                    let free: Vec<usize> = (0..bound.len()).filter(|&v| bound[v].is_none()).collect();
                    let mut idx = vec![0usize; free.len()];
                    'free: loop {
                        let mut full = bound.clone();
                        for (f, &v) in free.iter().enumerate() {
                            full[v] = Some(nodes[idx[f]]);
                        }
                        if free.iter().all(|&v| node_ok(v, full[v].unwrap())) {
                            out.push(Match {
                                nodes: full.into_iter().map(Option::unwrap).collect(),
                                edges: choice.iter().map(|&c| Bound::Edge(edges[c])).collect(),
                            });
                        }
                        for i in idx.iter_mut() {
                            *i += 1;
                            if *i < nodes.len() {
                                continue 'free;
                            }
                            *i = 0;
                        }
                        break;
                    }
                }
            }
            // Next edge assignment
            let mut j = 0;
            loop {
                if j == k {
                    return out;
                }
                choice[j] += 1;
                if choice[j] < edges.len() {
                    break;
                }
                choice[j] = 0;
                j += 1;
            }
        }
    }

    #[test]
    fn matches_brute_force_on_random_graphs() {
        use crate::algo::testing::Lcg;
        let patterns = [
            "(a)-[:x]->(b)",
            "(a:A)-->(b:B)",
            "(a)--(b)",
            "(a)-->(b)-->(c)",
            "(a)-[:x|y]->(b)<-[:y]-(c:A)",
            "(a)-->(b)-->(c)-->(a)",
            "(a)--(b)--(c)--(a)",
            "(a:A)-->(a)",
            "(a)-->(b), (c:B)",
            "(a {w: 1})-[:x]-(b)-->(b)",
            "(a {w: 0})-->(b {w: 1})",
            "(a:A {w: 1})-->(b {w: 2})",
        ];
        let mut total = 0;
        for seed in 0..8 {
            let mut rng = Lcg::new(seed);
            let mut g = G::new();
            let ix: Vec<NodeIx> = (0..6)
                .map(|i| {
                    let n = g
                        .add_node(format!("n{i}"), Record::with_attr([("w", Value::from(rng.below(2) as i64))]))
                        .unwrap();
                    for label in ["A", "B"] {
                        if rng.below(2) == 0 {
                            g.add_label(n, label).unwrap();
                        }
                    }
                    n
                })
                .collect();
            for _ in 0..12 {
                let (a, b) = (ix[rng.below(6) as usize], ix[rng.below(6) as usize]);
                let ty = ["x", "y", "z"][rng.below(3) as usize];
                g.insert_edge(a, b, None, Some(ty), Record::default()).unwrap();
            }
            // Without and with a property index (which changes the plan)
            for indexed in [false, true] {
                if indexed {
                    g.create_index::<GraphError>(&["w".to_string()]).unwrap();
                }
                for text in patterns {
                    let p = Pattern::parse(text).unwrap();
                    let mut got = find_matches::<_, _, GraphError>(&g, &p, None).unwrap();
                    let mut want = brute(&g, &p);
                    let key = |m: &Match| format!("{:?}", m);
                    got.sort_by_key(key);
                    want.sort_by_key(key);
                    assert_eq!(got, want, "seed {seed} indexed {indexed}: {text}");
                    total += got.len();
                }
            }
        }
        assert!(total > 1000, "{total}");
    }
}
