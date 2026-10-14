// projection.rs
//
// A read-only, compact copy of (part of) a graph for analytics: the
// structure in CSR layout (one flat array of neighbours plus row offsets),
// optionally one edge weight per entry, and the node ids. It owns plain data
// and no payloads, so it is `Send + Sync`, algorithms can run on it in
// parallel, and it stays valid (a snapshot) when the graph changes later.
//
// Nodes get dense indices `0..node_count()` in the graph's slot order. Each
// neighbour list is sorted by neighbour index (parallel edges stay in
// insertion order), which enables merge-based set operations (triangles,
// similarity); parallel edges keep their edge-slot order (insertion order,
// unless removed edges' slots were reused). Besides the adjacency the
// projection follows (`out_*`), it
// keeps the transposed one (`in_*`); for an undirected projection both are
// the same array.
//
// Building happens in two steps so callers can do the second one without
// holding locks: `Projection::collect` reads the graph (filters, weights),
// `RawProjection::finish` sorts. `Projection::build` does both. The
// transposed adjacency and the id index are built on first use (once, thread
// safe), so one-off queries that need neither don't pay for them.
//
// `Projection::from_edges` builds the same thing from plain edge lists
// (node ids plus source / target indices in edge order) without a graph:
// node `i` is `ids[i]` and the edges' order plays the part of slot order.
// Such a projection has no graph handles (`node`, `index_of`), and may
// carry node coordinates (`coords`) for A*.
//
// Weights can change in place (`set_weights`), the structure can't. Edges
// are named by their position in the projected order: the order of the
// edge lists for `from_edges`, the edge-slot order of the graph's kept
// edges for `collect` (`edge_index` finds an edge's position). A weighted
// projection keeps, per edge, the adjacency position(s) it became
// (`positions`: one entry per edge, two for `Both`, the source's row then
// the target's), and the transposed adjacency, once built, where each
// entry of `out` went (`Transposed::of_out`), so a change costs O(1) per
// edge. Infinite weights mark edges as impassable: every search skips
// them (see `algo` and `batch`).

use rayon::prelude::*;
use std::ops::Range;
use std::sync::OnceLock;

use crate::pathfinding::{Coords, EdgeCost};
use crate::{Attributes, Direction, Edge, EdgeIx, Graph, GraphError, Node, NodeIx};

const NONE: u32 = u32::MAX;

/// An adjacency entry while building: the neighbour, the edge it comes
/// from (its position in the projected order; for `Both` twice that, plus
/// 1 for the entry in the target's row) and the weight (0.0 when
/// unweighted). 16 bytes, like a `(u32, f64)` pair.
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    to: u32,
    edge: u32,
    w: f64,
}

/// Adjacency in CSR layout: the neighbours of node `u` are
/// `to[start[u]..start[u + 1]]`, with weights at the same positions.
#[derive(Clone, Debug, Default)]
pub(crate) struct Csr {
    start: Vec<u32>,
    to: Vec<u32>,
    weight: Option<Vec<f64>>,
}

impl Csr {
    fn row(&self, u: u32) -> Range<usize> {
        self.start[u as usize] as usize..self.start[u as usize + 1] as usize
    }

    pub(crate) fn neighbors(&self, u: u32) -> &[u32] {
        &self.to[self.row(u)]
    }

    pub(crate) fn weights(&self, u: u32) -> Option<&[f64]> {
        let row = self.row(u);
        self.weight.as_ref().map(|w| &w[row])
    }

