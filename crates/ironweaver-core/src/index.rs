// index.rs
//
// Property indexes on node attributes: for an attribute path, a sorted map
// from value (`Key`) to the nodes holding it, for equality and range
// lookups, and for picking candidates for filter expressions and pattern
// nodes.
//
// The graph keeps its indexes consistent with its structure (removed nodes
// leave them), but it can't see payload changes: `add_node`, `node_mut` and
// `nodes_mut` hand out payloads that may change, so they mark the node (or
// every node) dirty. Lookups stay exact meanwhile: they read the dirty
// nodes' current values instead of trusting the index. `flush_indexes` (or
// `reindex_node`, or `set_index_keys` for callers that compute keys
// themselves) brings the index up to date and clears the marks; do it after
// a batch of changes to keep lookups fast.
//
// An index can also be built off the graph (`begin_index_build`, then
// `IndexBuild::read` / `insert` with only `&Graph`, then `install_index`):
// while a build is open the graph records which nodes change, and
// installing only looks at those (they are marked dirty), so the `&mut`
// steps don't depend on the graph's size.
//
// Only scalar values are indexed (see `Key`): a node whose value at the
// path is missing, none, a list, a dict or NaN is not in the index, which
// matches `Expr` semantics (such values never compare equal or ordered).

use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use crate::graph::{hash_table_bytes, IxMap, IxSet};
use crate::{Attributes, CmpOp, Expr, Graph, GraphError, Key, NodeIx, Value};

/// The nodes under one key: usually one (a unique property), sometimes very
/// many (a category).
#[derive(Clone, Debug)]
enum Posting {
    One(NodeIx),
    Many(IxSet<NodeIx>),
}

impl Posting {
    fn insert(&mut self, ix: NodeIx) {
        match self {
            Posting::One(x) if *x == ix => {}
            Posting::One(x) => *self = Posting::Many([*x, ix].into_iter().collect()),
            Posting::Many(set) => {
                set.insert(ix);
            }
        }
    }

    /// Removes `ix`; true if nothing is left.
    fn remove(&mut self, ix: NodeIx) -> bool {
        match self {
            Posting::One(x) => *x == ix,
            Posting::Many(set) => {
                set.remove(&ix);
                if set.len() == 1 {
                    *self = Posting::One(*set.iter().next().expect("one left"));
                }
                set_is_empty(self)
            }
        }
    }

    fn for_each(&self, mut f: impl FnMut(NodeIx)) {
        match self {
            Posting::One(x) => f(*x),
            Posting::Many(set) => set.iter().for_each(|&x| f(x)),
        }
    }
}

fn set_is_empty(p: &Posting) -> bool {
    matches!(p, Posting::Many(set) if set.is_empty())
}

/// Heap bytes of a key (its text or bytes).
fn key_heap(k: &Key) -> usize {
    match k {
        Key::String(s) => s.capacity(),
        Key::Bytes(b) => b.capacity(),
        _ => 0,
    }
}

/// Heap bytes of a posting's node set.
fn posting_heap(p: &Posting) -> usize {
    match p {
        Posting::One(_) => 0,
        Posting::Many(set) => hash_table_bytes(set.capacity(), std::mem::size_of::<NodeIx>()),
    }
}

/// One property index.
#[derive(Clone, Debug)]
struct PropertyIndex {
    path: Vec<String>,
    map: BTreeMap<Key, Posting>,
    /// Each indexed node's key, to remove it without reading its payload.
    keys: IxMap<NodeIx, Key>,
    /// Heap bytes of the keys (in `map` and `keys`) and posting sets, kept
    /// up to date by `set` / `unset`.
    heap: usize,
}

impl PropertyIndex {
    fn new(path: &[String]) -> Self {
        PropertyIndex { path: path.to_vec(), map: BTreeMap::new(), keys: IxMap::default(), heap: 0 }
    }

    fn set(&mut self, ix: NodeIx, key: Option<Key>) {
        if let Some(old) = self.keys.get(&ix) {
            if key.as_ref() == Some(old) {
                return;
            }
            self.unset(ix);
        }
        if let Some(k) = key {
            match self.map.get_mut(&k) {
                Some(p) => {
                    let before = posting_heap(p);
                    p.insert(ix);
                    self.heap = self.heap + posting_heap(p) - before;
                }
                None => {
                    let copy = k.clone();
                    self.heap += key_heap(&copy);
                    self.map.insert(copy, Posting::One(ix));
                }
            }
            self.heap += key_heap(&k);
            self.keys.insert(ix, k);
        }
    }

    fn unset(&mut self, ix: NodeIx) {
        if let Some(old) = self.keys.remove(&ix) {
            self.heap -= key_heap(&old);
            let p = self.map.get_mut(&old).expect("indexed keys have postings");
            let before = posting_heap(p);
            let empty = p.remove(ix);
            self.heap = self.heap + posting_heap(p) - before;
            if empty {
                let (key, p) = self.map.remove_entry(&old).expect("just found");
                self.heap -= key_heap(&key) + posting_heap(&p);
            }
        }
    }

    /// The heap bytes, recomputed.
    fn count_heap(&self) -> usize {
        let map: usize = self.map.iter().map(|(k, p)| key_heap(k) + posting_heap(p)).sum();
        map + self.keys.values().map(key_heap).sum::<usize>()
    }
}

/// The nodes changed (added, touched or removed) since an index build
/// began.
#[derive(Debug)]
struct Tracker {
    build: u64,
    /// Gone once the `IndexBuild` is dropped: the tracker is then pruned.
    alive: Weak<()>,
    changed: IxSet<NodeIx>,
    /// Every node may have changed (after `nodes_mut`).
    all: bool,
}

/// Ids of index builds, unique in the process (so a build is never found
/// in another graph).
static NEXT_BUILD: AtomicU64 = AtomicU64::new(0);

