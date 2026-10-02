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

use crate::index::Indexes;
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

/// Hash map for string keys (node ids, symbol names): foldhash, several
/// times faster than the default SipHash on short keys, and seeded randomly
/// per process, so crafted ids can't force collisions.
pub type StrMap<K, V> = HashMap<K, V, foldhash::fast::RandomState>;
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

/// Bytes of a hashbrown table with room for `capacity` entries: buckets
/// (about 8/7 of the capacity) of `entry` bytes plus a control byte each.
pub(crate) fn hash_table_bytes(capacity: usize, entry: usize) -> usize {
    if capacity == 0 {
        0
    } else {
        (capacity * 8 / 7 + 1).next_power_of_two() * (entry + 1)
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
    index: StrMap<Box<str>, Symbol>,
    /// Bytes of the names, counted twice (list and index).
    name_bytes: usize,
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
        self.name_bytes += 2 * name.len();
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

    /// Store `value`; fails once `u32::MAX` slots are in use. `what` names
    /// the items for the error.
    fn insert(&mut self, value: T, what: &str) -> Result<(u32, u32), GraphError> {
        if let Some(slot) = self.free.pop() {
            let s = &mut self.slots[slot as usize];
            s.value = Some(value);
            self.len += 1;
            return Ok((slot, s.generation));
        }
        let slot = u32::try_from(self.slots.len())
            .ok()
            .filter(|&s| s < u32::MAX)
            .ok_or_else(|| GraphError::Capacity(format!("a graph holds at most {} {what}", u32::MAX - 1)))?;
        self.slots.push(Slot { generation: 0, value: Some(value) });
        self.len += 1;
        Ok((slot, 0))
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
        // A slot whose generations ran out is retired, so no old handle can
        // ever resolve to a new item
        if s.generation < u32::MAX {
            s.generation += 1;
            self.free.push(slot);
        }
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
#[derive(Debug)]
pub struct Graph<N, E> {
    nodes: Arena<Node<N>>,
    edges: Arena<Edge<E>>,
    index: StrMap<String, NodeIx>,
    edge_index: EdgeIndex,
    /// The id the next new edge gets.
    next_edge_id: u64,
    symbols: Symbols,
    /// Nodes carrying each label.
    labeled: HashMap<Symbol, IxSet<NodeIx>>,
    /// Property indexes (see `index.rs`).
    pub(crate) indexes: Indexes,
    /// Heap bytes that grow with the graph, kept up to date by every change
    /// so `memory_usage` is O(1): each live node's id, labels and adjacency
    /// lists, the id index's keys and the label sets.
    heap: usize,
}

impl<N: Clone, E: Clone> Clone for Graph<N, E> {
    fn clone(&self) -> Self {
        let mut g = Graph {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            index: self.index.clone(),
            edge_index: self.edge_index.clone(),
            next_edge_id: self.next_edge_id,
            symbols: self.symbols.clone(),
            labeled: self.labeled.clone(),
            indexes: self.indexes.clone(),
            heap: 0,
        };
        // Cloned strings and lists have other capacities
        g.recount();
        g
    }
}

/// Heap bytes of a node's id, labels and adjacency lists.
fn node_heap<N>(n: &Node<N>) -> usize {
    use std::mem::size_of;
    n.id.capacity()
        + n.labels.capacity() * size_of::<Symbol>()
        + (n.out.capacity() + n.inc.capacity()) * size_of::<EdgeIx>()
}

/// Heap bytes of a label's node set.
fn label_set_heap(set: &IxSet<NodeIx>) -> usize {
    hash_table_bytes(set.capacity(), std::mem::size_of::<NodeIx>())
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
            index: StrMap::with_capacity_and_hasher(nodes, Default::default()),
            edge_index: EdgeIndex { dense: Vec::with_capacity(edges), sparse: IxMap::default() },
            next_edge_id: 0,
            symbols: Symbols::default(),
            labeled: HashMap::new(),
            indexes: Indexes::default(),
            heap: 0,
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
        self.insert_node(id, data)
    }

    fn insert_node(&mut self, id: String, data: N) -> Result<NodeIx, GraphError> {
        let (slot, generation) = self
            .nodes
            .insert(Node { id: id.clone(), labels: Vec::new(), out: Vec::new(), inc: Vec::new(), data }, "nodes")?;
        let ix = NodeIx { slot, generation };
        self.heap += id.capacity() + node_heap(self.node_ref_mut(ix));
        self.index.insert(id, ix);
        self.indexes.touch(ix);
        Ok(ix)
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
        let (slot, generation) = self.edges.insert(Edge { from, to, id, ty, data }, "edges")?;
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
        let node = self.nodes.get_mut(ix.slot, ix.generation).ok_or(GraphError::Stale)?;
        let sym = self.symbols.intern(label);
        let labels = &mut node.labels;
        match labels.binary_search(&sym) {
            Ok(_) => Ok(false),
            Err(at) => {
                let before = labels.capacity();
                labels.insert(at, sym);
                let grown = (labels.capacity() - before) * std::mem::size_of::<Symbol>();
                let set = self.labeled.entry(sym).or_default();
                let before = label_set_heap(set);
                set.insert(ix);
                let grown = grown + label_set_heap(set) - before;
                self.heap += grown;
                Ok(true)
            }
        }
    }

    /// Remove a label from a node; returns whether it had it.
    pub fn remove_label(&mut self, ix: NodeIx, label: &str) -> Result<bool, GraphError> {
        let node = self.nodes.get_mut(ix.slot, ix.generation).ok_or(GraphError::Stale)?;
        let Some(sym) = self.symbols.get(label) else { return Ok(false) };
        let labels = &mut node.labels;
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

    /// Number of nodes carrying `label`.
    pub fn label_count(&self, label: &str) -> usize {
        self.symbols.get(label).and_then(|s| self.labeled.get(&s)).map_or(0, |set| set.len())
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
            // A hash set's capacity can change on removal too
            self.heap -= label_set_heap(set);
            set.remove(&ix);
            if set.is_empty() {
                self.labeled.remove(&sym);
            } else {
                self.heap += label_set_heap(set);
            }
        }
    }

    pub(crate) fn attach_out(&mut self, ix: EdgeIx) {
        let from = self.edge_ref(ix).from;
        let out = &mut self.node_ref_mut(from).out;
        let before = out.capacity();
        out.push(ix);
        self.heap += (out.capacity() - before) * std::mem::size_of::<EdgeIx>();
    }

    pub(crate) fn attach_in(&mut self, ix: EdgeIx) {
        let to = self.edge_ref(ix).to;
        let inc = &mut self.node_ref_mut(to).inc;
        let before = inc.capacity();
        inc.push(ix);
        self.heap += (inc.capacity() - before) * std::mem::size_of::<EdgeIx>();
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

    /// The node, mutably. Its payload may change, so property indexes
    /// re-check it until they are flushed (see [`Graph::flush_indexes`]).
    pub fn node_mut(&mut self, ix: NodeIx) -> Option<&mut Node<N>> {
        let node = self.nodes.get_mut(ix.slot, ix.generation)?;
        self.indexes.touch(ix);
        Some(node)
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
        self.nodes.get_mut(ix.slot, ix.generation).expect("adjacency lists reference live nodes")
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
        if self.index.get(&old) != Some(&ix) {
            return Err(not_indexed());
        }
        let (old_key, _) = self.index.remove_entry(&old).ok_or_else(not_indexed)?;
        let key = id.clone();
        self.heap = self.heap + key.capacity() + id.capacity() - old_key.capacity();
        self.index.insert(key, ix);
        let node = self.nodes.get_mut(ix.slot, ix.generation).ok_or_else(|| internal("a renamed node is gone"))?;
        let old_id = std::mem::replace(&mut node.id, id);
        self.heap -= old_id.capacity();
        Ok(())
    }

    /// Remove a node and every edge attached to it; returns its id and payload.
    ///
    /// # Panics
    ///
    /// Only if the graph's invariants are broken (a bug); [`Graph::apply`]
    /// reports that as [`GraphError::Internal`] instead.
    pub fn remove_node(&mut self, ix: NodeIx) -> Option<(String, N)> {
        self.try_remove_node(ix).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`Graph::remove_node`], with a broken invariant as
    /// [`GraphError::Internal`]. Everything is checked before anything
    /// changes, so on error the graph is unchanged.
    pub(crate) fn try_remove_node(&mut self, ix: NodeIx) -> Result<Option<(String, N)>, GraphError> {
        let Some(node) = self.node(ix) else { return Ok(None) };
        self.check_node(ix, node)?;
        let node =
            self.nodes.remove(ix.slot, ix.generation).ok_or_else(|| internal("a node just looked up is gone"))?;
        let (key, _) = self.index.remove_entry(&node.id).ok_or_else(not_indexed)?;
        self.heap -= key.capacity() + node_heap(&node);
        self.indexes.remove(ix);
        for &label in &node.labels {
            self.unindex_label(label, ix);
        }
        for &e in &node.out {
            // Self loops appear in both lists; the second visit finds nothing.
            if let Some(edge) = self.edges.remove(e.slot, e.generation) {
                self.edge_index.remove(edge.id);
                if let Some(to) = self.nodes.get_mut(edge.to.slot, edge.to.generation) {
                    to.inc.retain(|&x| x != e);
                }
            }
        }
        for &e in &node.inc {
            if let Some(edge) = self.edges.remove(e.slot, e.generation) {
                self.edge_index.remove(edge.id);
                if let Some(from) = self.nodes.get_mut(edge.from.slot, edge.from.generation) {
                    from.out.retain(|&x| x != e);
                }
            }
        }
        Ok(Some((node.id, node.data)))
    }

    /// Check the invariants removing a live node relies on: it is in the id
    /// index, and its edges and their endpoints are live.
    pub(crate) fn check_node(&self, ix: NodeIx, node: &Node<N>) -> Result<(), GraphError> {
        if self.index.get(&node.id) != Some(&ix) {
            return Err(not_indexed());
        }
        for &e in node.out.iter().chain(&node.inc) {
            let edge = self.edge(e).ok_or_else(|| internal("an adjacency list holds a removed edge"))?;
            if self.node(edge.from).is_none() || self.node(edge.to).is_none() {
                return Err(internal("an edge's endpoint is gone"));
            }
        }
        Ok(())
    }

    /// Remove one edge; returns it.
    ///
    /// # Panics
    ///
    /// Only if the graph's invariants are broken (a bug); [`Graph::apply`]
    /// reports that as [`GraphError::Internal`] instead.
    pub fn remove_edge(&mut self, ix: EdgeIx) -> Option<Edge<E>> {
        self.try_remove_edge(ix).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`Graph::remove_edge`], with a broken invariant as
    /// [`GraphError::Internal`]; on error the graph is unchanged.
    pub(crate) fn try_remove_edge(&mut self, ix: EdgeIx) -> Result<Option<Edge<E>>, GraphError> {
        let Some(edge) = self.edge(ix) else { return Ok(None) };
        if self.node(edge.from).is_none() || self.node(edge.to).is_none() {
            return Err(internal("an edge's endpoint is gone"));
        }
        let edge =
            self.edges.remove(ix.slot, ix.generation).ok_or_else(|| internal("an edge just looked up is gone"))?;
        self.edge_index.remove(edge.id);
        if let Some(from) = self.nodes.get_mut(edge.from.slot, edge.from.generation) {
            from.out.retain(|&x| x != ix);
        }
        if let Some(to) = self.nodes.get_mut(edge.to.slot, edge.to.generation) {
            to.inc.retain(|&x| x != ix);
        }
        Ok(Some(edge))
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

    /// Approximate bytes used by the graph's structure: node and edge slots
    /// (including the payloads' inline size), ids, adjacency lists, labels,
    /// the id, edge-id and label indexes and the property indexes. Memory
    /// that payloads own elsewhere (attribute maps, Python objects) is not
    /// counted.
    ///
    /// O(1) (O(number of property indexes)): the parts that grow with the
    /// graph are counted as it changes, so this is cheap enough to call on
    /// every write.
    pub fn memory_usage(&self) -> usize {
        self.fixed_memory() + self.heap + self.indexes.memory_usage()
    }

    /// The parts of `memory_usage` read off capacities.
    fn fixed_memory(&self) -> usize {
        use std::mem::size_of;
        let mut total = size_of::<Self>();
        total += self.nodes.slots.capacity() * size_of::<Slot<Node<N>>>() + self.nodes.free.capacity() * 4;
        total += self.edges.slots.capacity() * size_of::<Slot<Edge<E>>>() + self.edges.free.capacity() * 4;
        total += hash_table_bytes(self.index.capacity(), size_of::<(String, NodeIx)>());
        total += self.edge_index.dense.capacity() * 4;
        total += hash_table_bytes(self.edge_index.sparse.capacity(), size_of::<(EdgeId, u32)>());
        total += self.symbols.name_bytes;
        total += self.symbols.names.capacity() * size_of::<Box<str>>();
        total += hash_table_bytes(self.symbols.index.capacity(), size_of::<(Box<str>, Symbol)>());
        total + hash_table_bytes(self.labeled.capacity(), size_of::<(Symbol, IxSet<NodeIx>)>())
    }

    /// The counted heap bytes, recomputed (O(n)).
    fn count_heap(&self) -> usize {
        let nodes: usize = self.nodes().map(|(_, n)| node_heap(n)).sum();
        // The id index holds a second copy of every id
        let keys: usize = self.index.keys().map(String::capacity).sum();
        nodes + keys + self.labeled.values().map(label_set_heap).sum::<usize>()
    }

    /// Recompute the counters behind `memory_usage` (after bulk changes
    /// that bypass them).
    fn recount(&mut self) {
        self.heap = self.count_heap();
        self.indexes.recount();
    }

    /// All nodes, in slot order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeIx, &Node<N>)> + '_ {
        self.nodes.iter().map(|(slot, generation, n)| (NodeIx { slot, generation }, n))
    }

    /// Every node, mutably (property indexes re-check them all until
    /// flushed).
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = (NodeIx, &mut Node<N>)> + '_ {
        self.indexes.touch_all();
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
                let new =
                    out.insert_node(n.id.clone(), node_data(n)?).expect("a subgraph has no more nodes than its graph");
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
        // Labels and adjacency lists were set directly
        out.recount();
        Ok(out)
    }
}

fn internal(msg: &str) -> GraphError {
    GraphError::Internal(msg.to_owned())
}

fn not_indexed() -> GraphError {
    internal("a live node is missing from the id index")
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

    /// A graph whose node "a" is listed in the id index but carries
    /// another id: the invariant `apply` must not panic on.
    fn corrupted() -> Graph<crate::Record, crate::Record> {
        let mut g = Graph::new();
        let a = g.add_node("a", crate::Record::default()).unwrap();
        let b = g.add_node("b", crate::Record::default()).unwrap();
        g.add_edge(a, b, crate::Record::default()).unwrap();
        g.nodes.get_mut(a.slot, a.generation).unwrap().id = "x".into();
        g
    }

    #[test]
    fn apply_reports_a_broken_id_index_instead_of_panicking() {
        use crate::Op;
        let mut g = corrupted();
        let r = g.apply(Op::RenameNode { id: "a".into(), new_id: "c".into() });
        assert!(matches!(r, Err(GraphError::Internal(_))));
        assert!(g.contains_node("a") && !g.contains_node("c")); // unchanged
        let r = g.apply(Op::RemoveNode { id: "a".into() });
        assert!(matches!(r, Err(GraphError::Internal(_))));
        let ix = g.node_ix("a").unwrap();
        assert!(g.node(ix).is_some()); // unchanged, edge included
        assert_eq!(g.edge_count(), 1);
        assert!(matches!(g.try_remove_node(ix), Err(GraphError::Internal(_))));
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn apply_reports_a_dangling_edge_instead_of_panicking() {
        use crate::{Op, Record};
        let mut g: Graph<Record, Record> = Graph::new();
        let a = g.add_node("a", Record::default()).unwrap();
        let b = g.add_node("b", Record::default()).unwrap();
        let e = g.add_edge(a, b, Record::default()).unwrap();
        // Point the edge at a handle that never resolves
        g.edges.get_mut(e.slot, e.generation).unwrap().to = NodeIx { slot: 7, generation: 3 };
        let id = g.edge(e).unwrap().id();
        assert!(matches!(g.apply(Op::RemoveEdge { id }), Err(GraphError::Internal(_))));
        assert!(g.edge(e).is_some()); // unchanged
        assert!(matches!(g.apply(Op::RemoveNode { id: "a".into() }), Err(GraphError::Internal(_))));
        assert!(g.node(a).is_some());
        assert!(matches!(g.try_remove_node(a), Err(GraphError::Internal(_))));
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

    #[test]
    fn exhausted_slots_are_retired() {
        let mut g: Graph<(), ()> = Graph::new();
        let a = g.add_node("a", ()).unwrap();
        g.nodes.slots[a.slot()].generation = u32::MAX - 1;
        let a = NodeIx { slot: 0, generation: u32::MAX - 1 };
        g.index.insert("a".into(), a);
        g.remove_node(a).unwrap();
        let b = g.add_node("b", ()).unwrap();
        assert_eq!((b.slot(), b.generation), (0, u32::MAX));
        g.remove_node(b).unwrap();
        // Slot 0 can't take another generation: the next node goes elsewhere
        let c = g.add_node("c", ()).unwrap();
        assert_eq!(c.slot(), 1);
        assert!(g.node(b).is_none() && g.node(a).is_none());
    }

    #[test]
    fn memory_usage_grows_with_the_graph() {
        let (g, _) = triangle();
        let small = g.memory_usage();
        let mut big: Graph<(), u32> = Graph::new();
        let ix: Vec<NodeIx> = (0..1000).map(|i| big.add_node(format!("node{i}"), ()).unwrap()).collect();
        for w in ix.windows(2) {
            big.add_edge(w[0], w[1], 0).unwrap();
        }
        let m = big.memory_usage();
        assert!(small < m, "{small} {m}");
        // At least the ids (twice), slots and adjacency entries
        assert!(m > 1000 * (2 * 7 + 2 * std::mem::size_of::<EdgeIx>()), "{m}");
        assert!(m < 1000 * 400, "{m}");
    }

    /// The incremental counters equal a full recount.
    fn check_counters<N: Clone, E: Clone>(g: &Graph<N, E>) {
        assert_eq!(g.heap, g.count_heap());
        let mut fresh = g.indexes.clone();
        fresh.recount();
        for (a, b) in g.indexes.list_heaps().zip(fresh.list_heaps()) {
            assert_eq!(a, b);
        }
        assert_eq!(g.memory_usage(), g.fixed_memory() + g.count_heap() + g.indexes.memory_usage());
    }

    #[test]
    fn memory_counters_follow_every_change() {
        use crate::{Record, Value};
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut g: Graph<Record, Record> = Graph::new();
        g.create_index::<GraphError>(&["k".to_string()]).unwrap();
        let mut ids: Vec<String> = Vec::new();
        for step in 0..4000 {
            let live: Vec<NodeIx> = g.node_indices().collect();
            let pick = |rng: &mut rand::rngs::StdRng| live[rng.gen_range(0..live.len())];
            match rng.gen_range(0..10) {
                0..=2 => {
                    let id = format!("node-{step}-{}", "x".repeat(rng.gen_range(0..20)));
                    let mut id_with_room = String::with_capacity(id.len() + rng.gen_range(0..8));
                    id_with_room.push_str(&id);
                    let value = match rng.gen_range(0..3) {
                        0 => Value::from(rng.gen_range(0..5)),
                        1 => Value::from(format!("text{}", rng.gen_range(0..5))),
                        _ => Value::None,
                    };
                    g.add_node(id_with_room, Record::with_attr([("k", value)])).unwrap();
                    ids.push(id);
                }
                3 | 4 if !live.is_empty() => {
                    let (a, b) = (pick(&mut rng), pick(&mut rng));
                    g.add_edge(a, b, Record::default()).unwrap();
                }
                5 if !live.is_empty() => {
                    let a = pick(&mut rng);
                    let label = ["A", "B", "C", "D"][rng.gen_range(0..4)];
                    if rng.gen_bool(0.6) {
                        g.add_label(a, label).unwrap();
                    } else {
                        g.remove_label(a, label).unwrap();
                    }
                }
                6 if !live.is_empty() => {
                    let a = pick(&mut rng);
                    g.remove_node(a).unwrap();
                }
                7 if g.edge_count() > 0 => {
                    let edges: Vec<EdgeIx> = g.edges().map(|(e, _)| e).collect();
                    g.remove_edge(edges[rng.gen_range(0..edges.len())]).unwrap();
                }
                8 if !live.is_empty() => {
                    let a = pick(&mut rng);
                    let _ = g.rename_node(a, format!("renamed-{step}"));
                }
                _ if !live.is_empty() => {
                    let a = pick(&mut rng);
                    g.node_mut(a).unwrap().data.attr.insert("k".into(), Value::from(format!("v{}", step % 7)));
                    g.flush_indexes().unwrap();
                }
                _ => {}
            }
            assert_eq!(g.heap, g.count_heap(), "step {step}");
            if step % 50 == 0 {
                check_counters(&g);
                let copy = g.clone();
                check_counters(&copy);
                let half: Vec<NodeIx> = g.node_indices().step_by(2).collect();
                let sub =
                    g.induced_subgraph(half, |n| Ok::<_, GraphError>(n.data.clone()), |e| Ok(e.data.clone())).unwrap();
                check_counters(&sub);
            }
        }
        check_counters(&g);
        assert!(g.node_count() > 100);
    }
}
