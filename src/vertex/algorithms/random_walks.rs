// vertex/algorithms/random_walks.rs

use pyo3::prelude::*;
use pyo3::types::PyList;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use super::super::core::Vertex;
use crate::gc_pause::GcPause;

/// Number of walk attempts handled by one RNG stream in uniform mode. Chunks
/// are processed in parallel; because the chunking (and each chunk's seed) is
/// independent of the thread count, a given `seed` always yields the same
/// walks.
const CHUNK: usize = 256;

// A walk as indices into `WalkIndex::ids` / `WalkIndex::types`.
struct Walk {
    nodes: Vec<u32>,
    edges: Vec<u32>, // Edge type indices between nodes
}

/// Compact, GIL-free adjacency representation of the graph built once per
/// call. Walks run entirely on this index.
struct WalkIndex {
    /// Node ids; the first `n_real` entries are the vertex's own nodes, the
    /// rest are edge targets that are not part of the vertex (walks may step
    /// onto them and end there, as before).
    ids: Vec<String>,
    n_real: usize,
    /// Outgoing neighbours as (target index, edge type index).
    adj: Vec<Vec<(u32, u32)>>,
    types: Vec<String>,
}

impl WalkIndex {
    fn build(py: Python<'_>, vertex: &Vertex, include_edges: bool, type_field: &str) -> Self {
        let mut ids: Vec<String> = vertex.nodes.keys().cloned().collect();
        let n_real = ids.len();
        let mut index: HashMap<String, u32> =
            ids.iter().enumerate().map(|(i, id)| (id.clone(), i as u32)).collect();
        let mut types: Vec<String> = Vec::new();
        let mut type_index: HashMap<String, u32> = HashMap::new();
        let mut adj: Vec<Vec<(u32, u32)>> = Vec::with_capacity(n_real);

        for i in 0..n_real {
            let node = vertex.nodes[&ids[i]].borrow(py);
            let mut out = Vec::with_capacity(node.edges.len());
            for edge in &node.edges {
                let e = edge.borrow(py);
                let to_id = e.to_node.borrow(py).id.clone();
                let target = match index.get(&to_id) {
                    Some(&t) => t,
                    None => {
                        let t = ids.len() as u32;
                        ids.push(to_id.clone());
                        index.insert(to_id, t);
                        t
                    }
                };
                let type_idx = if include_edges {
                    let name = e
                        .attr
                        .get(type_field)
                        .and_then(|v| v.extract::<String>(py).ok())
                        .unwrap_or_else(|| "unknown".to_string());
                    *type_index.entry(name.clone()).or_insert_with(|| {
                        types.push(name);
                        (types.len() - 1) as u32
                    })
                } else {
                    0
                };
                out.push((target, type_idx));
            }
            adj.push(out);
        }
        // Targets outside the vertex have no outgoing edges.
        adj.resize_with(ids.len(), Vec::new);

        WalkIndex { ids, n_real, adj, types }
    }
}

/// Fenwick (binary indexed) tree over f64 weights: O(log n) point update and
/// O(log n) sampling proportional to weight.
struct Fenwick {
    tree: Vec<f64>,
}

impl Fenwick {
    fn new(n: usize, initial: f64) -> Self {
        let mut tree = vec![0.0; n + 1];
        for i in 1..=n {
            tree[i] += initial;
            let parent = i + (i & i.wrapping_neg());
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
            i += i & i.wrapping_neg();
        }
    }

    fn total(&self) -> f64 {
        let mut i = self.tree.len() - 1;
        let mut sum = 0.0;
        while i > 0 {
            sum += self.tree[i];
            i -= i & i.wrapping_neg();
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
                    edges
                        .iter()
                        .copied()
                        .filter(|(t, _)| allow_revisit || scratch.stamp[*t as usize] != generation),
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

fn validate_params(
    vertex: &Vertex,
    start_node_id: &Option<String>,
    max_length: usize,
    min_len: usize,
    stratified: bool,
) -> PyResult<()> {
    match start_node_id {
        Some(id) => {
            if !vertex.nodes.contains_key(id) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    format!("Start node with id '{}' not found", id),
                ));
            }
        }
        None => {
            if !stratified {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "start_node_id may only be None when stratified=True",
                ));
            }
            if vertex.nodes.is_empty() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Cannot perform stratified walks on an empty graph",
                ));
            }
        }
    }

    if max_length == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "max_length must be greater than 0",
        ));
    }

    if min_len > max_length {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "min_length cannot be greater than max_length",
        ));
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