/// The property indexes of a graph, and which nodes they may be stale for.
#[derive(Debug, Default)]
pub(crate) struct Indexes {
    list: Vec<PropertyIndex>,
    dirty: IxSet<NodeIx>,
    /// Every node may be stale (after `nodes_mut`).
    all_dirty: bool,
    /// Open index builds (see `begin_index_build`).
    builds: Vec<Tracker>,
}

/// A clone has no open builds: they belong to the original graph.
impl Clone for Indexes {
    fn clone(&self) -> Self {
        Indexes { list: self.list.clone(), dirty: self.dirty.clone(), all_dirty: self.all_dirty, builds: Vec::new() }
    }
}

impl Indexes {
    /// A node's payload may change (or the node was added).
    #[inline]
    pub(crate) fn touch(&mut self, ix: NodeIx) {
        self.mark_dirty(ix);
        if !self.builds.is_empty() {
            self.track(ix);
        }
    }

    /// Every payload may change.
    pub(crate) fn touch_all(&mut self) {
        if !self.list.is_empty() {
            self.all_dirty = true;
            self.dirty.clear();
        }
        self.builds.retain(|t| t.alive.strong_count() > 0);
        for t in &mut self.builds {
            t.all = true;
            t.changed = IxSet::default();
        }
    }

    #[inline]
    fn mark_dirty(&mut self, ix: NodeIx) {
        if !self.list.is_empty() && !self.all_dirty {
            self.dirty.insert(ix);
        }
    }

    fn track(&mut self, ix: NodeIx) {
        self.builds.retain_mut(|t| {
            if !t.all {
                t.changed.insert(ix);
            }
            t.alive.strong_count() > 0
        });
    }

    /// Approximate bytes used (see `Graph::memory_usage`).
    /// O(number of indexes): the keys' and postings' heap bytes are
    /// counted as they change.
    pub(crate) fn memory_usage(&self) -> usize {
        use std::mem::size_of;
        let mut total = hash_table_bytes(self.dirty.capacity(), size_of::<NodeIx>());
        for t in &self.builds {
            total += hash_table_bytes(t.changed.capacity(), size_of::<NodeIx>());
        }
        for index in &self.list {
            // B-tree nodes hold up to 11 entries; assume two thirds full
            total += index.map.len() * (size_of::<Key>() + size_of::<Posting>()) * 3 / 2;
            total += index.heap;
            total += hash_table_bytes(index.keys.capacity(), size_of::<(NodeIx, Key)>());
        }
        total
    }

    #[cfg(test)]
    pub(crate) fn list_heaps(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.list.iter().map(|i| (i.heap, i.count_heap()))
    }

    /// Recompute the heap counters (after a clone: copies of strings have
    /// other capacities).
    pub(crate) fn recount(&mut self) {
        for index in &mut self.list {
            index.heap = index.count_heap();
        }
    }

    pub(crate) fn remove(&mut self, ix: NodeIx) {
        if !self.builds.is_empty() {
            self.track(ix);
        }
        if self.list.is_empty() {
            return;
        }
        self.dirty.remove(&ix);
        for index in &mut self.list {
            index.unset(ix);
        }
    }

    fn position(&self, path: &[String]) -> Option<usize> {
        self.list.iter().position(|i| i.path == path)
    }
}

/// The index key of a node's value at `path`.
fn key_of<N: Attributes>(data: &N, path: &[String]) -> Result<Option<Key>, N::Error> {
    data.with_value(path, |v| v.and_then(Key::of))
}

fn check_path(path: &[String]) -> Result<(), GraphError> {
    if path.is_empty() || path.iter().any(String::is_empty) {
        return Err(GraphError::InvalidArgument("index path must not be empty".into()));
    }
    if path.len() == 1 && path[0] == "labels" {
        return Err(GraphError::InvalidArgument(
            "\"labels\" is the node's labels, which are always indexed; use nodes_with_label".into(),
        ));
    }
    Ok(())
}

/// A validated key range: bounds of one kind (`kind`).
struct Range {
    lo: Bound<Key>,
    hi: Bound<Key>,
    kind: Key,
    /// Nothing can lie within (lo above hi); `BTreeMap::range` would panic.
    empty: bool,
}

impl Range {
    fn new(lo: Bound<&Value>, hi: Bound<&Value>) -> Result<Range, GraphError> {
        let key = |v: &Value| {
            Key::of(v).ok_or_else(|| {
                GraphError::InvalidArgument(format!("range bounds must be scalar values (not NaN), got {v:?}"))
            })
        };
        let bound = |b: Bound<&Value>| -> Result<Bound<Key>, GraphError> {
            Ok(match b {
                Bound::Included(v) => Bound::Included(key(v)?),
                Bound::Excluded(v) => Bound::Excluded(key(v)?),
                Bound::Unbounded => Bound::Unbounded,
            })
        };
        let (lo, hi) = (bound(lo)?, bound(hi)?);
        let value = |b: &Bound<Key>| match b {
            Bound::Included(k) | Bound::Excluded(k) => Some(k.clone()),
            Bound::Unbounded => None,
        };
        let (a, b) = (value(&lo), value(&hi));
        let kind = match (&a, &b) {
            (Some(a), Some(b)) if !a.same_kind(b) => {
                return Err(GraphError::InvalidArgument("range bounds must be of the same kind".into()))
            }
            (Some(k), _) | (_, Some(k)) => k.clone(),
            (None, None) => return Err(GraphError::InvalidArgument("a range needs at least one bound".into())),
        };
        let empty = match (a, b) {
            (Some(a), Some(b)) => a > b || (a == b && !matches!((&lo, &hi), (Bound::Included(_), Bound::Included(_)))),
            _ => false,
        };
        Ok(Range { lo, hi, kind, empty })
    }

