// graph.rs
//
// Node and edge storage. Nodes and edges live in slot arenas; a handle is a
// slot number plus the slot's generation, which is bumped when the slot is
// freed, so a handle to a removed node or edge never resolves to whatever
// reuses its slot. Every edge is listed exactly once in its source's `out`
// list and once in its target's `inc` list, in insertion order.
//
// Handles live only as long as the process. What is saved and survives:
// node ids (strings), `EdgeId`s (a per-graph counter, never reused), node
// labels and edge types. Labels and types are interned `Symbol`s; the graph
// keeps an index from each label to its nodes.

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
    fn write_u64(&mut self, i: u64) {
        self.add(i);
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

/// Edge id -> edge slot. Ids come from a counter, so they are dense: most
/// live in a flat table indexed by id (sequential, cache friendly, 4 bytes
/// per id); ids far beyond the number of edges (explicit ids from replayed
/// ops or hand-written files) go to a hash map, so memory stays bounded.
#[derive(Clone, Debug, Default)]
struct EdgeIndex {
    /// `slot + 1` of the edge with id `i`, 0 for none.
    dense: Vec<u32>,
    sparse: IxMap<EdgeId, u32>,
}

impl EdgeIndex {
    fn get(&self, id: EdgeId) -> Option<u32> {
        match self.dense.get(id.0 as usize) {
            Some(&s) if s != 0 => Some(s - 1),
            Some(_) => None,
            None => self.sparse.get(&id).copied(),
        }
    }

    /// Record `id` -> `slot`. `edges` is the number of live edges (sizes the
    /// dense table).
    /// The caller checks that `id` is free.
    fn insert(&mut self, id: EdgeId, slot: u32, edges: usize) {
        let i = id.0 as usize;
        let limit = (4 * edges).max(1024) as u64;
        if i < self.dense.len() || id.0 < limit {
            if i >= self.dense.len() {
                let len = (i + 1).max(2 * self.dense.len());
                self.dense.resize(len, 0);
            }
            self.dense[i] = slot + 1;
        } else {
            self.sparse.insert(id, slot);
        }
    }

    fn remove(&mut self, id: EdgeId) {
        match self.dense.get_mut(id.0 as usize) {
            Some(s) => *s = 0,
            None => {
                self.sparse.remove(&id);
            }
        }
    }

    fn reserve(&mut self, ids: u64) {
        if ids <= (u32::MAX as u64) && (ids as usize) > self.dense.len() {
            self.dense.resize(ids as usize, 0);
        }
    }
}

/// Persistent id of an edge: assigned from a per-graph counter when the
/// edge is added, never reused, saved with the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct EdgeId(pub u64);

/// An interned node label or edge type; [`Graph::symbol_name`] gives the
/// text. Only meaningful for the graph that interned it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(u32);

/// Interned label / type names.
#[derive(Clone, Debug, Default)]
pub struct Symbols {
    names: Vec<Box<str>>,
    index: HashMap<Box<str>, Symbol>,
}

impl Symbols {
    pub fn get(&self, name: &str) -> Option<Symbol> {
        self.index.get(name).copied()
    }

    pub fn intern(&mut self, name: &str) -> Symbol {
        if let Some(&s) = self.index.get(name) {
            return s;
        }
        let s = Symbol(u32::try_from(self.names.len()).expect("too many labels and types"));
        self.names.push(name.into());
        self.index.insert(name.into(), s);
        s
    }

    pub fn name(&self, s: Symbol) -> &str {
        &self.names[s.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
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

/// A node: its id, labels, payload and incident edges.
#[derive(Clone, Debug)]
pub struct Node<N> {
    id: String,
    /// Sorted, distinct.
    labels: Vec<Symbol>,
    out: Vec<EdgeIx>,
    inc: Vec<EdgeIx>,
    pub data: N,
}

impl<N> Node<N> {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The node's labels, sorted by symbol.
    pub fn labels(&self) -> &[Symbol] {
        &self.labels
    }

    pub fn has_label(&self, label: Symbol) -> bool {
        self.labels.binary_search(&label).is_ok()
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

/// A directed edge: its persistent id, optional type and payload.
#[derive(Clone, Debug)]
pub struct Edge<E> {
    from: NodeIx,
    to: NodeIx,
    id: EdgeId,
    ty: Option<Symbol>,
    pub data: E,
}

impl<E> Edge<E> {
    pub fn id(&self) -> EdgeId {
        self.id
    }

    /// The edge's type, if it has one.
    pub fn edge_type(&self) -> Option<Symbol> {
        self.ty
    }

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

/// A directed multigraph with string node ids, node labels, edge types and
/// persistent edge ids.
///
/// Iteration order is slot order: insertion order until nodes are removed,
/// after which new nodes reuse the freed slots.
#[derive(Clone, Debug)]
pub struct Graph<N, E> {
    nodes: Arena<Node<N>>,
    edges: Arena<Edge<E>>,
    index: HashMap<String, NodeIx>,
    edge_index: EdgeIndex,
    /// The id the next new edge gets.
    next_edge_id: u64,
    symbols: Symbols,
    /// Nodes carrying each label.
    labeled: HashMap<Symbol, IxSet<NodeIx>>,
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
            edge_index: EdgeIndex { dense: Vec::with_capacity(edges), sparse: IxMap::default() },
            next_edge_id: 0,
            symbols: Symbols::default(),
            labeled: HashMap::new(),
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
        let (slot, generation) =
            self.nodes.insert(Node { id: id.clone(), labels: Vec::new(), out: Vec::new(), inc: Vec::new(), data });
        let ix = NodeIx { slot, generation };
        self.index.insert(id, ix);
        ix
    }

    /// Add an edge from `from` to `to` (both must be live), with a new id.
    pub fn add_edge(&mut self, from: NodeIx, to: NodeIx, data: E) -> Result<EdgeIx, GraphError> {
        self.insert_edge(from, to, None, None, data)
    }

    /// Add an edge with a type and/or a given id (`None`: a new one). Fails
    /// if the id is taken; later new ids are above it.
    pub fn insert_edge(
        &mut self,
        from: NodeIx,
        to: NodeIx,
        id: Option<EdgeId>,
        ty: Option<&str>,
        data: E,
    ) -> Result<EdgeIx, GraphError> {
        let ty = ty.map(|t| self.symbols.intern(t));
        let ix = self.add_edge_detached(from, to, id, ty, data)?;
        self.attach_out(ix);
        self.attach_in(ix);
        Ok(ix)
    }

    /// Create an edge without listing it on its endpoints yet; the loader
    /// uses this to restore the saved per-node edge order. Callers must
    /// attach every such edge on both ends.
    pub(crate) fn add_edge_detached(
        &mut self,
        from: NodeIx,
        to: NodeIx,
        id: Option<EdgeId>,
        ty: Option<Symbol>,
        data: E,
    ) -> Result<EdgeIx, GraphError> {
        if self.node(from).is_none() || self.node(to).is_none() {
            return Err(GraphError::Stale);
        }
        let id = id.unwrap_or(EdgeId(self.next_edge_id));
        if self.edge_index.get(id).is_some() {
            return Err(GraphError::DuplicateEdge(id.0));
        }
        self.next_edge_id = self
            .next_edge_id
            .max(id.0.checked_add(1).ok_or(GraphError::InvalidArgument("edge id space exhausted".into()))?);
        let (slot, generation) = self.edges.insert(Edge { from, to, id, ty, data });
        let ix = EdgeIx { slot, generation };
        self.edge_index.insert(id, slot, self.edges.len);
        Ok(ix)
    }

    /// The live edge with this id.
    pub fn edge_ix(&self, id: EdgeId) -> Option<EdgeIx> {
        let slot = self.edge_index.get(id)?;
        let generation = self.edges.slots[slot as usize].generation;
        Some(EdgeIx { slot, generation })
    }

    /// The id the next edge added without an explicit id gets.
    pub fn next_edge_id(&self) -> EdgeId {
        EdgeId(self.next_edge_id)
    }

    /// Make new edge ids start at `id` or later (never lowers the counter);
    /// for restoring a saved graph whose highest ids were removed.
    pub fn reserve_edge_ids(&mut self, id: EdgeId) {
        self.next_edge_id = self.next_edge_id.max(id.0);
    }

    /// Set or clear an edge's type; returns the previous type's name.
    pub fn set_edge_type(&mut self, ix: EdgeIx, ty: Option<&str>) -> Result<Option<String>, GraphError> {
        let sym = ty.map(|t| self.symbols.intern(t));
        let edge = self.edges.get_mut(ix.slot, ix.generation).ok_or(GraphError::Stale)?;
        let old = std::mem::replace(&mut edge.ty, sym);
        Ok(old.map(|s| self.symbols.name(s).to_owned()))
    }

    /// Name of an edge's type.
    pub fn edge_type_name(&self, ix: EdgeIx) -> Option<&str> {
        self.edge(ix)?.ty.map(|s| self.symbols.name(s))
    }

    /// The interned labels and types.
    pub fn symbols(&self) -> &Symbols {
        &self.symbols
    }

    /// Text of a label or type.
    pub fn symbol_name(&self, s: Symbol) -> &str {
        self.symbols.name(s)
    }

    /// The symbol for a label / type name, created if new.
    pub fn intern(&mut self, name: &str) -> Symbol {
        self.symbols.intern(name)
    }

    /// The symbol for a label / type name, if any node or edge ever used it.
    pub fn symbol(&self, name: &str) -> Option<Symbol> {
        self.symbols.get(name)
    }

    /// Add a label to a node; returns whether it was new.
    pub fn add_label(&mut self, ix: NodeIx, label: &str) -> Result<bool, GraphError> {
        self.node(ix).ok_or(GraphError::Stale)?;
        let sym = self.symbols.intern(label);
        let labels = &mut self.node_ref_mut(ix).labels;
        match labels.binary_search(&sym) {
            Ok(_) => Ok(false),
            Err(at) => {
                labels.insert(at, sym);
                self.labeled.entry(sym).or_default().insert(ix);
                Ok(true)
            }
        }
    }

    /// Remove a label from a node; returns whether it had it.
    pub fn remove_label(&mut self, ix: NodeIx, label: &str) -> Result<bool, GraphError> {
        self.node(ix).ok_or(GraphError::Stale)?;
        let Some(sym) = self.symbols.get(label) else { return Ok(false) };
        let labels = &mut self.node_ref_mut(ix).labels;
        match labels.binary_search(&sym) {
            Err(_) => Ok(false),
            Ok(at) => {
                labels.remove(at);
                self.unindex_label(sym, ix);
                Ok(true)
            }
        }
    }

    /// Names of a node's labels (sorted by when each label was first used).
    pub fn label_names(&self, ix: NodeIx) -> Option<Vec<&str>> {
        Some(self.node(ix)?.labels.iter().map(|&s| self.symbols.name(s)).collect())
    }

    /// Nodes carrying `label`, in slot order.
    pub fn nodes_with_label(&self, label: &str) -> Vec<NodeIx> {
        let mut out: Vec<NodeIx> = match self.symbols.get(label).and_then(|s| self.labeled.get(&s)) {
            Some(set) => set.iter().copied().collect(),
            None => Vec::new(),
        };
        out.sort_unstable_by_key(|ix| ix.slot);
        out
    }

    fn unindex_label(&mut self, sym: Symbol, ix: NodeIx) {
        if let Some(set) = self.labeled.get_mut(&sym) {
            set.remove(&ix);
            if set.is_empty() {
                self.labeled.remove(&sym);
            }
        }
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
        for &label in &node.labels {
            self.unindex_label(label, ix);
        }
        for &e in &node.out {
            // Self loops appear in both lists; the second visit finds nothing.
            if let Some(edge) = self.edges.remove(e.slot, e.generation) {
                self.edge_index.remove(edge.id);
                if edge.to != ix {
                    self.node_ref_mut(edge.to).inc.retain(|&x| x != e);
                }
            }
        }
        for &e in &node.inc {
            if let Some(edge) = self.edges.remove(e.slot, e.generation) {
                self.edge_index.remove(edge.id);
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
        self.edge_index.remove(edge.id);
        self.node_ref_mut(edge.from).out.retain(|&x| x != ix);
        self.node_ref_mut(edge.to).inc.retain(|&x| x != ix);
        Some(edge)
    }

    /// Edges from `from` to `to` (optionally only of type `ty`), in `from`'s
    /// outgoing order. Scans the shorter of the two adjacency lists.
    pub fn edges_between(&self, from: NodeIx, to: NodeIx, ty: Option<Symbol>) -> Vec<EdgeIx> {
        let (Some(a), Some(b)) = (self.node(from), self.node(to)) else { return Vec::new() };
        let fits = |e: &EdgeIx| {
            let edge = self.edge_ref(*e);
            edge.from == from && edge.to == to && ty.is_none_or(|t| edge.ty == Some(t))
        };
        if a.out.len() <= b.inc.len() {
            a.out.iter().copied().filter(fits).collect()
        } else {
            let mut found: Vec<EdgeIx> = b.inc.iter().copied().filter(fits).collect();
            let order: IxMap<EdgeIx, usize> = a.out.iter().enumerate().map(|(i, &e)| (e, i)).collect();
            found.sort_by_key(|e| order.get(e).copied());
            found
        }
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
        // Same symbols, so labels and types copy without re-interning
        out.symbols = self.symbols.clone();
        let mut map: IxMap<NodeIx, NodeIx> = IxMap::with_capacity_and_hasher(keep.size_hint().0, Default::default());
        let mut order: Vec<(NodeIx, NodeIx)> = Vec::with_capacity(keep.size_hint().0);
        for ix in keep {
            if map.contains_key(&ix) {
                continue;
            }
            if let Some(n) = self.node(ix) {
                let new = out.insert_node(n.id.clone(), node_data(n)?);
                if !n.labels.is_empty() {
                    out.node_ref_mut(new).labels = n.labels.clone();
                    for &label in &n.labels {
                        out.labeled.entry(label).or_default().insert(new);
                    }
                }
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
        out.edge_index.reserve(self.next_edge_id);
        for (slot, _, n) in out.nodes.iter_mut() {
            n.out.reserve_exact(out_deg[slot as usize] as usize);
            n.inc.reserve_exact(in_deg[slot as usize] as usize);
        }
        for &(old, new) in &order {
            for &e in &self.node(old).expect("kept nodes are live").out {
                let edge = self.edge_ref(e);
                if let Some(&to) = map.get(&edge.to) {
                    let data = edge_data(edge)?;
                    let ix =
                        out.add_edge_detached(new, to, Some(edge.id), edge.ty, data).expect("endpoints just added");
                    out.attach_out(ix);
                    out.attach_in(ix);
                }
            }
        }
        // Ids of edges that were not copied stay unused in the subgraph too
        out.reserve_edge_ids(self.next_edge_id());
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
    fn edge_ids_are_persistent_and_never_reused() {
        let (mut g, [a, b, c]) = triangle();
        let ids: Vec<EdgeId> = g.edges().map(|(_, e)| e.id()).collect();
        assert_eq!(ids, [EdgeId(0), EdgeId(1), EdgeId(2)]);
        let e1 = g.edge_ix(EdgeId(1)).unwrap();
        assert_eq!(g.edge(e1).unwrap().data, 2);
        g.remove_edge(e1).unwrap();
        assert_eq!(g.edge_ix(EdgeId(1)), None);
        let e = g.add_edge(b, c, 5).unwrap();
        assert_eq!(g.edge(e).unwrap().id(), EdgeId(3)); // not 1 again
                                                        // Explicit ids: taken ones fail, later new ids go above
        assert_eq!(g.insert_edge(a, b, Some(EdgeId(3)), None, 0), Err(GraphError::DuplicateEdge(3)));
        let e = g.insert_edge(a, b, Some(EdgeId(10)), Some("knows"), 0).unwrap();
        assert_eq!(g.edge_type_name(e), Some("knows"));
        assert_eq!(g.next_edge_id(), EdgeId(11));
        // Removing a node drops its edges' ids
        g.remove_node(a).unwrap();
        assert_eq!(g.edge_ix(EdgeId(10)), None);
        assert_eq!(g.edge_ix(EdgeId(0)), None);
        assert!(g.edge_ix(EdgeId(3)).is_some());
    }

    #[test]
    fn labels_types_and_the_label_index() {
        let (mut g, [a, b, c]) = triangle();
        assert!(g.add_label(a, "Person").unwrap());
        assert!(!g.add_label(a, "Person").unwrap());
        g.add_label(a, "Admin").unwrap();
        g.add_label(c, "Person").unwrap();
        assert_eq!(g.label_names(a).unwrap(), ["Person", "Admin"]);
        let person = g.symbol("Person").unwrap();
        assert!(g.node(a).unwrap().has_label(person) && !g.node(b).unwrap().has_label(person));
        assert_eq!(g.nodes_with_label("Person"), [a, c]);
        assert!(g.remove_label(a, "Person").unwrap());
        assert!(!g.remove_label(b, "Nope").unwrap());
        assert_eq!(g.nodes_with_label("Person"), [c]);
        g.remove_node(c).unwrap();
        assert!(g.nodes_with_label("Person").is_empty());
        assert_eq!(g.add_label(c, "X"), Err(GraphError::Stale));

        let e = g.node(a).unwrap().out_edges()[0];
        assert_eq!(g.set_edge_type(e, Some("knows")).unwrap(), None);
        assert_eq!(g.set_edge_type(e, Some("likes")).unwrap(), Some("knows".into()));
        let likes = g.symbol("likes").unwrap();
        assert_eq!(g.edges_between(a, b, Some(likes)), [e]);
        assert!(g.edges_between(a, b, g.symbol("knows")).is_empty());
        assert_eq!(g.edges_between(a, b, None), [e]);
        assert_eq!(g.set_edge_type(e, None).unwrap(), Some("likes".into()));
    }

    #[test]
    fn subgraphs_keep_ids_labels_and_types() {
        let (mut g, [a, b, c]) = triangle();
        g.add_label(b, "B").unwrap();
        let e = g.insert_edge(a, b, None, Some("t"), 7).unwrap();
        let sub = g.induced_subgraph([a, b], |_| Ok::<_, ()>(()), |e| Ok(e.data)).unwrap();
        let (six, se) = (sub.node_ix("b").unwrap(), sub.edge_ix(g.edge(e).unwrap().id()).unwrap());
        assert_eq!(sub.label_names(six).unwrap(), ["B"]);
        assert_eq!(sub.edge_type_name(se), Some("t"));
        assert_eq!(sub.edge_count(), 2);
        assert_eq!(sub.next_edge_id(), g.next_edge_id());
        let _ = c;
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
