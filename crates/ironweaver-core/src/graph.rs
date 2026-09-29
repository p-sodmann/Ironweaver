// graph.rs
//
// Node and edge storage. Nodes and edges live in slot arenas; a handle is a
// slot number plus the slot's generation, which is bumped when the slot is
// freed, so a handle to a removed node or edge never resolves to whatever
// reuses its slot. Every edge is listed exactly once in its source's `out`
// list and once in its target's `inc` list, in insertion order.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use crate::{Direction, GraphError};

/// Hasher for `NodeIx` / `EdgeIx` keys: a multiply-rotate mix of the two
/// `u32`s, much cheaper than the default SipHash. Handles are not
/// attacker-controlled, so no DoS resistance is needed.
#[derive(Default, Clone, Copy)]
pub struct IxHasher(u64);

impl IxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for IxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(b as u64);
        }
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// `HashMap` keyed by node or edge handles.
pub type IxMap<K, V> = HashMap<K, V, BuildHasherDefault<IxHasher>>;
/// `HashSet` of node or edge handles.
pub type IxSet<K> = HashSet<K, BuildHasherDefault<IxHasher>>;

/// Handle to a node of a [`Graph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeIx {
    slot: u32,
    generation: u32,
}

/// Handle to an edge of a [`Graph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeIx {
    slot: u32,
    generation: u32,
}

impl NodeIx {
    /// Slot number, below [`Graph::node_bound`]; usable as a dense index.
    pub fn slot(self) -> usize {
        self.slot as usize
    }
}

impl EdgeIx {
    /// Slot number, below [`Graph::edge_bound`]; usable as a dense index.
    pub fn slot(self) -> usize {
        self.slot as usize
    }
}

/// A node: its id, payload and incident edges.
#[derive(Clone, Debug)]
pub struct Node<N> {
    id: String,
    out: Vec<EdgeIx>,
    inc: Vec<EdgeIx>,
    pub data: N,
}

impl<N> Node<N> {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Outgoing edges, in insertion order.
    pub fn out_edges(&self) -> &[EdgeIx] {
        &self.out
    }

    /// Incoming edges, in insertion order.
    pub fn in_edges(&self) -> &[EdgeIx] {
        &self.inc
    }
}

/// A directed edge and its payload.
#[derive(Clone, Debug)]
pub struct Edge<E> {
    from: NodeIx,
    to: NodeIx,
    pub data: E,
}

impl<E> Edge<E> {
    pub fn source(&self) -> NodeIx {
        self.from
    }

    pub fn target(&self) -> NodeIx {
        self.to
    }
}

#[derive(Clone, Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

#[derive(Clone, Debug)]
struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Arena<T> {
    fn with_capacity(n: usize) -> Self {
        Arena { slots: Vec::with_capacity(n), free: Vec::new(), len: 0 }
    }

    fn insert(&mut self, value: T) -> (u32, u32) {
        self.len += 1;
        if let Some(slot) = self.free.pop() {
            let s = &mut self.slots[slot as usize];
            s.value = Some(value);
            return (slot, s.generation);
        }
        let slot = u32::try_from(self.slots.len()).expect("graph too large");
        self.slots.push(Slot { generation: 0, value: Some(value) });
        (slot, 0)
    }

    fn get(&self, slot: u32, generation: u32) -> Option<&T> {
        match self.slots.get(slot as usize) {
            Some(s) if s.generation == generation => s.value.as_ref(),
            _ => None,
        }
    }

    fn get_mut(&mut self, slot: u32, generation: u32) -> Option<&mut T> {
        match self.slots.get_mut(slot as usize) {
            Some(s) if s.generation == generation => s.value.as_mut(),
            _ => None,
        }
    }

    fn remove(&mut self, slot: u32, generation: u32) -> Option<T> {
        let s = self.slots.get_mut(slot as usize)?;
        if s.generation != generation {
            return None;
        }
        let value = s.value.take()?;
        s.generation = s.generation.wrapping_add(1);
        self.free.push(slot);
        self.len -= 1;
        Some(value)
    }

    fn iter(&self) -> impl Iterator<Item = (u32, u32, &T)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.value.as_ref().map(|v| (i as u32, s.generation, v)))
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = (u32, u32, &mut T)> + '_ {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| {
            let generation = s.generation;
            s.value.as_mut().map(|v| (i as u32, generation, v))
        })
    }
}