    fn contains(&self, k: &Key) -> bool {
        let above = match &self.lo {
            Bound::Included(b) => k >= b,
            Bound::Excluded(b) => k > b,
            Bound::Unbounded => true,
        };
        let below = match &self.hi {
            Bound::Included(b) => k <= b,
            Bound::Excluded(b) => k < b,
            Bound::Unbounded => true,
        };
        !self.empty && k.same_kind(&self.kind) && above && below
    }
}

impl<N, E> Graph<N, E> {
    /// Paths of the node property indexes, in creation order.
    pub fn index_paths(&self) -> Vec<&[String]> {
        self.indexes.list.iter().map(|i| i.path.as_slice()).collect()
    }

    /// Whether there is an index on `path`.
    pub fn has_index(&self, path: &[String]) -> bool {
        self.indexes.position(path).is_some()
    }

    /// Create an index on `path` from keys the caller read (for payloads
    /// that shouldn't be read while the graph is borrowed mutably). Live
    /// nodes missing from `keys` are marked dirty; stale handles are
    /// skipped. Returns false if the index exists already.
    pub fn create_index_with_keys(
        &mut self,
        path: &[String],
        keys: impl IntoIterator<Item = (NodeIx, Option<Key>)>,
    ) -> Result<bool, GraphError> {
        check_path(path)?;
        if self.has_index(path) {
            return Ok(false);
        }
        let first = self.indexes.list.is_empty();
        let mut index = PropertyIndex::new(path);
        let mut seen = IxSet::default();
        for (ix, key) in keys {
            if self.node(ix).is_some() {
                index.set(ix, key);
                seen.insert(ix);
            }
        }
        if first {
            self.indexes.dirty.clear();
            self.indexes.all_dirty = false;
        }
        self.indexes.list.push(index);
        if seen.len() < self.node_count() {
            let missing: Vec<NodeIx> = self.node_indices().filter(|ix| !seen.contains(ix)).collect();
            for ix in missing {
                self.indexes.touch(ix);
            }
        }
        Ok(true)
    }

    /// Drop the index on `path`; returns whether there was one.
    pub fn drop_index(&mut self, path: &[String]) -> bool {
        let Some(at) = self.indexes.position(path) else { return false };
        self.indexes.list.remove(at);
        if self.indexes.list.is_empty() {
            let builds = std::mem::take(&mut self.indexes.builds);
            self.indexes = Indexes { builds, ..Indexes::default() };
        }
        true
    }

    /// Set a node's keys, one per index in [`index_paths`](Self::index_paths)
    /// order (`None`: not indexed), and mark it up to date. For callers
    /// that read payloads themselves; a stale `ix` is ignored.
    pub fn set_index_keys(&mut self, ix: NodeIx, keys: Vec<Option<Key>>) -> Result<(), GraphError> {
        if keys.len() != self.indexes.list.len() {
            return Err(GraphError::InvalidArgument(format!(
                "expected {} index keys, got {}",
                self.indexes.list.len(),
                keys.len()
            )));
        }
        if self.node(ix).is_none() {
            return Ok(());
        }
        for (index, key) in self.indexes.list.iter_mut().zip(keys) {
            index.set(ix, key);
        }
        self.indexes.dirty.remove(&ix);
        Ok(())
    }

    /// Whether some node's index entries may be stale (see the module
    /// comment); `flush_indexes` clears it.
    pub fn indexes_dirty(&self) -> bool {
        self.indexes.all_dirty || !self.indexes.dirty.is_empty()
    }

    /// Nodes whose index entries may be stale, in slot order.
    pub fn dirty_nodes(&self) -> Vec<NodeIx> {
        let mut out: Vec<NodeIx> = if self.indexes.all_dirty {
            self.node_indices().collect()
        } else {
            self.indexes.dirty.iter().copied().collect()
        };
        out.sort_unstable_by_key(|ix| ix.slot());
        out
    }

    /// Start building an index on `path` off the graph: O(1). Until the
    /// build is installed or cancelled the graph records which nodes
    /// change. Fill the build with [`IndexBuild::read`] or
    /// [`IndexBuild::insert`], which only borrow the graph shared (so they
    /// can run under a read lock, in chunks), then install it with
    /// [`install_index`](Self::install_index). Fails if `path` is invalid
    /// or indexed already.
    pub fn begin_index_build(&mut self, path: &[String]) -> Result<IndexBuild, GraphError> {
        check_path(path)?;
        if self.has_index(path) {
            return Err(GraphError::InvalidArgument(format!("there is an index on {path:?} already")));
        }
        let id = NEXT_BUILD.fetch_add(1, Ordering::Relaxed);
        let token = Arc::new(());
        self.indexes.builds.retain(|t| t.alive.strong_count() > 0);
        self.indexes.builds.push(Tracker {
            build: id,
            alive: Arc::downgrade(&token),
            changed: IxSet::default(),
            all: false,
        });
        Ok(IndexBuild { id, _token: token, index: PropertyIndex::new(path), unkeyed: IxSet::default() })
    }