pub fn random_walks(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: Option<String>,
    max_length: usize,
    min_length: Option<usize>,
    num_attempts: usize,
    allow_revisit: Option<bool>,
    include_edge_types: Option<bool>,
    edge_type_field: Option<String>,
    stratified: Option<bool>,
    seed: Option<u64>,
) -> PyResult<Py<PyList>> {
    let min_len = min_length.unwrap_or(1);
    let allow_revisit_nodes = allow_revisit.unwrap_or(false);
    let include_edges = include_edge_types.unwrap_or(false);
    let type_field = edge_type_field.unwrap_or_else(|| "type".to_string());
    let stratified_mode = stratified.unwrap_or(false);

    validate_params(vertex, &start_node_id, max_length, min_len, stratified_mode)?;

    let index = WalkIndex::build(py, vertex, include_edges, &type_field);
    let fixed_start: Option<u32> = start_node_id
        .as_ref()
        .map(|id| index.ids.iter().position(|x| x == id).unwrap() as u32);
    let base_seed: u64 = seed.unwrap_or_else(rand::random);

    // Walks run on the pure-Rust index, so the GIL is released meanwhile.
    let walks: Vec<Walk> = py.allow_threads(|| {
        if stratified_mode {
            // Visit counts persist across all attempts of this call so that
            // later walks are steered towards nodes that earlier walks
            // neglected; this is inherently sequential.
            let mut rng = StdRng::seed_from_u64(base_seed);
            let mut strat = Stratification::new(&index);
            let mut scratch = Scratch::new(index.ids.len());
            let mut walks = Vec::with_capacity(num_attempts);
            for _ in 0..num_attempts {
                let start = fixed_start.unwrap_or_else(|| strat.sample_start(&mut rng));
                let walk = perform_walk(
                    &index, start, max_length, allow_revisit_nodes, include_edges,
                    Some(&mut strat), &mut scratch, &mut rng,
                );
                if walk.nodes.len() >= min_len {
                    walks.push(walk);
                }
            }
            walks
        } else {
            let start = fixed_start.expect("validated: start node is required");
            let n_chunks = (num_attempts + CHUNK - 1) / CHUNK;
            let per_chunk: Vec<Vec<Walk>> = (0..n_chunks)
                .into_par_iter()
                .map(|chunk| {
                    let mut rng = StdRng::seed_from_u64(chunk_seed(base_seed, chunk));
                    let mut scratch = Scratch::new(index.ids.len());
                    let attempts = CHUNK.min(num_attempts - chunk * CHUNK);
                    let mut walks = Vec::with_capacity(attempts);
                    for _ in 0..attempts {
                        let walk = perform_walk(
                            &index, start, max_length, allow_revisit_nodes, include_edges,
                            None, &mut scratch, &mut rng,
                        );
                        if walk.nodes.len() >= min_len {
                            walks.push(walk);
                        }
                    }
                    walks
                })
                .collect();
            per_chunk.into_iter().flatten().collect()
        }
    });

    let unique_walks = deduplicate_walks(walks, include_edges);

    // Convert to Python list
    let _gc = GcPause::new(py);
    let result = PyList::empty(py);
    for walk in unique_walks {
        let py_walk = if include_edges {
            // Return list of [node, edge_type, node, edge_type, ...] format
            let mut items: Vec<&str> = Vec::with_capacity(walk.nodes.len() * 2);
            for (i, n) in walk.nodes.iter().enumerate() {
                items.push(&index.ids[*n as usize]);
                if let Some(t) = walk.edges.get(i) {
                    items.push(&index.types[*t as usize]);
                }
            }
            PyList::new(py, items)?
        } else {
            PyList::new(py, walk.nodes.iter().map(|n| index.ids[*n as usize].as_str()))?
        };
        result.append(py_walk)?;
    }

    Ok(result.into())
}