    /// The reverse adjacency (and, if weighted, where each entry of `self`
    /// went). Rows come out sorted because sources are visited in order.
    fn transpose(&self, n: usize) -> Transposed {
        let mut start = vec![0u32; n + 1];
        for &v in &self.to {
            start[v as usize + 1] += 1;
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        let mut fill: Vec<u32> = start[..n].to_vec();
        let mut to = vec![0u32; self.to.len()];
        let mut weight = self.weight.as_ref().map(|_| vec![0f64; self.to.len()]);
        let mut of_out = if self.weight.is_some() { vec![0u32; self.to.len()] } else { Vec::new() };
        for u in 0..n as u32 {
            for i in self.row(u) {
                let v = self.to[i] as usize;
                let at = fill[v] as usize;
                fill[v] += 1;
                to[at] = u;
                if let (Some(dst), Some(src)) = (&mut weight, &self.weight) {
                    dst[at] = src[i];
                    of_out[i] = at as u32;
                }
            }
        }
        Transposed { csr: Csr { start, to, weight }, of_out }
    }

    fn bytes(&self) -> usize {
        4 * (self.start.len() + self.to.len()) + self.weight.as_ref().map_or(0, |w| 8 * w.len())
    }
}

/// The transposed adjacency of a directed projection; for a weighted one
/// also the position in it of each entry of `out`, to update weights.
#[derive(Clone, Debug)]
struct Transposed {
    csr: Csr,
    of_out: Vec<u32>,
}

impl Transposed {
    fn bytes(&self) -> usize {
        self.csr.bytes() + 4 * self.of_out.len()
    }
}

/// Node coordinates: `dim` numbers per node, by dense index, row after row.
#[derive(Clone, Debug)]
struct Coordinates {
    dim: usize,
    values: Vec<f64>,
}

/// A read-only, compact copy of a graph's structure (optionally filtered
/// and weighted) for analytics; see the module comment.
#[derive(Clone, Debug)]
pub struct Projection {
    direction: Direction,
    edge_count: usize,
    out: Csr,
    /// Transposed `out`, built on first use; never used for undirected
    /// projections (the same as `out`).
    inc: OnceLock<Transposed>,
    /// Weighted projections: the position in `out` of each edge's entry
    /// (edge `k` at `k`; for `Both` at `2k` in the source's row and
    /// `2k + 1` in the target's). Empty if unweighted.
    positions: Vec<u32>,
    /// Weighted projections of a graph: the projected edges, in order
    /// (increasing slots). Empty otherwise.
    edges: Vec<EdgeIx>,
    /// Dense index -> node handle (empty without a graph) / id.
    nodes: Vec<NodeIx>,
    ids: Vec<String>,
    /// Dense indices ordered by id, for lookups by id; built on first use.
    by_id: OnceLock<Vec<u32>>,
    /// Node slot -> dense index (`NONE` for nodes not in the projection).
    dense: Vec<u32>,
    coords: Option<Coordinates>,
}

/// First half of building a [`Projection`]: everything read from the graph.
/// [`finish`](RawProjection::finish) needs no graph access.
pub struct RawProjection {
    direction: Direction,
    edge_count: usize,
    start: Vec<u32>,
    /// Unsorted rows, with edge positions and weights.
    adj: Vec<Entry>,
    weighted: bool,
    /// The kept edges, if weighted and collected from a graph.
    edges: Vec<EdgeIx>,
    nodes: Vec<NodeIx>,
    ids: Vec<String>,
    /// Heap bytes of `ids`' strings.
    id_bytes: usize,
    dense: Vec<u32>,
    /// Already known when built from edge lists (checking ids sorts them).
    by_id: Option<Vec<u32>>,
    coords: Option<Coordinates>,
}

/// Row starts and (unsorted) entries of an adjacency.
type Rows = (Vec<u32>, Vec<Entry>);

/// The adjacency rows of `kept` edges (source, target, weight) between `n`
/// nodes, by counting sort: a row lists a node's edges in the order of
/// `kept` (outgoing, then incoming for `Both`), unsorted. Returns the row
/// starts and the entries, tagged with their edge (see [`Entry`]).
fn adjacency_rows(n: usize, direction: Direction, kept: &[(u32, u32, f64)]) -> Result<Rows, GraphError> {
    let entries = if direction == Direction::Both { 2 * kept.len() } else { kept.len() };
    if u32::try_from(entries).is_err() || u32::try_from(n).is_err() {
        return Err(GraphError::InvalidArgument(format!(
            "graph too large to project ({} nodes, {} adjacency entries; at most {} each)",
            n,
            entries,
            u32::MAX - 1
        )));
    }
    let (forward, backward) = match direction {
        Direction::Out => (true, false),
        Direction::In => (false, true),
        Direction::Both => (true, true),
    };
    let mut start = vec![0u32; n + 1];
    for &(s, t, _) in kept {
        if forward {
            start[s as usize + 1] += 1;
        }
        if backward {
            start[t as usize + 1] += 1;
        }
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill: Vec<u32> = start[..n].to_vec();
    let mut adj = vec![Entry::default(); entries];
    let mut put = |row: u32, to: u32, edge: usize, w: f64| {
        let at = &mut fill[row as usize];
        // `edge` < `entries`, checked above to fit
        adj[*at as usize] = Entry { to, edge: edge as u32, w };
        *at += 1;
    };
    let both = forward && backward;
    if forward {
        for (k, &(s, t, w)) in kept.iter().enumerate() {
            put(s, t, if both { 2 * k } else { k }, w);
        }
    }
    if backward {
        for (k, &(s, t, w)) in kept.iter().enumerate() {
            put(t, s, if both { 2 * k + 1 } else { k }, w);
        }
    }
    Ok((start, adj))
}

/// Checked dense index `x` of `n`, found at `name[at]`.
fn node_index(x: i64, n: usize, name: &str, at: usize) -> Result<u32, GraphError> {
    match usize::try_from(x) {
        Ok(u) if u < n => Ok(u as u32),
        _ if n == 0 => Err(GraphError::InvalidArgument(format!("{}[{}] is {}, but there are no nodes", name, at, x))),
        _ => Err(GraphError::InvalidArgument(format!("{}[{}] is {}, not a node index (0 to {})", name, at, x, n - 1))),
    }
}

impl Projection {
    /// Project every node and edge of `g`, following edges in `direction`,
    /// with weights from `cost` (`EdgeCost::Unit` for none). Every edge's
    /// weight is read and validated here.
    pub fn build<N, E, X>(g: &Graph<N, E>, direction: Direction, cost: &EdgeCost) -> Result<Self, X>
    where
        E: Attributes,
        X: From<GraphError> + From<E::Error>,
    {
        Ok(Self::collect::<N, E, X>(g, direction, cost, |_, _| Ok(true), |_, _| Ok(true))?.finish())
    }

    /// Read the part of `g` to project: the nodes for which `node_ok` is
    /// true, and the edges between them for which `edge_ok` is true (asked
    /// once per edge, in slot order, and only for edges between kept nodes).
    /// Weights are read and validated for the kept edges.
    pub fn collect<N, E, X>(
        g: &Graph<N, E>,
        direction: Direction,
        cost: &EdgeCost,
        mut node_ok: impl FnMut(NodeIx, &Node<N>) -> Result<bool, X>,
        mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
    ) -> Result<RawProjection, X>
    where
        E: Attributes,
        X: From<GraphError> + From<E::Error>,
    {
        let mut dense = vec![NONE; g.node_bound()];
        let mut nodes = Vec::with_capacity(g.node_count());
        let mut ids = Vec::with_capacity(g.node_count());
        let mut id_bytes = 0;
        for (ix, node) in g.nodes() {
            if node_ok(ix, node)? {
                dense[ix.slot()] = nodes.len() as u32;
                nodes.push(ix);
                let id = node.id().to_owned();
                id_bytes += id.capacity();
                ids.push(id);
            }
        }

        // Kept edges, in slot order: one sequential pass over the edge arena
        let weighted = !matches!(cost, EdgeCost::Unit);
        let mut kept: Vec<(u32, u32, f64)> = Vec::with_capacity(g.edge_count());
        let mut edges = Vec::new();
        for (e, edge) in g.edges() {
            let (s, t) = (dense[edge.source().slot()], dense[edge.target().slot()]);
            if s == NONE || t == NONE || !edge_ok(e, edge)? {
                continue;
            }
            let w = if weighted { cost.cost::<E, X>(&edge.data)? } else { 0.0 };
            kept.push((s, t, w));
            if weighted {
                edges.push(e);
            }
        }
        let edge_count = kept.len();
        // Rows in slot order, sorted by `finish`
        let (start, adj) = adjacency_rows(nodes.len(), direction, &kept)?;
        drop(kept);
        Ok(RawProjection {
            direction,
            edge_count,
            start,
            adj,
            weighted,
            edges,
            nodes,
            ids,
            id_bytes,
            dense,
            by_id: None,
            coords: None,
        })
    }

    /// Build from edge lists instead of a graph: node `i` has id `ids[i]`,
    /// edge `k` goes from `src[k]` to `dst[k]` (indices into `ids`) with
    /// weight `weight[k]` (unweighted if `None`). Edges keep the order
    /// given (parallel edges stay in it), so the result is the same as
    /// projecting a graph with these nodes and edges, added in this order,
    /// in `direction`. Ids must be distinct, the lists equally long, the
    /// indices in range and the weights non-negative (not NaN), as for
    /// graph edges.
    ///
    /// The result has no graph handles: [`node`](Self::node) panics and
    /// [`index_of`](Self::index_of) finds nothing.
    pub fn from_edges<I>(
        ids: Vec<String>,
        direction: Direction,
        src: &[I],
        dst: &[I],
        weight: Option<&[f64]>,
    ) -> Result<RawProjection, GraphError>
    where
        I: Copy + Into<i64>,
    {
        let n = ids.len();
        if src.len() != dst.len() {
            return Err(GraphError::InvalidArgument(format!(
                "src and dst must have the same length, got {} and {}",
                src.len(),
                dst.len()
            )));
        }
        if let Some(w) = weight
            && w.len() != src.len()
        {
            return Err(GraphError::InvalidArgument(format!(
                "weight must have one entry per edge ({}), got {}",
                src.len(),
                w.len()
            )));
        }
        if u32::try_from(n).is_err() {
            return Err(GraphError::InvalidArgument(format!(
                "graph too large to project ({} nodes; at most {})",
                n,
                u32::MAX - 1
            )));
        }

        // Ids sorted, to find duplicates; kept as the id index
        let mut by_id: Vec<u32> = (0..n as u32).collect();
        by_id.par_sort_unstable_by(|&a, &b| ids[a as usize].cmp(&ids[b as usize]).then(a.cmp(&b)));
        if let Some(pair) = by_id.windows(2).find(|p| ids[p[0] as usize] == ids[p[1] as usize]) {
            return Err(GraphError::InvalidArgument(format!(
                "duplicate node id '{}' (ids[{}] and ids[{}])",
                ids[pair[0] as usize], pair[0], pair[1]
            )));
        }

        let mut kept: Vec<(u32, u32, f64)> = Vec::with_capacity(src.len());
        for (k, (&s, &t)) in src.iter().zip(dst).enumerate() {
            let s = node_index(s.into(), n, "src", k)?;
            let t = node_index(t.into(), n, "dst", k)?;
            let w = match weight {
                None => 0.0,
                Some(w) => {
                    let w = w[k];
                    if w.is_nan() || w < 0.0 {
                        return Err(GraphError::InvalidArgument(format!(
                            "Edge weights must be non-negative, got {} (weight[{}])",
                            w, k
                        )));
                    }
                    w
                }
            };
            kept.push((s, t, w));
        }
        let (start, adj) = adjacency_rows(n, direction, &kept)?;
        let id_bytes = ids.iter().map(String::capacity).sum();
        Ok(RawProjection {
            direction,
            edge_count: kept.len(),
            start,
            adj,
            weighted: weight.is_some(),
            edges: Vec::new(),
            nodes: Vec::new(),
            ids,
            id_bytes,
            dense: Vec::new(),
            by_id: Some(by_id),
            coords: None,
        })
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.ids.len()
    }

    /// Number of graph edges projected (an undirected projection lists each
    /// in both endpoints' neighbour lists).
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The direction edges were followed in when projecting: `Out` keeps
    /// them as they are, `In` reverses them, `Both` makes them undirected.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Whether the projection has edge weights.
    pub fn is_weighted(&self) -> bool {
        self.out.weight.is_some()
    }

    /// Id of dense node `u`.
    pub fn id(&self, u: u32) -> &str {
        &self.ids[u as usize]
    }

    /// All node ids, by dense index.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// Graph handle of dense node `u`. Panics for a projection built by
    /// [`from_edges`](Self::from_edges), which has no graph.
    pub fn node(&self, u: u32) -> NodeIx {
        self.nodes[u as usize]
    }

    /// Dense index of the node with this id, if it is in the projection.
    pub fn index_of_id(&self, id: &str) -> Option<u32> {
        let by_id = self.by_id.get_or_init(|| {
            let mut by_id: Vec<u32> = (0..self.ids.len() as u32).collect();
            by_id.par_sort_unstable_by(|&a, &b| self.ids[a as usize].cmp(&self.ids[b as usize]));
            by_id
        });
        by_id.binary_search_by(|&u| self.ids[u as usize].as_str().cmp(id)).ok().map(|i| by_id[i])
    }

    /// The adjacency against the projection's edges.
    pub(crate) fn inc(&self) -> &Csr {
        if self.direction == Direction::Both {
            return &self.out;
        }
        &self.inc.get_or_init(|| self.out.transpose(self.ids.len())).csr
    }

    /// Number of coordinates per node (0 without coordinates).
    pub fn coord_dim(&self) -> usize {
        self.coords.as_ref().map_or(0, |c| c.dim)
    }

    /// Coordinates of dense node `u`, if the projection has them (see
    /// [`RawProjection::with_coords`]).
    pub fn coords(&self, u: u32) -> Option<&[f64]> {
        self.coords.as_ref().map(|c| &c.values[u as usize * c.dim..(u as usize + 1) * c.dim])
    }

    /// Dense index of a graph node, if it is in the projection. Only
    /// meaningful for the graph the projection was built from.
    pub fn index_of(&self, ix: NodeIx) -> Option<u32> {
        match self.dense.get(ix.slot()) {
            Some(&u) if u != NONE && self.nodes[u as usize] == ix => Some(u),
            _ => None,
        }
    }

    /// Neighbours of `u` along the projection's edges, sorted.
    pub fn out_neighbors(&self, u: u32) -> &[u32] {
        self.out.neighbors(u)
    }

    /// Weights of the edges to [`out_neighbors`](Self::out_neighbors).
    pub fn out_weights(&self, u: u32) -> Option<&[f64]> {
        self.out.weights(u)
    }

    /// Nodes with an edge to `u`, sorted (the same as `out_neighbors` for an
    /// undirected projection).
    pub fn in_neighbors(&self, u: u32) -> &[u32] {
        self.inc().neighbors(u)
    }

    /// Weights of the edges from [`in_neighbors`](Self::in_neighbors).
    pub fn in_weights(&self, u: u32) -> Option<&[f64]> {
        self.inc().weights(u)
    }

    pub fn out_degree(&self, u: u32) -> usize {
        self.out.row(u).len()
    }

    pub fn in_degree(&self, u: u32) -> usize {
        self.inc().row(u).len()
    }

    /// Position of graph edge `e` in the projected order (what
    /// [`set_weights`](Self::set_weights) takes), if it was projected.
    /// Only weighted projections collected from a graph keep their edges;
    /// others find nothing. Only meaningful for the graph the projection
    /// was built from.
    pub fn edge_index(&self, e: EdgeIx) -> Option<usize> {
        let k = self.edges.binary_search_by_key(&e.slot(), |x| x.slot()).ok()?;
        (self.edges[k] == e).then_some(k)
    }

    /// Change the weights of some edges in place: edge `edges[i]` (its
    /// position in the projected order: the order of the edge lists for
    /// [`from_edges`](Self::from_edges), the graph's edge-slot order of the
    /// kept edges for [`collect`](Self::collect); see
    /// [`edge_index`](Self::edge_index)) gets `weights[i]`. The structure
    /// stays as it is; an infinite weight makes the edge impassable (every
    /// search skips it), a finite one makes it passable again. Every
    /// adjacency entry of the edge is updated (both rows for `Both`, the
    /// transposed adjacency if it was built), so results are the same as
    /// for a projection built with these weights. O(1) per edge.
    ///
    /// Fails, changing nothing, if the projection is unweighted, the lists'
    /// lengths differ, an index is out of range or a weight is negative or
    /// NaN. If an edge is listed more than once, the last weight wins.
    pub fn set_weights<I>(&mut self, edges: &[I], weights: &[f64]) -> Result<(), GraphError>
    where
        I: Copy + Into<i64>,
    {
        if !self.is_weighted() {
            return Err(GraphError::InvalidArgument(
                "set_weights needs a weighted projection (project with weight=... or from_arrays(weight=...))".into(),
            ));
        }
        if edges.len() != weights.len() {
            return Err(GraphError::InvalidArgument(format!(
                "edges and weights must have the same length, got {} and {}",
                edges.len(),
                weights.len()
            )));
        }
        let m = self.edge_count;
        for (i, (&k, &w)) in edges.iter().zip(weights).enumerate() {
            let k: i64 = k.into();
            if usize::try_from(k).map_or(true, |k| k >= m) {
                return Err(GraphError::InvalidArgument(if m == 0 {
                    format!("edges[{}] is {}, but there are no edges", i, k)
                } else {
                    format!("edges[{}] is {}, not an edge index (0 to {})", i, k, m - 1)
                }));
            }
            if w.is_nan() || w < 0.0 {
                return Err(GraphError::InvalidArgument(format!(
                    "Edge weights must be non-negative, got {} (weights[{}])",
                    w, i
                )));
            }
        }
        let both = self.direction == Direction::Both;
        let out = self.out.weight.as_mut().expect("checked: the projection is weighted");
        let mut inc = self.inc.get_mut();
        for (&k, &w) in edges.iter().zip(weights) {
            let k = k.into() as usize;
            let at = if both { &self.positions[2 * k..2 * k + 2] } else { &self.positions[k..k + 1] };
            for &i in at {
                out[i as usize] = w;
                if let Some(t) = inc.as_deref_mut() {
                    let w_in = t.csr.weight.as_mut().expect("weighted like `out`");
                    w_in[t.of_out[i as usize] as usize] = w;
                }
            }
        }
        Ok(())
    }

    /// Approximate heap memory used, in bytes.
    pub fn memory_usage(&self) -> usize {
        let ids: usize = self.ids.iter().map(|s| s.capacity() + std::mem::size_of::<String>()).sum();
        self.out.bytes()
            + self.inc.get().map_or(0, Transposed::bytes)
            + 4 * self.positions.len()
            + 8 * self.edges.len()
            + ids
            + 8 * self.nodes.len()
            + 4 * (self.by_id.get().map_or(0, Vec::len) + self.dense.len())
            + self.coords.as_ref().map_or(0, |c| 8 * c.values.len())
    }
}

impl RawProjection {
    /// Approximate heap memory used, in bytes. O(1). `finish` sorts in
    /// place, so this is also about what finishing holds at its peak, on
    /// top of the projection it returns.
    pub fn memory_usage(&self) -> usize {
        use std::mem::size_of;
        size_of::<u32>() * (self.start.capacity() + self.dense.capacity())
            + size_of::<Entry>() * self.adj.capacity()
            + size_of::<EdgeIx>() * self.edges.capacity()
            + size_of::<NodeIx>() * self.nodes.capacity()
            + size_of::<String>() * self.ids.capacity()
            + self.id_bytes
            + size_of::<u32>() * self.by_id.as_ref().map_or(0, Vec::capacity)
            + size_of::<f64>() * self.coords.as_ref().map_or(0, |c| c.values.capacity())
    }

    /// Attach node coordinates (for A*): `dim` (at least 1) finite numbers
    /// per node, by dense index, row after row (`values[u * dim + i]`).
    pub fn with_coords(mut self, dim: usize, values: Vec<f64>) -> Result<Self, GraphError> {
        let n = self.ids.len();
        if dim == 0 {
            return Err(GraphError::InvalidArgument("coords must have at least one column".into()));
        }
        if Some(values.len()) != n.checked_mul(dim) {
            return Err(GraphError::InvalidArgument(format!(
                "coords must have one row of {} numbers per node ({} nodes), got {} numbers",
                dim,
                n,
                values.len()
            )));
        }
        if let Some(i) = values.iter().position(|x| !x.is_finite()) {
            return Err(GraphError::InvalidArgument(format!(
                "coords must be finite, got {} (row {}, column {})",
                values[i],
                i / dim,
                i % dim
            )));
        }
        self.coords = Some(Coordinates { dim, values });
        Ok(self)
    }

    /// Attach coordinates read from the projected nodes of `g` (the graph
    /// this was collected from) at `coords`. Every node needs coordinates
    /// of the same length; missing or non-numeric ones are an error.
    pub fn with_graph_coords<N, E, X>(self, g: &Graph<N, E>, coords: &Coords) -> Result<Self, X>
    where
        N: Attributes,
        X: From<GraphError> + From<N::Error>,
    {
        if self.nodes.len() != self.ids.len() {
            return Err(GraphError::InvalidArgument("this projection was not collected from a graph".into()).into());
        }
        let mut dim = 0;
        let mut values = Vec::new();
        for (u, &ix) in self.nodes.iter().enumerate() {
            let node = g.node(ix).ok_or(GraphError::Stale)?;
            let row = coords.read::<N, X>(node)?.ok_or_else(|| {
                GraphError::InvalidArgument(format!(
                    "node '{}' has no coordinates at coords={}",
                    node.id(),
                    coords.describe()
                ))
            })?;
            if u == 0 {
                dim = row.len();
                values.reserve(dim * self.nodes.len());
            } else if row.len() != dim {
                return Err(GraphError::InvalidArgument(format!(
                    "node '{}' has {} coordinates but node '{}' has {}",
                    node.id(),
                    row.len(),
                    self.ids[0],
                    dim
                ))
                .into());
            }
            values.extend_from_slice(&row);
        }
        if self.nodes.is_empty() {
            dim = match coords {
                Coords::PerDimension(paths) => paths.len(),
                Coords::Sequence(_) => 1,
            };
        }
        Ok(self.with_coords(dim, values)?)
    }

    /// Sort the neighbour lists (in parallel where it pays off).
    pub fn finish(self) -> Projection {
        let RawProjection {
            direction,
            edge_count,
            start,
            mut adj,
            weighted,
            edges,
            nodes,
            ids,
            dense,
            by_id,
            coords,
            ..
        } = self;

        // Sort each row by neighbour; the sort is stable, so parallel edges
        // keep their insertion order
        let mut rows: Vec<&mut [Entry]> = Vec::with_capacity(ids.len());
        let mut rest = &mut adj[..];
        for [from, to] in start.array_windows() {
            let (row, tail) = rest.split_at_mut((to - from) as usize);
            rows.push(row);
            rest = tail;
        }
        rows.par_iter_mut().with_min_len(1024).for_each(|row| row.sort_by_key(|e| e.to));

        let to = adj.iter().map(|e| e.to).collect();
        let weight = weighted.then(|| adj.iter().map(|e| e.w).collect());
        let mut positions = Vec::new();
        if weighted {
            positions = vec![0u32; adj.len()];
            for (i, e) in adj.iter().enumerate() {
                positions[e.edge as usize] = i as u32;
            }
        }
        drop(adj);
        let out = Csr { start, to, weight };
        let by_id = by_id.map_or_else(OnceLock::new, OnceLock::from);
        Projection {
            direction,
            edge_count,
            out,
            inc: OnceLock::new(),
            positions,
            edges,
            nodes,
            ids,
            by_id,
            dense,
            coords,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Record, Value};

    type G = Graph<Record, Record>;

    fn weighted(w: f64) -> Record {
        Record::with_attr([("weight", Value::from(w))])
    }

    /// c -> a, a -> b (twice, weights 2 then 1), b -> c, a -> a
    fn graph() -> (G, Vec<NodeIx>) {
        let mut g = G::new();
        let ix: Vec<NodeIx> = ["c", "a", "b"].iter().map(|id| g.add_node(*id, Record::default()).unwrap()).collect();
        let (c, a, b) = (ix[0], ix[1], ix[2]);
        g.add_edge(a, b, weighted(2.0)).unwrap();
        g.add_edge(c, a, weighted(3.0)).unwrap();
        g.add_edge(a, a, weighted(4.0)).unwrap();
        g.add_edge(a, b, weighted(1.0)).unwrap();
        g.add_edge(b, c, weighted(5.0)).unwrap();
        (g, ix)
    }

    #[test]
    fn raw_projection_reports_its_memory() {
        let (g, _) = graph();
        let cost = EdgeCost::weighted(None, None);
        let raw = Projection::collect::<_, _, GraphError>(&g, Direction::Both, &cost, |_, _| Ok(true), |_, _| Ok(true))
            .unwrap();
        let m = raw.memory_usage();
        // 10 adjacency entries, 4 row starts, 3 nodes and their ids
        assert!(m >= 10 * 16 + 4 * 4 + 3 * (8 + 24 + 1), "{m}");
        let p = raw.finish();
        assert!(m >= p.memory_usage() / 2, "{m} {}", p.memory_usage());
    }

    #[test]
    fn out_and_in_adjacency_are_sorted() {
        let (g, _) = graph();
        let p = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::weighted(None, None)).unwrap();
        assert_eq!((p.node_count(), p.edge_count()), (3, 5));
        let [c, a, b] = ["c", "a", "b"].map(|id| p.index_of_id(id).unwrap());
        assert_eq!((c, a, b), (0, 1, 2));
        assert_eq!(p.out_neighbors(a), [a, b, b]);
        assert_eq!(p.out_weights(a).unwrap(), [4.0, 2.0, 1.0]); // parallel edges keep their order
        assert_eq!(p.in_neighbors(a), [c, a]);
        assert_eq!(p.in_weights(a).unwrap(), [3.0, 4.0]);
        assert_eq!(p.in_neighbors(b), [a, a]);
        assert_eq!((p.out_degree(c), p.in_degree(c)), (1, 1));
        assert_eq!(p.index_of_id("zz"), None);
        assert!(p.memory_usage() > 0);

        let rev = Projection::build::<_, _, GraphError>(&g, Direction::In, &EdgeCost::Unit).unwrap();
        assert!(!rev.is_weighted());
        assert_eq!(rev.out_neighbors(a), [c, a]);
        assert_eq!(rev.in_neighbors(a), [a, b, b]);

        let both = Projection::build::<_, _, GraphError>(&g, Direction::Both, &EdgeCost::Unit).unwrap();
        assert_eq!(both.edge_count(), 5);
        assert_eq!(both.out_neighbors(a), [c, a, a, b, b]); // the self-loop from both ends
        assert_eq!(both.in_neighbors(a), both.out_neighbors(a));
    }

    #[test]
    fn filters_and_handles() {
        let (mut g, ix) = graph();
        let mut asked = 0;
        let raw = Projection::collect::<_, _, GraphError>(
            &g,
            Direction::Both,
            &EdgeCost::Unit,
            |_, n| Ok(n.id() != "c"),
            |_, e| {
                asked += 1;
                Ok(e.source() != e.target())
            },
        )
        .unwrap();
        // Only the three edges between a and b are offered, once each
        assert_eq!(asked, 3);
        let p = raw.finish();
        assert_eq!(p.ids(), ["a", "b"]);
        assert_eq!(p.edge_count(), 2);
        assert_eq!(p.index_of(ix[0]), None);
        assert_eq!(p.index_of(ix[1]), Some(0));
        assert_eq!(p.node(1), ix[2]);

        // A snapshot: later changes to the graph don't affect it
        g.remove_node(ix[2]).unwrap();
        assert_eq!(p.out_neighbors(0), [1, 1]);
        assert_eq!(p.index_of(ix[2]), Some(1)); // the handle was valid when projected
    }

    #[test]
    fn bad_weights_fail() {
        let (mut g, ix) = graph();
        g.add_edge(ix[0], ix[1], Record::with_attr([("weight", Value::from("x"))])).unwrap();
        let err = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::weighted(None, None));
        assert!(matches!(err, Err(GraphError::InvalidType(_))));
        // Unless the edge is filtered out
        let p = Projection::collect::<_, _, GraphError>(
            &g,
            Direction::Out,
            &EdgeCost::weighted(None, None),
            |_, _| Ok(true),
            |_, e| Ok(e.data.attr.get("weight").and_then(Value::as_f64).is_some()),
        );
        assert_eq!(p.unwrap().finish().edge_count(), 5);
    }

    /// Every row, weights and id of two projections agree.
    fn assert_same(a: &Projection, b: &Projection) {
        assert_eq!((a.ids(), a.edge_count(), a.direction()), (b.ids(), b.edge_count(), b.direction()));
        assert_eq!(a.is_weighted(), b.is_weighted());
        for u in 0..a.node_count() as u32 {
            assert_eq!(a.out_neighbors(u), b.out_neighbors(u));
            assert_eq!(a.out_weights(u), b.out_weights(u));
            assert_eq!(a.in_neighbors(u), b.in_neighbors(u));
            assert_eq!(a.in_weights(u), b.in_weights(u));
            assert_eq!(a.index_of_id(a.id(u)), Some(u));
        }
    }

    #[test]
    fn from_edges_matches_a_graph_projection() {
        let (g, handles) = graph();
        let ids: Vec<String> = ["c", "a", "b"].map(String::from).to_vec();
        // The edges of `graph()`, in insertion order
        let (src, dst) = ([1i64, 0, 1, 1, 2], [2i64, 1, 1, 2, 0]);
        let w = [2.0, 3.0, 4.0, 1.0, 5.0];
        for direction in [Direction::Out, Direction::In, Direction::Both] {
            let built = Projection::build::<_, _, GraphError>(&g, direction, &EdgeCost::weighted(None, None)).unwrap();
            let raw = Projection::from_edges(ids.clone(), direction, &src, &dst, Some(&w)).unwrap();
            assert!(raw.memory_usage() > 0);
            let p = raw.finish();
            assert_same(&p, &built);
            assert_eq!(p.index_of(handles[0]), None);
            assert_eq!(p.coords(0), None);

            let unweighted = Projection::build::<_, _, GraphError>(&g, direction, &EdgeCost::Unit).unwrap();
            let src32: Vec<u32> = src.iter().map(|&x| x as u32).collect();
            let dst32: Vec<u32> = dst.iter().map(|&x| x as u32).collect();
            assert_same(
                &Projection::from_edges(ids.clone(), direction, &src32, &dst32, None).unwrap().finish(),
                &unweighted,
            );
        }
        let empty = Projection::from_edges::<i64>(Vec::new(), Direction::Out, &[], &[], None).unwrap().finish();
        assert!(empty.is_empty());
        assert_eq!(empty.index_of_id("a"), None);
    }

    #[test]
    fn from_edges_checks_its_input() {
        let ids = || vec!["a".to_string(), "b".to_string()];
        let err = |r: Result<RawProjection, GraphError>| match r {
            Err(GraphError::InvalidArgument(msg)) => msg,
            Err(e) => panic!("unexpected error {e}"),
            Ok(_) => panic!("no error"),
        };
        let out = Direction::Out;
        assert!(err(Projection::from_edges(ids(), out, &[0i64], &[1, 0], None)).contains("same length"));
        assert!(err(Projection::from_edges(ids(), out, &[0i64], &[1], Some(&[]))).contains("one entry per edge"));
        assert_eq!(
            err(Projection::from_edges(ids(), out, &[0i64, 2], &[1, 0], None)),
            "src[1] is 2, not a node index (0 to 1)"
        );
        assert!(err(Projection::from_edges(ids(), out, &[0i64], &[-1], None)).starts_with("dst[0] is -1"));
        assert!(err(Projection::from_edges(Vec::new(), out, &[0i64], &[0], None)).contains("no nodes"));
        for bad in [-1.0, f64::NAN] {
            let msg = err(Projection::from_edges(ids(), out, &[0i64, 1], &[1, 0], Some(&[1.0, bad])));
            assert!(msg.starts_with("Edge weights must be non-negative") && msg.ends_with("(weight[1])"), "{msg}");
        }
        // Infinite weights are allowed, as for graph edges
        assert!(Projection::from_edges(ids(), out, &[0i64], &[1], Some(&[f64::INFINITY])).is_ok());
        let dup = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(
            err(Projection::from_edges::<i64>(dup, out, &[], &[], None)),
            "duplicate node id 'a' (ids[0] and ids[2])"
        );
    }

    #[test]
    fn coordinates() {
        let raw = || Projection::from_edges(vec!["a".into(), "b".into()], Direction::Out, &[0i64], &[1], None).unwrap();
        let p = raw().with_coords(2, vec![0.0, 1.0, 2.0, 3.0]).unwrap().finish();
        assert_eq!((p.coord_dim(), p.coords(1)), (2, Some(&[2.0, 3.0][..])));
        assert_eq!(raw().finish().coord_dim(), 0);
        assert!(raw().with_coords(0, vec![]).is_err());
        assert!(raw().with_coords(2, vec![0.0; 3]).is_err());
        assert!(raw().with_coords(1, vec![0.0, f64::INFINITY]).is_err());
    }

    #[test]
    fn coordinates_from_the_graph() {
        let mut g = G::new();
        let at = |x: f64, y: f64| Record::with_attr([("x", Value::from(x)), ("y", Value::from(y))]);
        let a = g.add_node("a", at(0.0, 1.0)).unwrap();
        let b = g.add_node("b", at(2.0, 3.0)).unwrap();
        g.add_edge(a, b, Record::default()).unwrap();
        let xy = Coords::per_dimension(&["x", "y"]).unwrap();
        let collect = |g: &G| {
            Projection::collect::<_, _, GraphError>(
                g,
                Direction::Out,
                &EdgeCost::Unit,
                |_, _| Ok(true),
                |_, _| Ok(true),
            )
            .unwrap()
        };
        let p = collect(&g).with_graph_coords::<_, _, GraphError>(&g, &xy).unwrap().finish();
        assert_eq!((p.coord_dim(), p.coords(1)), (2, Some(&[2.0, 3.0][..])));
        let err = |c: &Coords, g: &G| collect(g).with_graph_coords::<_, _, GraphError>(g, c).err().unwrap().to_string();
        assert_eq!(
            err(&Coords::per_dimension(&["x", "z"]).unwrap(), &g),
            "node 'a' has no coordinates at coords=['x', 'z']"
        );
        g.add_node("c", Record::with_attr([("x", Value::from("left")), ("y", Value::from(0.0))])).unwrap();
        assert!(err(&xy, &g).contains("must be a number"));
    }
}
