// ops.rs
//
// Changes to a graph as data. An `Op` names nodes by id and edges by
// `EdgeId` (never by in-process handles), so ops can be written to a log,
// sent elsewhere and replayed. `Graph::apply` performs one op and returns
// the ops that undo it; `Graph::apply_all` applies a batch atomically (on
// the first failure it undoes what it already did). That is the base for a
// write-ahead log, replication, transaction rollback and change feeds.
//
// Every op is checked before the graph is touched, so a failing op leaves
// the graph unchanged. Undoing restores ids, labels, types and payloads; the
// order of edges in adjacency lists may differ (re-added edges go last).

use serde::{Deserialize, Serialize};

use crate::{EdgeId, EdgeIx, Graph, GraphError, NodeIx, Record, Value};

/// Payloads whose attributes ops can set one at a time.
pub trait AttrPatch {
    /// Set (`Some`) or remove (`None`) attribute `key`; returns the previous
    /// value.
    fn set_attr(&mut self, key: &str, value: Option<Value>) -> Option<Value>;
}

impl AttrPatch for Record {
    fn set_attr(&mut self, key: &str, value: Option<Value>) -> Option<Value> {
        match value {
            Some(v) => self.attr.insert(key.to_owned(), v),
            None => self.attr.remove(key),
        }
    }
}

/// One change to a graph. See the module comment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op<N, E> {
    /// Fails if the id is taken.
    AddNode {
        id: String,
        labels: Vec<String>,
        data: N,
    },
    /// Removes the node's edges too.
    RemoveNode {
        id: String,
    },
    RenameNode {
        id: String,
        new_id: String,
    },
    AddLabel {
        id: String,
        label: String,
    },
    RemoveLabel {
        id: String,
        label: String,
    },
    /// Replace the node's payload.
    SetNode {
        id: String,
        data: N,
    },
    /// Set (`Some`) or remove (`None`) one attribute of the node's payload.
    SetNodeAttr {
        id: String,
        key: String,
        value: Option<Value>,
    },
    /// Fails if the edge id is taken or an endpoint is missing. Take new ids
    /// from [`Graph::next_edge_id`].
    AddEdge {
        id: EdgeId,
        from: String,
        to: String,
        ty: Option<String>,
        data: E,
    },
    RemoveEdge {
        id: EdgeId,
    },
    SetEdgeType {
        id: EdgeId,
        ty: Option<String>,
    },
    /// Replace the edge's payload.
    SetEdge {
        id: EdgeId,
        data: E,
    },
    /// Set (`Some`) or remove (`None`) one attribute of the edge's payload.
    SetEdgeAttr {
        id: EdgeId,
        key: String,
        value: Option<Value>,
    },
}

impl<N: AttrPatch, E: AttrPatch> Graph<N, E> {
    fn node_named(&self, id: &str) -> Result<NodeIx, GraphError> {
        self.node_ix(id).ok_or_else(|| GraphError::NodeNotFound(id.to_owned()))
    }

    fn edge_with(&self, id: EdgeId) -> Result<EdgeIx, GraphError> {
        self.edge_ix(id).ok_or(GraphError::EdgeNotFound(id.0))
    }

    /// Apply one op; returns the ops that undo it (apply them in order).
    /// On error the graph is unchanged.
    pub fn apply(&mut self, op: Op<N, E>) -> Result<Vec<Op<N, E>>, GraphError> {
        Ok(match op {
            Op::AddNode { id, labels, data } => {
                if self.contains_node(&id) {
                    return Err(GraphError::DuplicateNode(id));
                }
                let ix = self.add_node(id.clone(), data)?;
                for label in &labels {
                    self.add_label(ix, label)?;
                }
                vec![Op::RemoveNode { id }]
            }
            Op::RemoveNode { id } => {
                let ix = self.node_named(&id)?;
                let node = self.node(ix).expect("just looked up");
                let labels = self.label_names(ix).expect("live").into_iter().map(str::to_owned).collect();
                // Incident edges, each once (a self-loop is in both lists)
                let mut edges: Vec<EdgeIx> = node.out_edges().to_vec();
                edges.extend(node.in_edges().iter().filter(|&&e| self.edge(e).is_some_and(|x| x.source() != ix)));
                let mut undo = Vec::with_capacity(edges.len() + 1);
                let mut restore = Vec::with_capacity(edges.len());
                for e in edges {
                    restore.push(self.take_edge(e));
                }
                let (id, data) = self.remove_node(ix).expect("live");
                undo.push(Op::AddNode { id, labels, data });
                undo.extend(restore);
                undo
            }
            Op::RenameNode { id, new_id } => {
                let ix = self.node_named(&id)?;
                self.rename_node(ix, new_id.clone())?;
                vec![Op::RenameNode { id: new_id, new_id: id }]
            }
            Op::AddLabel { id, label } => {
                let ix = self.node_named(&id)?;
                if self.add_label(ix, &label)? {
                    vec![Op::RemoveLabel { id, label }]
                } else {
                    Vec::new()
                }
            }
            Op::RemoveLabel { id, label } => {
                let ix = self.node_named(&id)?;
                if self.remove_label(ix, &label)? {
                    vec![Op::AddLabel { id, label }]
                } else {
                    Vec::new()
                }
            }
            Op::SetNode { id, data } => {
                let ix = self.node_named(&id)?;
                let old = std::mem::replace(&mut self.node_mut(ix).expect("live").data, data);
                vec![Op::SetNode { id, data: old }]
            }
            Op::SetNodeAttr { id, key, value } => {
                let ix = self.node_named(&id)?;
                let old = self.node_mut(ix).expect("live").data.set_attr(&key, value);
                vec![Op::SetNodeAttr { id, key, value: old }]
            }
            Op::AddEdge { id, from, to, ty, data } => {
                let (a, b) = (self.node_named(&from)?, self.node_named(&to)?);
                self.insert_edge(a, b, Some(id), ty.as_deref(), data)?;
                vec![Op::RemoveEdge { id }]
            }
            Op::RemoveEdge { id } => {
                let e = self.edge_with(id)?;
                vec![self.take_edge(e)]
            }
            Op::SetEdgeType { id, ty } => {
                let e = self.edge_with(id)?;
                let old = self.set_edge_type(e, ty.as_deref())?;
                vec![Op::SetEdgeType { id, ty: old }]
            }
            Op::SetEdge { id, data } => {
                let e = self.edge_with(id)?;
                let old = std::mem::replace(&mut self.edge_mut(e).expect("live").data, data);
                vec![Op::SetEdge { id, data: old }]
            }
            Op::SetEdgeAttr { id, key, value } => {
                let e = self.edge_with(id)?;
                let old = self.edge_mut(e).expect("live").data.set_attr(&key, value);
                vec![Op::SetEdgeAttr { id, key, value: old }]
            }
        })
    }