/// A directed multigraph with string node ids.
///
/// Iteration order is slot order: insertion order until nodes are removed,
/// after which new nodes reuse the freed slots.
#[derive(Clone, Debug)]
pub struct Graph<N, E> {
    nodes: Arena<Node<N>>,
    edges: Arena<Edge<E>>,
    index: HashMap<String, NodeIx>,
}

impl<N, E> Default for Graph<N, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N, E> Graph<N, E> {
    pub fn new() -> Self {
        Self::with_capacity(0, 0)
    }

    pub fn with_capacity(nodes: usize, edges: usize) -> Self {
        Graph {
            nodes: Arena::with_capacity(nodes),
            edges: Arena::with_capacity(edges),
            index: HashMap::with_capacity(nodes),
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len == 0
    }

    /// Upper bound (exclusive) of [`NodeIx::slot`] for every live node.
    pub fn node_bound(&self) -> usize {
        self.nodes.slots.len()
    }

    /// Upper bound (exclusive) of [`EdgeIx::slot`] for every live edge.
    pub fn edge_bound(&self) -> usize {
        self.edges.slots.len()
    }

    /// Add a node; fails if the id is taken.
    pub fn add_node(&mut self, id: impl Into<String>, data: N) -> Result<NodeIx, GraphError> {
        let id = id.into();
        if self.index.contains_key(&id) {
            return Err(GraphError::DuplicateNode(id));
        }
        Ok(self.insert_node(id, data))
    }

    fn insert_node(&mut self, id: String, data: N) -> NodeIx {
        let (slot, generation) = self.nodes.insert(Node { id: id.clone(), out: Vec::new(), inc: Vec::new(), data });
        let ix = NodeIx { slot, generation };
        self.index.insert(id, ix);
        ix
    }

    /// Add an edge from `from` to `to` (both must be live).
    pub fn add_edge(&mut self, from: NodeIx, to: NodeIx, data: E) -> Result<EdgeIx, GraphError> {
        let ix = self.add_edge_detached(from, to, data)?;
        self.attach_out(ix);
        self.attach_in(ix);
        Ok(ix)
    }

    /// Create an edge without listing it on its endpoints yet; the loader
    /// uses this to restore the saved per-node edge order. Callers must
    /// attach every such edge on both ends.
    pub(crate) fn add_edge_detached(&mut self, from: NodeIx, to: NodeIx, data: E) -> Result<EdgeIx, GraphError> {
        if self.node(from).is_none() || self.node(to).is_none() {
            return Err(GraphError::Stale);
        }
        let (slot, generation) = self.edges.insert(Edge { from, to, data });
        Ok(EdgeIx { slot, generation })
    }

    pub(crate) fn attach_out(&mut self, ix: EdgeIx) {
        let from = self.edge_ref(ix).from;
        self.node_ref_mut(from).out.push(ix);
    }

    pub(crate) fn attach_in(&mut self, ix: EdgeIx) {
        let to = self.edge_ref(ix).to;
        self.node_ref_mut(to).inc.push(ix);
    }

    pub fn node_ix(&self, id: &str) -> Option<NodeIx> {
        self.index.get(id).copied()
    }

    pub fn contains_node(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    pub fn node(&self, ix: NodeIx) -> Option<&Node<N>> {
        self.nodes.get(ix.slot, ix.generation)
    }

    pub fn node_mut(&mut self, ix: NodeIx) -> Option<&mut Node<N>> {
        self.nodes.get_mut(ix.slot, ix.generation)
    }

    pub fn node_by_id(&self, id: &str) -> Option<&Node<N>> {
        self.node_ix(id).and_then(|ix| self.node(ix))
    }

    pub fn edge(&self, ix: EdgeIx) -> Option<&Edge<E>> {
        self.edges.get(ix.slot, ix.generation)
    }

    pub fn edge_mut(&mut self, ix: EdgeIx) -> Option<&mut Edge<E>> {
        self.edges.get_mut(ix.slot, ix.generation)
    }

    /// A node known to be live (listed in an edge or adjacency list).
    fn node_ref_mut(&mut self, ix: NodeIx) -> &mut Node<N> {
        self.node_mut(ix).expect("adjacency lists reference live nodes")
    }

    /// An edge known to be live (listed in an adjacency list).
    pub(crate) fn edge_ref(&self, ix: EdgeIx) -> &Edge<E> {
        self.edge(ix).expect("adjacency lists reference live edges")
    }

    /// Change a node's id; fails if the new id is taken by another node.
    pub fn rename_node(&mut self, ix: NodeIx, id: impl Into<String>) -> Result<(), GraphError> {
        let id = id.into();
        let old = self.node(ix).ok_or(GraphError::Stale)?.id.clone();
        if old == id {
            return Ok(());
        }
        if self.index.contains_key(&id) {
            return Err(GraphError::DuplicateNode(id));
        }
        self.index.remove(&old);
        self.index.insert(id.clone(), ix);
        self.node_ref_mut(ix).id = id;
        Ok(())
    }

    /// Remove a node and every edge attached to it; returns its id and payload.
    pub fn remove_node(&mut self, ix: NodeIx) -> Option<(String, N)> {
        let node = self.nodes.remove(ix.slot, ix.generation)?;
        self.index.remove(&node.id);
        for &e in &node.out {
            // Self loops appear in both lists; the second visit finds nothing.
            if let Some(edge) = self.edges.remove(e.slot, e.generation) {
                if edge.to != ix {
                    self.node_ref_mut(edge.to).inc.retain(|&x| x != e);
                }
            }
        }
        for &e in &node.inc {
            if let Some(edge) = self.edges.remove(e.slot, e.generation) {
                if edge.from != ix {
                    self.node_ref_mut(edge.from).out.retain(|&x| x != e);
                }
            }
        }
        Some((node.id, node.data))
    }

    /// Remove one edge; returns it.
    pub fn remove_edge(&mut self, ix: EdgeIx) -> Option<Edge<E>> {
        let edge = self.edges.remove(ix.slot, ix.generation)?;
        self.node_ref_mut(edge.from).out.retain(|&x| x != ix);
        self.node_ref_mut(edge.to).inc.retain(|&x| x != ix);
        Some(edge)
    }

    /// All nodes, in slot order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeIx, &Node<N>)> + '_ {
        self.nodes.iter().map(|(slot, generation, n)| (NodeIx { slot, generation }, n))
    }

    pub fn nodes_mut(&mut self) -> impl Iterator<Item = (NodeIx, &mut Node<N>)> + '_ {
        self.nodes.iter_mut().map(|(slot, generation, n)| (NodeIx { slot, generation }, n))
    }

