// random_walks.rs
//
// Random walks over a compact adjacency index built once per call; the walks
// themselves never touch the graph (so callers may run them without holding
// any lock on it, e.g. with the Python GIL released).

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::budget::{Budget, Limited, Meter, OnLimit};
use crate::{Attributes, Graph, GraphError, Lookup};

/// Number of walk attempts handled by one RNG stream in uniform mode. Chunks
/// are processed in parallel; because the chunking (and each chunk's seed) is
/// independent of the thread count, a given `seed` always yields the same
/// walks.
const CHUNK: usize = 256;

/// Parameters of a batch of random walks.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkOptions {
    /// Maximum number of nodes per walk (> 0).
    pub max_length: usize,
    /// Number of walks attempted.
    pub num_attempts: usize,
    /// Walks with fewer nodes are dropped (default 1).
    pub min_length: usize,
    /// Whether a walk may visit a node twice (default false).
    pub allow_revisit: bool,
    /// Interleave edge types with node ids in the output (default false).
    pub include_edge_types: bool,
    /// Edge attribute holding the edge type (default `"type"`; edges
    /// without a string there are `"unknown"`).
    pub edge_type_field: String,
    /// Weight every choice by `1 / (1 + times visited)`, steering walks
    /// towards the least-visited nodes; visit counts persist across the
    /// attempts. Without a start node, each walk's start is sampled that way
    /// too (default false).
    pub stratified: bool,
    /// Seed for the RNG; the same seed and options give the same walks
    /// (default: random).
    pub seed: Option<u64>,
}

impl WalkOptions {
    pub fn new(max_length: usize, num_attempts: usize) -> Self {
        WalkOptions {
            max_length,
            num_attempts,
            min_length: 1,
            allow_revisit: false,
            include_edge_types: false,
            edge_type_field: "type".to_string(),
            stratified: false,
            seed: None,
        }
    }
}

/// A walk as indices into `WalkIndex::ids` / `WalkIndex::types`.
pub struct Walk {
    nodes: Vec<u32>,
    edges: Vec<u32>, // Edge type indices between nodes
}

/// Compact adjacency representation of the graph, built once per call.
struct WalkIndex {
    /// Node ids in graph order.
    ids: Vec<String>,
    /// Number of nodes that may be sampled as a start (all of them).
    n_real: usize,
    /// Outgoing neighbours as (target index, edge type index).
    adj: Vec<Vec<(u32, u32)>>,
    types: Vec<String>,
}

impl WalkIndex {
    fn build<N, E: Attributes>(g: &Graph<N, E>, include_edges: bool, type_field: &str) -> Result<Self, E::Error> {
        let mut dense = vec![u32::MAX; g.node_bound()];
        let mut ids = Vec::with_capacity(g.node_count());
        for (i, (ix, node)) in g.nodes().enumerate() {
            dense[ix.slot()] = i as u32;
            ids.push(node.id().to_owned());
        }
        let mut types: Vec<String> = Vec::new();
        let mut type_index: HashMap<String, u32> = HashMap::new();
        let mut adj: Vec<Vec<(u32, u32)>> = Vec::with_capacity(ids.len());

        for (_, node) in g.nodes() {
            let mut out = Vec::with_capacity(node.out_edges().len());
            for &e in node.out_edges() {
                let edge = g.edge_ref(e);
                let target = dense[edge.target().slot()];
                let type_idx = if include_edges {
                    // "type" is the edge's type field (or, failing that, the
                    // attribute); other names are attributes
                    let field = (type_field == "type").then(|| edge.edge_type()).flatten();
                    let name = match field {
                        Some(t) => g.symbol_name(t).to_owned(),
                        None => match edge.data.text(type_field)? {
                            Lookup::Found(s) => s,
                            Lookup::Missing | Lookup::Invalid => "unknown".to_string(),
                        },
                    };
                    match type_index.get(&name) {
                        Some(&t) => t,
                        None => {
                            let t = types.len() as u32;
                            type_index.insert(name.clone(), t);
                            types.push(name);
                            t
                        }
                    }
                } else {
                    0
                };
                out.push((target, type_idx));
            }
            adj.push(out);
        }

        let n_real = ids.len();
        Ok(WalkIndex { ids, n_real, adj, types })
    }
}

/// Fenwick (binary indexed) tree over f64 weights: O(log n) point update and
/// O(log n) sampling proportional to weight.
/// The lowest set bit of `i` (`i > 0`), which steps through a Fenwick tree.
/// (`usize::isolate_lowest_one` needs Rust 1.98.)
fn lowest_bit(i: usize) -> usize {
    1 << i.trailing_zeros()
}