    /// Install a build begun on this graph as the index on its path, in
    /// O(nodes changed since [`begin_index_build`](Self::begin_index_build)):
    /// removed nodes leave it, and changed or added nodes are marked dirty
    /// (lookups re-read them until [`flush_indexes`](Self::flush_indexes)).
    /// Live nodes the build didn't read are marked dirty too, which costs
    /// a pass over the nodes; so does a `nodes_mut` during the build (every
    /// node is then dirty). Returns false (and drops the build) if the path
    /// was indexed meanwhile; fails if the build wasn't begun on this graph
    /// or was installed or cancelled already.
    pub fn install_index(&mut self, build: IndexBuild) -> Result<bool, GraphError> {
        let tracker = self.take_tracker(build.id)?;
        let IndexBuild { mut index, mut unkeyed, .. } = build;
        if self.has_index(&index.path) {
            return Ok(false);
        }
        // Read nodes that were removed later leave the index; then every
        // read node is live
        let changed: Vec<NodeIx> = if tracker.all {
            let dead: Vec<NodeIx> = index.keys.keys().copied().filter(|&ix| self.node(ix).is_none()).collect();
            for ix in dead {
                index.unset(ix);
            }
            unkeyed.retain(|&ix| self.node(ix).is_some());
            Vec::new()
        } else {
            let mut live = Vec::with_capacity(tracker.changed.len());
            for ix in tracker.changed {
                if self.node(ix).is_some() {
                    live.push(ix);
                } else {
                    index.unset(ix);
                    unkeyed.remove(&ix);
                }
            }
            live
        };
        // Every live node must be read or changed (and so re-read)
        let read = |ix: &NodeIx| index.keys.contains_key(ix) || unkeyed.contains(ix);
        let covered = index.keys.len() + unkeyed.len() + changed.iter().filter(|ix| !read(ix)).count();
        let missing: Vec<NodeIx> = if tracker.all || covered == self.node_count() {
            Vec::new()
        } else {
            self.node_indices().filter(|ix| !read(ix)).collect()
        };
        if self.indexes.list.is_empty() {
            // Nothing was tracked before the first index
            self.indexes.dirty.clear();
            self.indexes.all_dirty = false;
        }
        self.indexes.list.push(index);
        if tracker.all {
            self.indexes.all_dirty = true;
            self.indexes.dirty.clear();
        }
        for ix in changed.into_iter().chain(missing) {
            self.indexes.mark_dirty(ix);
        }
        Ok(true)
    }

    /// Drop a build without installing it, so the graph stops recording
    /// changes for it; returns whether it was open on this graph. (Just
    /// dropping it works too: the graph stops at its next change.)
    pub fn cancel_index_build(&mut self, build: IndexBuild) -> bool {
        self.take_tracker(build.id).is_ok()
    }

    /// Number of index builds begun on this graph and not yet installed,
    /// cancelled or dropped.
    pub fn open_index_builds(&self) -> usize {
        self.indexes.builds.iter().filter(|t| t.alive.strong_count() > 0).count()
    }

    fn take_tracker(&mut self, build: u64) -> Result<Tracker, GraphError> {
        let at = self.indexes.builds.iter().position(|t| t.build == build).ok_or_else(foreign_build)?;
        Ok(self.indexes.builds.swap_remove(at))
    }
}

fn foreign_build() -> GraphError {
    GraphError::InvalidArgument("the index build is not open on this graph".into())
}

/// An index being built off the graph: see [`Graph::begin_index_build`].
/// It owns the index under construction and doesn't borrow the graph, so
/// it can be filled between (and outside) lock holds, and moved across
/// threads.
#[derive(Debug)]
pub struct IndexBuild {
    id: u64,
    /// Lets the graph see that the build was dropped.
    _token: Arc<()>,
    index: PropertyIndex,
    /// Nodes read that have no key (the others are in `index.keys`).
    unkeyed: IxSet<NodeIx>,
}

impl IndexBuild {
    /// The indexed path.
    pub fn path(&self) -> &[String] {
        &self.index.path
    }

    /// Number of nodes read so far (with a key or without).
    pub fn len(&self) -> usize {
        self.index.keys.len() + self.unkeyed.len()
    }

    /// Whether no node was read yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Record a node's key, read by the caller (`None`: the node has no
    /// indexable value). `graph` must be the graph the build was begun on;
    /// a stale `ix` is ignored. Reading a node again replaces its key.
    pub fn insert<N, E>(&mut self, graph: &Graph<N, E>, ix: NodeIx, key: Option<Key>) -> Result<(), GraphError> {
        self.check_graph(graph)?;
        self.put(graph, ix, key);
        Ok(())
    }

    fn check_graph<N, E>(&self, graph: &Graph<N, E>) -> Result<(), GraphError> {
        if graph.indexes.builds.iter().any(|t| t.build == self.id) {
            Ok(())
        } else {
            Err(foreign_build())
        }
    }

    fn put<N, E>(&mut self, graph: &Graph<N, E>, ix: NodeIx, key: Option<Key>) {
        if graph.node(ix).is_none() {
            return;
        }
        if key.is_some() {
            self.unkeyed.remove(&ix);
        } else {
            self.unkeyed.insert(ix);
        }
        self.index.set(ix, key);
    }
}

impl<N: Attributes, E> Graph<N, E> {
    /// Index node values at `path` (an attribute name, then keys into nested
    /// dicts); returns false if the index exists already.
    pub fn create_index<X>(&mut self, path: &[String]) -> Result<bool, X>
    where
        X: From<GraphError> + From<N::Error>,
    {
        check_path(path)?;
        if self.has_index(path) {
            return Ok(false);
        }
        let mut index = PropertyIndex::new(path);
        for (ix, node) in self.nodes() {
            index.set(ix, key_of(&node.data, path)?);
        }
        if self.indexes.list.is_empty() {
            // Nothing was tracked before the first index
            self.indexes.dirty.clear();
            self.indexes.all_dirty = false;
        }
        self.indexes.list.push(index);
        Ok(true)
    }

    /// Re-read a node's indexed values and mark it up to date.
    pub fn reindex_node(&mut self, ix: NodeIx) -> Result<(), N::Error> {
        let Some(node) = self.node(ix) else { return Ok(()) };
        let mut keys = Vec::with_capacity(self.indexes.list.len());
        for index in &self.indexes.list {
            keys.push(key_of(&node.data, &index.path)?);
        }
        self.set_index_keys(ix, keys).expect("one key per index");
        Ok(())
    }

    /// Bring every index up to date.
    pub fn flush_indexes(&mut self) -> Result<(), N::Error> {
        for ix in self.dirty_nodes() {
            self.reindex_node(ix)?;
        }
        self.indexes.all_dirty = false;
        self.indexes.dirty.clear();
        Ok(())
    }