    /// Remove a live edge; the op that adds it back.
    fn take_edge(&mut self, e: EdgeIx) -> Op<N, E> {
        let ty = self.edge_type_name(e).map(str::to_owned);
        let edge = self.remove_edge(e).expect("live");
        let name = |ix: NodeIx| self.node(ix).expect("endpoints are live").id().to_owned();
        Op::AddEdge { id: edge.id(), from: name(edge.source()), to: name(edge.target()), ty, data: edge.data }
    }

    /// Apply `ops` in order, all or nothing: if one fails, the ones before
    /// it are undone and the error is returned (with the failing op's
    /// position). Returns the ops that undo the whole batch.
    pub fn apply_all(&mut self, ops: impl IntoIterator<Item = Op<N, E>>) -> Result<Vec<Op<N, E>>, (usize, GraphError)> {
        let mut undo: Vec<Vec<Op<N, E>>> = Vec::new();
        for (i, op) in ops.into_iter().enumerate() {
            match self.apply(op) {
                Ok(inverse) => undo.push(inverse),
                Err(e) => {
                    for inverse in undo.into_iter().rev() {
                        for op in inverse {
                            self.apply(op).expect("undoing an applied op succeeds");
                        }
                    }
                    return Err((i, e));
                }
            }
        }
        Ok(undo.into_iter().rev().flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format;

    type G = Graph<Record, Record>;
    type O = Op<Record, Record>;

    fn rec(k: &str, v: i64) -> Record {
        Record::with_attr([(k, Value::from(v))])
    }

    /// Everything that must survive an undo, in a comparable form.
    fn state(g: &G) -> Vec<String> {
        let mut out: Vec<String> = g
            .nodes()
            .map(|(ix, n)| format!("node {} {:?} {:?}", n.id(), g.label_names(ix).unwrap(), sorted(&n.data)))
            .collect();
        out.extend(g.edges().map(|(e, x)| {
            let (a, b) = (g.node(x.source()).unwrap().id(), g.node(x.target()).unwrap().id());
            format!("edge {} {a}->{b} {:?} {:?}", x.id().0, g.edge_type_name(e), sorted(&x.data))
        }));
        out.sort();
        out
    }

    fn sorted(r: &Record) -> Vec<(String, String)> {
        let mut v: Vec<_> = r.attr.iter().map(|(k, v)| (k.clone(), format!("{v:?}"))).collect();
        v.sort();
        v
    }

    fn sample() -> G {
        let mut g = G::new();
        let ops: Vec<O> = vec![
            Op::AddNode { id: "a".into(), labels: vec!["Person".into()], data: rec("age", 30) },
            Op::AddNode { id: "b".into(), labels: vec![], data: Record::default() },
            Op::AddEdge {
                id: EdgeId(0),
                from: "a".into(),
                to: "b".into(),
                ty: Some("knows".into()),
                data: rec("w", 1),
            },
            Op::AddEdge { id: EdgeId(1), from: "b".into(), to: "a".into(), ty: None, data: Record::default() },
            Op::AddEdge { id: EdgeId(2), from: "a".into(), to: "a".into(), ty: Some("self".into()), data: rec("w", 2) },
        ];
        g.apply_all(ops).unwrap();
        g
    }

    #[test]
    fn every_op_is_undone_by_its_inverse() {
        let ops: Vec<O> = vec![
            Op::AddNode { id: "c".into(), labels: vec!["X".into(), "Y".into()], data: rec("k", 1) },
            Op::RemoveNode { id: "a".into() },
            Op::RenameNode { id: "a".into(), new_id: "z".into() },
            Op::AddLabel { id: "b".into(), label: "New".into() },
            Op::AddLabel { id: "a".into(), label: "Person".into() }, // already there: no-op
            Op::RemoveLabel { id: "a".into(), label: "Person".into() },
            Op::SetNode { id: "a".into(), data: rec("x", 9) },
            Op::SetNodeAttr { id: "a".into(), key: "age".into(), value: Some(Value::from(31)) },
            Op::SetNodeAttr { id: "a".into(), key: "age".into(), value: None },
            Op::SetNodeAttr { id: "a".into(), key: "new".into(), value: Some(Value::from("v")) },
            Op::AddEdge { id: EdgeId(7), from: "b".into(), to: "b".into(), ty: None, data: Record::default() },
            Op::RemoveEdge { id: EdgeId(0) },
            Op::RemoveEdge { id: EdgeId(2) },
            Op::SetEdgeType { id: EdgeId(0), ty: None },
            Op::SetEdgeType { id: EdgeId(1), ty: Some("t".into()) },
            Op::SetEdge { id: EdgeId(0), data: rec("q", 3) },
            Op::SetEdgeAttr { id: EdgeId(0), key: "w".into(), value: Some(Value::from(5)) },
        ];
        for op in ops {
            let mut g = sample();
            let before = state(&g);
            let undo = g.apply(op.clone()).unwrap();
            if !undo.is_empty() {
                assert_ne!(state(&g), before, "{op:?} changed nothing");
            }
            for u in undo {
                g.apply(u).unwrap();
            }
            assert_eq!(state(&g), before, "{op:?}");
        }
    }

    #[test]
    fn failures_leave_the_graph_unchanged() {
        let bad: Vec<O> = vec![
            Op::AddNode { id: "a".into(), labels: vec![], data: Record::default() },
            Op::RemoveNode { id: "zz".into() },
            Op::RenameNode { id: "a".into(), new_id: "b".into() },
            Op::AddEdge { id: EdgeId(0), from: "a".into(), to: "b".into(), ty: None, data: Record::default() },
            Op::AddEdge { id: EdgeId(9), from: "a".into(), to: "zz".into(), ty: None, data: Record::default() },
            Op::RemoveEdge { id: EdgeId(99) },
            Op::SetEdgeAttr { id: EdgeId(99), key: "k".into(), value: None },
        ];
        for op in bad {
            let mut g = sample();
            let before = state(&g);
            assert!(g.apply(op.clone()).is_err(), "{op:?}");
            assert_eq!(state(&g), before, "{op:?}");
        }
    }

    #[test]
    fn batches_are_atomic_and_undoable() {
        let mut g = sample();
        let before = state(&g);
        let batch: Vec<O> = vec![
            Op::AddNode { id: "c".into(), labels: vec![], data: Record::default() },
            Op::AddEdge { id: g.next_edge_id(), from: "c".into(), to: "a".into(), ty: None, data: Record::default() },
            Op::RemoveNode { id: "b".into() },
            Op::RemoveNode { id: "b".into() }, // fails: already gone
        ];
        assert_eq!(g.apply_all(batch.clone()).unwrap_err().0, 3);
        assert_eq!(state(&g), before);

        let undo = g.apply_all(batch[..3].to_vec()).unwrap();
        assert_ne!(state(&g), before);
        g.apply_all(undo).unwrap();
        assert_eq!(state(&g), before);
    }

    #[test]
    fn ops_round_trip_through_serde_and_replay_elsewhere() {
        let ops: Vec<O> = vec![
            Op::AddNode { id: "a".into(), labels: vec!["L".into()], data: rec("k", 1) },
            Op::AddEdge { id: EdgeId(5), from: "a".into(), to: "a".into(), ty: Some("t".into()), data: rec("w", 2) },
            Op::SetNodeAttr { id: "a".into(), key: "list".into(), value: Some(Value::from(vec![1i64, 2])) },
        ];
        let log = sonic_rs::to_string(&ops).unwrap();
        let replayed: Vec<O> = sonic_rs::from_str(&log).unwrap();
        assert_eq!(replayed, ops);
        let (mut g1, mut g2) = (G::new(), G::new());
        g1.apply_all(ops).unwrap();
        g2.apply_all(replayed).unwrap();
        assert_eq!(state(&g1), state(&g2));
        assert_eq!(g2.next_edge_id(), EdgeId(6));

        // Deeply nested values in a log are rejected, not a stack overflow
        // MAX_DEPTH lists around an int: one level too deep
        let deep =
            format!("{}{{\"Int\":1}}{}", "{\"List\":[".repeat(format::MAX_DEPTH), "]}".repeat(format::MAX_DEPTH));
        let doc = format!("{{\"SetNodeAttr\":{{\"id\":\"a\",\"key\":\"k\",\"value\":{deep}}}}}");
        let err = sonic_rs::from_str::<O>(&doc).unwrap_err().to_string();
        assert!(err.contains("nested more than"), "{err}");
    }
}