struct Fenwick {
    tree: Vec<f64>,
}

impl Fenwick {
    fn new(n: usize, initial: f64) -> Self {
        let mut tree = vec![0.0; n + 1];
        for i in 1..=n {
            tree[i] += initial;
            let parent = i + lowest_bit(i);
            if parent <= n {
                let v = tree[i];
                tree[parent] += v;
            }
        }
        Fenwick { tree }
    }

    fn add(&mut self, idx: usize, delta: f64) {
        let mut i = idx + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += lowest_bit(i);
        }
    }

    fn total(&self) -> f64 {
        let mut i = self.tree.len() - 1;
        let mut sum = 0.0;
        while i > 0 {
            sum += self.tree[i];
            i -= lowest_bit(i);
        }
        sum
    }

    /// Index whose cumulative weight range contains `target`.
    fn find(&self, mut target: f64) -> usize {
        let n = self.tree.len() - 1;
        let mut pos = 0;
        let mut step = n.next_power_of_two();
        while step > 0 {
            let next = pos + step;
            if next <= n && self.tree[next] <= target {
                pos = next;
                target -= self.tree[next];
            }
            step >>= 1;
        }
        pos.min(n - 1)
    }
}

/// Per-call stratification state: visit counts plus a Fenwick tree over the
/// start weights `1 / (1 + visits)` of the vertex's own nodes.
struct Stratification {
    visits: Vec<u64>,
    start_weights: Fenwick,
    n_real: usize,
}

impl Stratification {
    fn new(index: &WalkIndex) -> Self {
        Stratification {
            visits: vec![0; index.ids.len()],
            start_weights: Fenwick::new(index.n_real, 1.0),
            n_real: index.n_real,
        }
    }

    // Weight used in stratified mode: inverse of how often the node was
    // visited (smoothed so unvisited nodes have weight 1.0, not infinity).
    fn weight(&self, node: u32) -> f64 {
        1.0 / (1.0 + self.visits[node as usize] as f64)
    }

    fn visit(&mut self, node: u32) {
        let before = self.weight(node);
        self.visits[node as usize] += 1;
        if (node as usize) < self.n_real {
            let after = self.weight(node);
            self.start_weights.add(node as usize, after - before);
        }
    }

    fn sample_start<R: Rng>(&self, rng: &mut R) -> u32 {
        let total = self.start_weights.total();
        self.start_weights.find(rng.gen::<f64>() * total) as u32
    }
}

/// Reusable per-thread scratch space for walks.
struct Scratch {
    /// `stamp[node] == generation` marks the node as visited in the current walk.
    stamp: Vec<u32>,
    generation: u32,
    options: Vec<(u32, u32)>,
    weights: Vec<f64>,
}

impl Scratch {
    fn new(n: usize) -> Self {
        Scratch { stamp: vec![0; n], generation: 0, options: Vec::new(), weights: Vec::new() }
    }
}

// Pick an index with probability proportional to its weight.
fn weighted_pick_index<R: Rng>(weights: &[f64], rng: &mut R) -> usize {
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return 0;
    }
    let mut target = rng.gen::<f64>() * total;
    for (i, w) in weights.iter().enumerate() {
        target -= w;
        if target < 0.0 {
            return i;
        }
    }
    weights.len() - 1
}

// Simple random walk that embraces randomness without backtracking.
#[allow(clippy::too_many_arguments)]
fn perform_walk<R: Rng>(
    index: &WalkIndex,
    start: u32,
    max_length: usize,
    allow_revisit: bool,
    include_edge_types: bool,
    mut strat: Option<&mut Stratification>,
    scratch: &mut Scratch,
    rng: &mut R,
) -> Walk {
    let mut walk_nodes = Vec::with_capacity(max_length);
    let mut walk_edges = Vec::new();
    scratch.generation = scratch.generation.wrapping_add(1);
    if scratch.generation == 0 {
        scratch.stamp.iter_mut().for_each(|s| *s = 0);
        scratch.generation = 1;
    }
    let generation = scratch.generation;
    let mut current = start;

    for _ in 0..max_length {
        walk_nodes.push(current);
        if let Some(s) = strat.as_deref_mut() {
            s.visit(current);
        }
        if !allow_revisit {
            scratch.stamp[current as usize] = generation;
        }

        let edges = &index.adj[current as usize];
        let chosen = match strat.as_deref() {
            None if allow_revisit => {
                if edges.is_empty() {
                    break;
                }
                edges[rng.gen_range(0..edges.len())]
            }
            _ => {
                scratch.options.clear();
                scratch.options.extend(
                    edges.iter().copied().filter(|(t, _)| allow_revisit || scratch.stamp[*t as usize] != generation),
                );
                if scratch.options.is_empty() {
                    break;
                }
                match strat.as_deref() {
                    // Stratified: bias towards the least-visited candidates.
                    Some(s) => {
                        scratch.weights.clear();
                        scratch.weights.extend(scratch.options.iter().map(|(t, _)| s.weight(*t)));
                        scratch.options[weighted_pick_index(&scratch.weights, rng)]
                    }
                    None => scratch.options[rng.gen_range(0..scratch.options.len())],
                }
            }
        };

        if include_edge_types {
            walk_edges.push(chosen.1);
        }
        current = chosen.0;
    }

    Walk { nodes: walk_nodes, edges: walk_edges }
}