    /// Nodes whose value at `path` equals `value` (numbers across int and
    /// float), in slot order; `None` if `path` has no index. A non-scalar
    /// `value` finds nothing.
    pub fn find_nodes(&self, path: &[String], value: &Value) -> Result<Option<Vec<NodeIx>>, N::Error> {
        let Some(at) = self.indexes.position(path) else { return Ok(None) };
        let Some(key) = Key::of(value) else { return Ok(Some(Vec::new())) };
        self.lookup(
            at,
            |k| *k == key,
            |map, out| {
                if let Some(p) = map.get(&key) {
                    p.for_each(|ix| out.push(ix));
                }
            },
        )
        .map(Some)
    }

    /// Nodes whose value at `path` lies between the bounds (which must be
    /// scalars of the same kind: numbers, strings, dates, ...; values of
    /// other kinds are never in range), in slot order; `None` if `path` has
    /// no index.
    pub fn find_nodes_in_range<X>(
        &self,
        path: &[String],
        lo: Bound<&Value>,
        hi: Bound<&Value>,
    ) -> Result<Option<Vec<NodeIx>>, X>
    where
        X: From<GraphError> + From<N::Error>,
    {
        let Some(at) = self.indexes.position(path) else { return Ok(None) };
        let range = Range::new(lo, hi)?;
        Ok(Some(self.range_lookup(at, &range)?))
    }

    fn range_lookup(&self, at: usize, r: &Range) -> Result<Vec<NodeIx>, N::Error> {
        if r.empty {
            return Ok(Vec::new());
        }
        self.lookup(
            at,
            |k| r.contains(k),
            |map, out| {
                // Keys of one kind are contiguous; other kinds at the open ends
                // are skipped
                for (k, p) in map.range((r.lo.clone(), r.hi.clone())) {
                    if k.same_kind(&r.kind) {
                        p.for_each(|ix| out.push(ix));
                    }
                }
            },
        )
    }

    /// Nodes found by `collect` in index `at`, corrected for dirty nodes
    /// (whose current key must satisfy `fits`), in slot order.
    fn lookup(
        &self,
        at: usize,
        fits: impl Fn(&Key) -> bool,
        collect: impl FnOnce(&BTreeMap<Key, Posting>, &mut Vec<NodeIx>),
    ) -> Result<Vec<NodeIx>, N::Error> {
        let index = &self.indexes.list[at];
        let mut out = Vec::new();
        if self.indexes.all_dirty {
            for (ix, node) in self.nodes() {
                if key_of(&node.data, &index.path)?.is_some_and(|k| fits(&k)) {
                    out.push(ix);
                }
            }
            return Ok(out);
        }
        collect(&index.map, &mut out);
        if !self.indexes.dirty.is_empty() {
            out.retain(|ix| !self.indexes.dirty.contains(ix));
            for &ix in &self.indexes.dirty {
                if let Some(node) = self.node(ix) {
                    if key_of(&node.data, &index.path)?.is_some_and(|k| fits(&k)) {
                        out.push(ix);
                    }
                }
            }
        }
        out.sort_unstable_by_key(|ix| ix.slot());
        Ok(out)
    }

    /// A superset of the nodes matching `expr` found through indexes (and
    /// the label index), in slot order; `None` if the indexes can't narrow
    /// it down. Check each candidate with `expr.matches_node`.
    pub fn index_candidates(&self, expr: &Expr) -> Result<Option<Vec<NodeIx>>, N::Error> {
        Ok(match expr {
            Expr::Const(false) => Some(Vec::new()),
            Expr::Label(name) => Some(self.nodes_with_label(name)),
            Expr::Compare { path, op, value } if self.has_index(path) => {
                let range = match op {
                    CmpOp::Eq => return self.find_nodes(path, value),
                    CmpOp::Ne => return Ok(None),
                    CmpOp::Lt => (Bound::Unbounded, Bound::Excluded(value)),
                    CmpOp::Le => (Bound::Unbounded, Bound::Included(value)),
                    CmpOp::Gt => (Bound::Excluded(value), Bound::Unbounded),
                    CmpOp::Ge => (Bound::Included(value), Bound::Unbounded),
                };
                match Range::new(range.0, range.1) {
                    Ok(r) => Some(self.range_lookup(self.indexes.position(path).expect("indexed"), &r)?),
                    // Comparisons with non-scalars (or NaN) are never true
                    Err(_) => Some(Vec::new()),
                }
            }
            Expr::In { path, values } if self.has_index(path) => {
                let mut all = Vec::new();
                for v in values {
                    all.extend(self.find_nodes(path, v)?.expect("indexed"));
                }
                Some(sorted_unique(all))
            }
            Expr::And(items) => {
                // The smallest narrowed-down set (the rest is checked later)
                let mut best: Option<Vec<NodeIx>> = None;
                // A lower and an upper bound on one indexed path: one range
                let mut combined: Vec<&[String]> = Vec::new();
                for a in items {
                    let Expr::Compare { path, op: lo_op @ (CmpOp::Gt | CmpOp::Ge), value: lo } = a else { continue };
                    let Some(at) = self.indexes.position(path) else { continue };
                    for b in items {
                        let Expr::Compare { path: p, op: hi_op @ (CmpOp::Lt | CmpOp::Le), value: hi } = b else {
                            continue;
                        };
                        if p != path {
                            continue;
                        }
                        let lo = if *lo_op == CmpOp::Ge { Bound::Included(lo) } else { Bound::Excluded(lo) };
                        let hi = if *hi_op == CmpOp::Le { Bound::Included(hi) } else { Bound::Excluded(hi) };
                        let c = match Range::new(lo, hi) {
                            Ok(r) => self.range_lookup(at, &r)?,
                            // Bounds of different kinds (or NaN): nothing lies between
                            Err(_) => Vec::new(),
                        };
                        combined.push(path);
                        if best.as_ref().is_none_or(|b| c.len() < b.len()) {
                            best = Some(c);
                        }
                    }
                }
                // Then point lookups and the rest; open-ended ranges (often
                // most of the graph) only if nothing narrower was found
                let open_range =
                    |e: &Expr| matches!(e, Expr::Compare { op: CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge, .. });
                for pass in [false, true] {
                    if pass && best.is_some() {
                        break;
                    }
                    for item in items.iter().filter(|e| open_range(e) == pass) {
                        if let Expr::Compare { path, .. } = item {
                            if combined.contains(&path.as_slice()) {
                                continue;
                            }
                        }
                        if let Some(c) = self.index_candidates(item)? {
                            if best.as_ref().is_none_or(|b| c.len() < b.len()) {
                                best = Some(c);
                            }
                        }
                    }
                }
                best
            }
            Expr::Or(items) => {
                let mut all = Vec::new();
                for item in items {
                    match self.index_candidates(item)? {
                        Some(c) => all.extend(c),
                        None => return Ok(None),
                    }
                }
                Some(sorted_unique(all))
            }
            _ => None,
        })
    }
}