    pub fn node_indices(&self) -> impl Iterator<Item = NodeIx> + '_ {
        self.nodes().map(|(ix, _)| ix)
    }

    /// All edges, in slot order.
    pub fn edges(&self) -> impl Iterator<Item = (EdgeIx, &Edge<E>)> + '_ {
        self.edges.iter().map(|(slot, generation, e)| (EdgeIx { slot, generation }, e))
    }

    pub fn edges_mut(&mut self) -> impl Iterator<Item = (EdgeIx, &mut Edge<E>)> + '_ {
        self.edges.iter_mut().map(|(slot, generation, e)| (EdgeIx { slot, generation }, e))
    }

    /// Neighbours of `ix` one edge away in `direction`, as `(edge, neighbour)`
    /// pairs: outgoing edges first, then incoming ones. Empty for a stale `ix`.
    pub fn neighbors(&self, ix: NodeIx, direction: Direction) -> impl Iterator<Item = (EdgeIx, NodeIx)> + '_ {
        let node = self.node(ix);
        let out: &[EdgeIx] = match node {
            Some(n) if direction != Direction::In => &n.out,
            _ => &[],
        };
        let inc: &[EdgeIx] = match node {
            Some(n) if direction != Direction::Out => &n.inc,
            _ => &[],
        };
        out.iter().map(move |&e| (e, self.edge_ref(e).to)).chain(inc.iter().map(move |&e| (e, self.edge_ref(e).from)))
    }

    /// A new graph with the nodes in `keep` (in that order, duplicates and
    /// stale handles skipped) and every edge between two of them. Payloads
    /// are produced by `node_data` / `edge_data`; the first error they return
    /// is passed on.
    pub fn induced_subgraph<N2, E2, X>(
        &self,
        keep: impl IntoIterator<Item = NodeIx>,
        mut node_data: impl FnMut(&Node<N>) -> Result<N2, X>,
        mut edge_data: impl FnMut(&Edge<E>) -> Result<E2, X>,
    ) -> Result<Graph<N2, E2>, X> {
        let keep = keep.into_iter();
        let mut out: Graph<N2, E2> = Graph::with_capacity(keep.size_hint().0, 0);
        let mut map: IxMap<NodeIx, NodeIx> = IxMap::with_capacity_and_hasher(keep.size_hint().0, Default::default());
        let mut order: Vec<(NodeIx, NodeIx)> = Vec::with_capacity(keep.size_hint().0);
        for ix in keep {
            if map.contains_key(&ix) {
                continue;
            }
            if let Some(n) = self.node(ix) {
                let new = out.insert_node(n.id.clone(), node_data(n)?);
                map.insert(ix, new);
                order.push((ix, new));
            }
        }

        // Count the edges first so every list is allocated once, at its final
        // size (the new graph's slots are 0..order.len()).
        let mut out_deg = vec![0u32; order.len()];
        let mut in_deg = vec![0u32; order.len()];
        let mut n_edges = 0;
        for &(old, new) in &order {
            for &e in &self.node(old).expect("kept nodes are live").out {
                if let Some(&to) = map.get(&self.edge_ref(e).to) {
                    out_deg[new.slot()] += 1;
                    in_deg[to.slot()] += 1;
                    n_edges += 1;
                }
            }
        }
        out.edges.slots.reserve_exact(n_edges);
        for (slot, _, n) in out.nodes.iter_mut() {
            n.out.reserve_exact(out_deg[slot as usize] as usize);
            n.inc.reserve_exact(in_deg[slot as usize] as usize);
        }
        for &(old, new) in &order {
            for &e in &self.node(old).expect("kept nodes are live").out {
                let edge = self.edge_ref(e);
                if let Some(&to) = map.get(&edge.to) {
                    let data = edge_data(edge)?;
                    out.add_edge(new, to, data).expect("both endpoints were just added");
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> (Graph<(), u32>, [NodeIx; 3]) {
        let mut g = Graph::new();
        let a = g.add_node("a", ()).unwrap();
        let b = g.add_node("b", ()).unwrap();
        let c = g.add_node("c", ()).unwrap();
        g.add_edge(a, b, 1).unwrap();
        g.add_edge(b, c, 2).unwrap();
        g.add_edge(c, a, 3).unwrap();
        (g, [a, b, c])
    }

    #[test]
    fn add_and_look_up() {
        let (mut g, [a, b, _]) = triangle();
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 3);
        assert_eq!(g.node_ix("b"), Some(b));
        assert_eq!(g.add_node("a", ()), Err(GraphError::DuplicateNode("a".into())));
        let out: Vec<_> = g.neighbors(a, Direction::Out).map(|(_, n)| n).collect();
        assert_eq!(out, [b]);
        let both: Vec<_> = g.neighbors(a, Direction::Both).map(|(_, n)| g.node(n).unwrap().id()).collect();
        assert_eq!(both, ["b", "c"]);
    }

    #[test]
    fn remove_node_detaches_edges_and_invalidates_handles() {
        let (mut g, [a, b, c]) = triangle();
        g.add_edge(b, b, 9).unwrap();
        let (id, ()) = g.remove_node(b).unwrap();
        assert_eq!(id, "b");
        assert_eq!(g.edge_count(), 1);
        assert!(g.node(a).unwrap().out_edges().is_empty());
        assert!(g.node(c).unwrap().in_edges().is_empty());
        assert!(g.node(b).is_none());
        // The freed slot is reused, but the old handle stays dead.
        let d = g.add_node("d", ()).unwrap();
        assert_eq!(d.slot(), b.slot());
        assert!(g.node(b).is_none());
        assert!(g.remove_node(b).is_none());
        assert_eq!(g.add_edge(a, b, 0), Err(GraphError::Stale));
    }

    #[test]
    fn remove_edge_and_rename() {
        let (mut g, [a, b, _]) = triangle();
        let e = g.node(a).unwrap().out_edges()[0];
        assert_eq!(g.remove_edge(e).unwrap().data, 1);
        assert!(g.node(b).unwrap().in_edges().is_empty());
        assert!(g.edge(e).is_none());
        g.rename_node(a, "z").unwrap();
        assert_eq!(g.node_ix("z"), Some(a));
        assert!(!g.contains_node("a"));
        assert!(matches!(g.rename_node(a, "b"), Err(GraphError::DuplicateNode(_))));
    }

    #[test]
    fn induced_subgraph_keeps_order_and_internal_edges() {
        let (g, [a, b, c]) = triangle();
        let sub = g.induced_subgraph([c, a, c], |_| Ok::<_, ()>(()), |e| Ok(e.data * 10)).unwrap();
        let ids: Vec<_> = sub.nodes().map(|(_, n)| n.id()).collect();
        assert_eq!(ids, ["c", "a"]);
        assert_eq!(sub.edge_count(), 1);
        let (_, e) = sub.edges().next().unwrap();
        assert_eq!(e.data, 30);
        assert!(sub.node_ix("b").is_none());
        let _ = b;
    }
}