fn validate_params<N, E>(g: &Graph<N, E>, start_node_id: Option<&str>, opts: &WalkOptions) -> Result<(), GraphError> {
    let invalid = |msg: &str| Err(GraphError::InvalidArgument(msg.to_string()));
    match start_node_id {
        Some(id) => {
            if !g.contains_node(id) {
                return Err(GraphError::InvalidArgument(format!("Start node with id '{}' not found", id)));
            }
        }
        None => {
            if !opts.stratified {
                return invalid("start_node_id may only be None when stratified=True");
            }
            if g.is_empty() {
                return invalid("Cannot perform stratified walks on an empty graph");
            }
        }
    }
    if opts.max_length == 0 {
        return invalid("max_length must be greater than 0");
    }
    if opts.min_length > opts.max_length {
        return invalid("min_length cannot be greater than max_length");
    }
    Ok(())
}

// Remove duplicate walks, keeping the first occurrence. Keys are index
// sequences, so ids containing separators can never collide.
fn deduplicate_walks(walks: Vec<Walk>, include_edges: bool) -> Vec<Walk> {
    let mut seen: HashSet<Vec<u32>> = HashSet::with_capacity(walks.len());
    walks
        .into_iter()
        .filter(|walk| {
            let mut key = walk.nodes.clone();
            if include_edges {
                key.push(u32::MAX);
                key.extend_from_slice(&walk.edges);
            }
            seen.insert(key)
        })
        .collect()
}