impl IndexBuild {
    /// Read the keys of `nodes` from `graph` (the graph the build was begun
    /// on), skipping stale handles. Call it once with
    /// [`Graph::node_indices`], or in chunks between lock holds; nodes
    /// changed in between are re-read when the build is installed.
    pub fn read<N: Attributes, E, X>(
        &mut self,
        graph: &Graph<N, E>,
        nodes: impl IntoIterator<Item = NodeIx>,
    ) -> Result<(), X>
    where
        X: From<GraphError> + From<N::Error>,
    {
        self.check_graph(graph)?;
        for ix in nodes {
            if let Some(node) = graph.node(ix) {
                let key = key_of(&node.data, &self.index.path)?;
                self.put(graph, ix, key);
            }
        }
        Ok(())
    }
}

fn sorted_unique(mut v: Vec<NodeIx>) -> Vec<NodeIx> {
    v.sort_unstable_by_key(|ix| ix.slot());
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Date, Record};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    type G = Graph<Record, Record>;

    fn p(s: &str) -> Vec<String> {
        s.split('.').map(str::to_owned).collect()
    }

    fn random_value(rng: &mut StdRng) -> Option<Value> {
        Some(match rng.gen_range(0..9) {
            0 => return None,
            1 => Value::Int(rng.gen_range(-3..4)),
            2 => Value::Float(rng.gen_range(-6..8) as f64 / 2.0),
            3 => Value::from(["a", "b", "c"][rng.gen_range(0..3)]),
            4 => Value::Bool(rng.gen()),
            5 => Value::Date(Date(rng.gen_range(0..4))),
            6 => Value::List(vec![Value::Int(1)]),
            7 => Value::None,
            _ => Value::Float(f64::NAN),
        })
    }

    fn set(g: &mut G, ix: NodeIx, rng: &mut StdRng) {
        let attr = &mut g.node_mut(ix).unwrap().data.attr;
        match random_value(rng) {
            Some(v) => attr.insert("x".into(), v),
            None => attr.remove("x"),
        };
    }

    /// Every lookup agrees with evaluating the expression on every node.
    fn check(g: &G) {
        let probes = [Value::Int(1), Value::Float(1.0), Value::Float(1.5), Value::from("b"), Value::Bool(true)];
        let mut exprs = vec![Expr::Label("L".into())];
        for v in probes.iter().chain([&Value::Date(Date(2)), &Value::Float(f64::NAN), &Value::None]) {
            for op in [CmpOp::Eq, CmpOp::Ne, CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge] {
                exprs.push(Expr::Compare { path: p("x"), op, value: v.clone() });
            }
        }
        exprs.push(Expr::In { path: p("x"), values: probes.to_vec() });
        let n = exprs.len();
        exprs.push(Expr::And(vec![exprs[3].clone(), Expr::Label("L".into())]));
        exprs.push(Expr::Or(vec![exprs[1].clone(), exprs[n - 1].clone()]));
        exprs.push(Expr::Or(vec![exprs[1].clone(), Expr::Exists { path: p("x") }]));
        for (lo, hi) in [
            (Value::Int(-1), Value::Float(1.5)),
            (Value::from("a"), Value::from("b")),
            (Value::Int(0), Value::from("z")),
        ] {
            for (lop, hop) in [(CmpOp::Ge, CmpOp::Le), (CmpOp::Gt, CmpOp::Lt)] {
                exprs.push(Expr::And(vec![
                    Expr::Compare { path: p("x"), op: lop, value: lo.clone() },
                    Expr::Compare { path: p("x"), op: hop, value: hi.clone() },
                ]));
            }
        }
        for e in &exprs {
            let want: Vec<NodeIx> = g.node_indices().filter(|&ix| e.matches_node(g, ix).unwrap()).collect::<Vec<_>>();
            match g.index_candidates(e).unwrap() {
                Some(c) => {
                    let got: Vec<NodeIx> = c.into_iter().filter(|&ix| e.matches_node(g, ix).unwrap()).collect();
                    let mut want = want.clone();
                    want.sort_unstable_by_key(|ix| ix.slot());
                    assert_eq!(got, want, "{e:?}");
                }
                None => assert!(matches!(e, Expr::Compare { op: CmpOp::Ne, .. } | Expr::Or(_)), "{e:?}"),
            }
        }
        // Exact lookups are exact (not just supersets)
        for v in &probes {
            let found = g.find_nodes(&p("x"), v).unwrap().unwrap();
            let e = Expr::Compare { path: p("x"), op: CmpOp::Eq, value: v.clone() };
            assert!(found.iter().all(|&ix| e.matches_node(g, ix).unwrap()));
        }
    }

    #[test]
    fn lookups_match_brute_force_through_changes() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut g = G::new();
        let mut live = Vec::new();
        for i in 0..40 {
            let ix = g.add_node(format!("n{i}"), Record::default()).unwrap();
            set(&mut g, ix, &mut rng);
            live.push(ix);
        }
        assert!(g.create_index::<GraphError>(&p("x")).unwrap());
        assert!(!g.create_index::<GraphError>(&p("x")).unwrap());
        assert!(!g.indexes_dirty());
        check(&g);
        for round in 0..30 {
            for _ in 0..5 {
                match rng.gen_range(0..5) {
                    0 => {
                        let ix = g.add_node(format!("m{round}_{}", rng.gen::<u32>()), Record::default()).unwrap();
                        set(&mut g, ix, &mut rng);
                        live.push(ix);
                    }
                    1 if !live.is_empty() => {
                        let ix = live.swap_remove(rng.gen_range(0..live.len()));
                        g.remove_node(ix);
                    }
                    2 if !live.is_empty() => {
                        let ix = live[rng.gen_range(0..live.len())];
                        g.add_label(ix, "L").unwrap();
                    }
                    _ if !live.is_empty() => {
                        let ix = live[rng.gen_range(0..live.len())];
                        set(&mut g, ix, &mut rng);
                    }
                    _ => {}
                }
            }
            if round % 7 == 3 {
                for (_, n) in g.nodes_mut().take(2) {
                    n.data.attr.insert("x".into(), Value::Int(1));
                }
            }
            check(&g);
            if round % 3 == 0 {
                g.flush_indexes().unwrap();
                assert!(!g.indexes_dirty());
                check(&g);
            }
        }
    }

    #[test]
    fn ranges_and_errors() {
        let mut g = G::new();
        for (i, v) in [Value::Int(1), Value::Float(2.5), Value::from("a"), Value::Int(4), Value::Date(Date(1))]
            .into_iter()
            .enumerate()
        {
            g.add_node(format!("n{i}"), Record::with_attr([("v", v)])).unwrap();
        }
        let path = p("v");
        assert_eq!(g.find_nodes(&path, &Value::Int(1)).unwrap(), None);
        g.create_index::<GraphError>(&path).unwrap();
        let ids = |r: Result<Option<Vec<NodeIx>>, GraphError>| -> Vec<String> {
            r.unwrap().unwrap().iter().map(|&ix| g.node(ix).unwrap().id().to_owned()).collect()
        };
        let (two, four) = (Value::Float(2.0), Value::Int(4));
        assert_eq!(ids(g.find_nodes_in_range(&path, Bound::Included(&two), Bound::Included(&four))), ["n1", "n3"]);
        assert_eq!(ids(g.find_nodes_in_range(&path, Bound::Unbounded, Bound::Excluded(&four))), ["n0", "n1"]);
        assert_eq!(ids(g.find_nodes_in_range(&path, Bound::Excluded(&four), Bound::Included(&two))), [] as [&str; 0]);
        assert_eq!(ids(g.find_nodes_in_range(&path, Bound::Excluded(&two), Bound::Excluded(&two))), [] as [&str; 0]);
        let a = Value::from("a");
        assert_eq!(ids(g.find_nodes_in_range(&path, Bound::Included(&a), Bound::Unbounded)), ["n2"]);
        let err = g.find_nodes_in_range::<GraphError>(&path, Bound::Included(&a), Bound::Included(&four));
        assert!(err.is_err());
        let nan = Value::Float(f64::NAN);
        assert!(g.find_nodes_in_range::<GraphError>(&path, Bound::Included(&nan), Bound::Unbounded).is_err());
        assert!(g.find_nodes_in_range::<GraphError>(&path, Bound::Unbounded, Bound::Unbounded).is_err());
        assert!(g.create_index::<GraphError>(&p("labels")).is_err());
        assert!(g.create_index::<GraphError>(&[]).is_err());
        assert_eq!(g.index_paths(), [path.as_slice()]);
        assert!(g.set_index_keys(g.node_ix("n0").unwrap(), vec![]).is_err());
        assert!(g.drop_index(&path));
        assert!(!g.drop_index(&path));
        // Built from given keys: nodes left out are re-read (dirty)
        let n0 = g.node_ix("n0").unwrap();
        assert!(g.create_index_with_keys(&path, [(n0, Key::of(&Value::Int(9)))]).unwrap());
        assert!(g.indexes_dirty());
        assert_eq!(g.find_nodes(&path, &Value::Int(9)).unwrap().unwrap(), [n0]);
        assert_eq!(g.find_nodes(&path, &Value::Int(4)).unwrap().unwrap().len(), 1);
        assert!(g.drop_index(&path));
        assert!(g.index_paths().is_empty());
    }

    /// One random change: add, remove, relabel or set a node.
    fn random_change(g: &mut G, live: &mut Vec<NodeIx>, rng: &mut StdRng) {
        match rng.gen_range(0..5) {
            0 => {
                let ix = g.add_node(format!("m{}", rng.gen::<u64>()), Record::default()).unwrap();
                set(g, ix, rng);
                live.push(ix);
            }
            1 if !live.is_empty() => {
                let ix = live.swap_remove(rng.gen_range(0..live.len()));
                g.remove_node(ix);
            }
            2 if !live.is_empty() => {
                let ix = live[rng.gen_range(0..live.len())];
                g.add_label(ix, "L").unwrap();
            }
            _ if !live.is_empty() => {
                let ix = live[rng.gen_range(0..live.len())];
                set(g, ix, rng);
            }
            _ => {}
        }
    }

    #[test]
    fn builds_off_the_graph_survive_changes() {
        let mut rng = StdRng::seed_from_u64(11);
        for case in 0..60 {
            let mut g = G::new();
            let mut live = Vec::new();
            for i in 0..30 {
                let ix = g.add_node(format!("n{i}"), Record::default()).unwrap();
                set(&mut g, ix, &mut rng);
                live.push(ix);
            }
            // Sometimes another index exists already, with pending changes
            if case % 3 == 0 {
                g.create_index::<GraphError>(&p("y")).unwrap();
                set(&mut g, live[0], &mut rng);
            }
            let mut build = g.begin_index_build(&p("x")).unwrap();
            assert_eq!(g.open_index_builds(), 1);
            // Read in chunks, changing the graph in between
            let nodes: Vec<NodeIx> = g.node_indices().collect();
            let skip = case % 4 == 1;
            for (i, chunk) in nodes.chunks(7).enumerate() {
                if skip && i == 1 {
                    continue; // never read: install must find these
                }
                build.read::<_, _, GraphError>(&g, chunk.iter().copied()).unwrap();
                for _ in 0..rng.gen_range(0..4) {
                    random_change(&mut g, &mut live, &mut rng);
                }
                if case % 10 == 7 && i == 2 {
                    for (_, n) in g.nodes_mut().take(1) {
                        n.data.attr.insert("x".into(), Value::Int(1));
                    }
                }
            }
            for _ in 0..rng.gen_range(0..4) {
                random_change(&mut g, &mut live, &mut rng);
            }
            assert!(g.install_index(build).unwrap());
            assert_eq!(g.open_index_builds(), 0);
            check(&g);
            for (a, b) in g.indexes.list_heaps() {
                assert_eq!(a, b);
            }
            random_change(&mut g, &mut live, &mut rng);
            check(&g);
            g.flush_indexes().unwrap();
            assert!(!g.indexes_dirty());
            check(&g);
        }
    }

    #[test]
    fn index_builds_belong_to_their_graph() {
        let mut g = G::new();
        let a = g.add_node("a", Record::with_attr([("x", Value::Int(1))])).unwrap();
        let b = g.add_node("b", Record::with_attr([("x", Value::Int(2))])).unwrap();
        assert!(g.begin_index_build(&p("labels")).is_err());

        // Unchanged and fully read: installed clean
        let mut build = g.begin_index_build(&p("x")).unwrap();
        build.insert(&g, a, Key::of(&Value::Int(1))).unwrap();
        build.insert(&g, b, Key::of(&Value::Int(2))).unwrap();
        assert_eq!((build.path(), build.len()), (p("x").as_slice(), 2));
        assert!(g.install_index(build).unwrap());
        assert!(!g.indexes_dirty());
        assert_eq!(g.find_nodes(&p("x"), &Value::Int(2)).unwrap().unwrap(), [b]);
        assert!(g.begin_index_build(&p("x")).is_err());

        // Another graph, a clone, or a second install: refused
        let mut other = G::new();
        let build = g.begin_index_build(&p("z")).unwrap();
        let mut copy = g.clone();
        assert_eq!(copy.open_index_builds(), 0);
        let mut build2 = other.begin_index_build(&p("z")).unwrap();
        assert!(build2.insert(&g, a, None).is_err());
        assert!(build2.read::<_, _, GraphError>(&copy, [a]).is_err());
        assert!(copy.install_index(build).is_err());
        assert!(!g.cancel_index_build(build2));
        // Dropped builds are no longer open, and are pruned on a change
        assert_eq!((g.open_index_builds(), other.open_index_builds()), (0, 0));
        assert_eq!(g.indexes.builds.len(), 1);
        g.node_mut(a).unwrap();
        assert!(g.indexes.builds.is_empty());
        let build = g.begin_index_build(&p("z")).unwrap();
        drop(build);
        assert_eq!(g.open_index_builds(), 0);

        // Indexed meanwhile: dropped, and stops tracking
        let build = g.begin_index_build(&p("w")).unwrap();
        g.create_index::<GraphError>(&p("w")).unwrap();
        assert!(!g.install_index(build).unwrap());
        assert_eq!(g.open_index_builds(), 0);

        // Cancelled builds stop tracking; dropping the last index keeps
        // the open builds
        let build = g.begin_index_build(&p("v")).unwrap();
        assert!(g.drop_index(&p("x")) && g.drop_index(&p("w")));
        assert_eq!(g.open_index_builds(), 1);
        g.node_mut(a).unwrap();
        assert!(g.memory_usage() > 0);
        assert!(g.cancel_index_build(build));
        assert_eq!(g.open_index_builds(), 0);
    }

    #[test]
    fn nested_paths_and_ops() {
        let mut g = G::new();
        let pos = |lat: f64| Value::Dict([("lat".to_string(), Value::Float(lat))].into_iter().collect());
        g.add_node("a", Record::with_attr([("pos", pos(1.0))])).unwrap();
        g.create_index::<GraphError>(&p("pos.lat")).unwrap();
        g.apply(crate::Op::AddNode { id: "b".into(), labels: vec![], data: Record::with_attr([("pos", pos(2.0))]) })
            .unwrap();
        g.apply(crate::Op::SetNodeAttr { id: "a".into(), key: "pos".into(), value: Some(pos(2.0)) }).unwrap();
        let found = g.find_nodes(&p("pos.lat"), &Value::Int(2)).unwrap().unwrap();
        assert_eq!(found.len(), 2);
        g.flush_indexes().unwrap();
        assert_eq!(g.find_nodes(&p("pos.lat"), &Value::Int(2)).unwrap().unwrap().len(), 2);
        g.apply(crate::Op::RemoveNode { id: "b".into() }).unwrap();
        assert_eq!(g.find_nodes(&p("pos.lat"), &Value::Int(2)).unwrap().unwrap(), [g.node_ix("a").unwrap()]);
    }
}