fn chunk_seed(base: u64, chunk: usize) -> u64 {
    // SplitMix64 step so neighbouring chunks get unrelated streams.
    let mut z = base.wrapping_add((chunk as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A validated batch of walks, detached from the graph: `run` does the
/// walking and `items` turns a walk into ids.
pub struct WalkPlan {
    index: WalkIndex,
    start: Option<u32>,
    opts: WalkOptions,
}

/// Validate the options and index the graph for walking from `start_node_id`
/// (`None` only with `stratified`).
pub fn plan<N, E, X>(g: &Graph<N, E>, start_node_id: Option<&str>, opts: WalkOptions) -> Result<WalkPlan, X>
where
    E: Attributes,
    X: From<GraphError> + From<E::Error>,
{
    validate_params(g, start_node_id, &opts)?;
    let index = WalkIndex::build(g, opts.include_edge_types, &opts.edge_type_field)?;
    let start = start_node_id.map(|id| index.ids.iter().position(|x| x == id).expect("validated") as u32);
    Ok(WalkPlan { index, start, opts })
}

impl WalkPlan {
    /// Perform the walks; duplicates are removed (first occurrence kept).
    pub fn run(&self) -> Vec<Walk> {
        self.run_attempts(self.opts.num_attempts).0
    }

    /// [`run`](Self::run) under a [`Budget`]. A walk's work is bounded in
    /// advance (at most `max_length` nodes), so `max_visited` caps the
    /// number of attempts at `max_visited / max_length`; the walks made are
    /// the first ones [`run`](Self::run) would make with the same seed.
    /// `max_results` keeps the first walks (after duplicates are removed).
    /// In [`OnLimit::Error`](crate::OnLimit::Error) mode, attempts that
    /// don't fit fail before any walking. `visited` counts the nodes the
    /// walks passed through (walks shorter than `min_length` included).
    pub fn run_limited(&self, budget: Budget) -> Result<Limited<Vec<Walk>>, GraphError> {
        let mut meter = Meter::new(budget);
        let mut attempts = self.opts.num_attempts;
        if let Some(max) = budget.max_visited {
            let fit = max / self.opts.max_length;
            if fit < attempts {
                attempts = fit;
                meter.stopped();
                if budget.on_limit == OnLimit::Error {
                    return meter.finish(Vec::new());
                }
            }
        }
        let (mut walks, visited) = self.run_attempts(attempts);
        meter.add_visited(visited);
        let mut kept = 0;
        while kept < walks.len() && meter.produce() {
            kept += 1;
        }
        walks.truncate(kept);
        meter.finish(walks)
    }

    /// The first `num_attempts` attempts, deduplicated, and the number of
    /// nodes walked through.
    fn run_attempts(&self, num_attempts: usize) -> (Vec<Walk>, usize) {
        let index = &self.index;
        let opts = &self.opts;
        let fixed_start = self.start;
        let base_seed: u64 = opts.seed.unwrap_or_else(rand::random);
        let (max_length, allow_revisit, include_edges) = (opts.max_length, opts.allow_revisit, opts.include_edge_types);
        let stop = crate::cancel::stop();

        let (walks, visited): (Vec<Walk>, usize) = if opts.stratified {
            // Visit counts persist across all attempts so that later walks are
            // steered towards nodes that earlier walks neglected; this is
            // inherently sequential.
            let mut rng = StdRng::seed_from_u64(base_seed);
            let mut strat = Stratification::new(index);
            let mut scratch = Scratch::new(index.ids.len());
            let mut walks = Vec::with_capacity(num_attempts);
            let mut visited = 0;
            for _ in 0..num_attempts {
                if stop.requested() {
                    break;
                }
                let start = fixed_start.unwrap_or_else(|| strat.sample_start(&mut rng));
                let walk = perform_walk(
                    index,
                    start,
                    max_length,
                    allow_revisit,
                    include_edges,
                    Some(&mut strat),
                    &mut scratch,
                    &mut rng,
                );
                visited += walk.nodes.len();
                if walk.nodes.len() >= opts.min_length {
                    walks.push(walk);
                }
            }
            (walks, visited)
        } else {
            let start = fixed_start.expect("validated: start node is required");
            let n_chunks = num_attempts.div_ceil(CHUNK);
            let per_chunk: Vec<(Vec<Walk>, usize)> = (0..n_chunks)
                .into_par_iter()
                .map(|chunk| {
                    if stop.requested() {
                        return (Vec::new(), 0);
                    }
                    let mut rng = StdRng::seed_from_u64(chunk_seed(base_seed, chunk));
                    let mut scratch = Scratch::new(index.ids.len());
                    let attempts = CHUNK.min(num_attempts - chunk * CHUNK);
                    let mut walks = Vec::with_capacity(attempts);
                    let mut visited = 0;
                    for _ in 0..attempts {
                        let walk = perform_walk(
                            index,
                            start,
                            max_length,
                            allow_revisit,
                            include_edges,
                            None,
                            &mut scratch,
                            &mut rng,
                        );
                        visited += walk.nodes.len();
                        if walk.nodes.len() >= opts.min_length {
                            walks.push(walk);
                        }
                    }
                    (walks, visited)
                })
                .collect();
            let visited = per_chunk.iter().map(|(_, v)| v).sum();
            (per_chunk.into_iter().flat_map(|(w, _)| w).collect(), visited)
        };

        (deduplicate_walks(walks, include_edges), visited)
    }

    /// The walk as node ids, or, with `include_edge_types`, alternating node
    /// ids and edge types (`[node, type, node, type, ..., node]`).
    pub fn items<'a>(&'a self, walk: &'a Walk) -> impl Iterator<Item = &'a str> + 'a {
        let index = &self.index;
        walk.nodes.iter().enumerate().flat_map(move |(i, n)| {
            let edge = walk.edges.get(i).map(|t| index.types[*t as usize].as_str());
            std::iter::once(index.ids[*n as usize].as_str()).chain(edge)
        })
    }
}

/// Random walks from `start_node_id` (see [`WalkOptions`]), as lists of ids.
pub fn random_walks<N, E, X>(
    g: &Graph<N, E>,
    start_node_id: Option<&str>,
    opts: WalkOptions,
) -> Result<Vec<Vec<String>>, X>
where
    E: Attributes,
    X: From<GraphError> + From<E::Error>,
{
    let plan = plan::<N, E, X>(g, start_node_id, opts)?;
    Ok(plan.run().iter().map(|w| plan.items(w).map(str::to_owned).collect()).collect())
}
