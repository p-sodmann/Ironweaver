// algo/leiden.rs
//
// Community detection by the Leiden algorithm (Traag, Waltman & van Eck,
// 2019) maximising modularity, and modularity itself. Edges are taken as
// undirected and weighted by the projection's weights (1 if unweighted);
// parallel edges add up, and a self-loop counts twice towards its node's
// degree (like networkx).
//
// Leiden repeats three steps until nothing moves:
// 1. local moving: nodes move to the neighbouring community that improves
//    modularity most, revisiting only the neighbours of nodes that moved;
// 2. refinement: each community is split into well-connected
//    subcommunities, merging singletons at random (by `randomness`) into
//    subcommunities that improve modularity;
// 3. aggregation: every subcommunity becomes one node of a smaller graph,
//    starting in the community its subcommunity belongs to.
// Refinement is what guarantees connected communities (Louvain can return
// disconnected ones). As a last step, any community that is still
// disconnected is split into its connected parts, which only raises
// modularity.

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::collections::VecDeque;

use super::{groups, NONE};
use crate::{Direction, GraphError, Projection};

/// Leiden options.
#[derive(Clone, Debug)]
pub struct Leiden {
    /// Resolution γ: higher values give more, smaller communities.
    pub resolution: f64,
    /// Randomness θ of the refinement step (> 0): higher values merge more
    /// randomly; small values nearly always take the best merge.
    pub randomness: f64,
    /// Runs of the whole algorithm, each starting from the previous
    /// result; stops early once a run changes nothing.
    pub max_iter: usize,
    pub seed: u64,
}

impl Default for Leiden {
    fn default() -> Self {
        Leiden { resolution: 1.0, randomness: 0.01, max_iter: 3, seed: 0 }
    }
}

/// A weighted undirected graph without self-loops in the adjacency; a
/// node's self-loop weight is kept apart.
struct WGraph {
    start: Vec<usize>,
    to: Vec<u32>,
    w: Vec<f64>,
    self_w: Vec<f64>,
    /// Weighted degree: incident weights, self-loops twice.
    k: Vec<f64>,
}

impl WGraph {
    fn len(&self) -> usize {
        self.self_w.len()
    }

    fn row(&self, u: u32) -> impl Iterator<Item = (u32, f64)> + '_ {
        let r = self.start[u as usize]..self.start[u as usize + 1];
        self.to[r.clone()].iter().copied().zip(self.w[r].iter().copied())
    }

    /// From one row per node: its distinct neighbours with the summed
    /// weights (symmetric: v in row u with weight w iff u in row v with w),
    /// and its self-loop weight.
    fn from_rows(rows: Vec<(Vec<(u32, f64)>, f64)>) -> WGraph {
        let mut start = Vec::with_capacity(rows.len() + 1);
        start.push(0);
        for r in &rows {
            start.push(start.last().expect("non-empty") + r.0.len());
        }
        // Parallel collects keep the order
        let to = rows.par_iter().flat_map_iter(|r| r.0.iter().map(|x| x.0)).collect();
        let w = rows.par_iter().flat_map_iter(|r| r.0.iter().map(|x| x.1)).collect();
        let k = rows.par_iter().map(|(row, s)| row.iter().map(|x| x.1).sum::<f64>() + 2.0 * s).collect();
        let self_w = rows.iter().map(|r| r.1).collect();
        WGraph { start, to, w, self_w, k }
    }

    /// The projection's edges as undirected, parallel edges summed. Rows
    /// are merged from the sorted neighbour lists, in parallel.
    fn of(p: &Projection) -> WGraph {
        let both = p.direction() == Direction::Both;
        let rows = (0..p.node_count() as u32)
            .into_par_iter()
            .with_min_len(256)
            .map(|u| {
                let entries = |to: &'_ [u32], w: Option<&'_ [f64]>| -> Vec<(u32, f64)> {
                    to.iter().enumerate().map(|(i, &v)| (v, w.map_or(1.0, |w| w[i]))).collect()
                };
                let out = entries(p.out_neighbors(u), p.out_weights(u));
                // A directed edge u -> v is in u's out-row and v's in-row;
                // an undirected projection lists it from both ends already
                let inc = if both { Vec::new() } else { entries(p.in_neighbors(u), p.in_weights(u)) };
                let mut row: Vec<(u32, f64)> = Vec::with_capacity(out.len() + inc.len());
                let mut self_w = 0.0;
                let (mut i, mut j) = (0, 0);
                while i < out.len() || j < inc.len() {
                    let from_out = j == inc.len() || (i < out.len() && out[i].0 <= inc[j].0);
                    let (v, w) = if from_out { out[i] } else { inc[j] };
                    if from_out {
                        i += 1;
                    } else {
                        j += 1;
                    }
                    if v == u {
                        // Undirected: a self-loop is listed twice in its row;
                        // directed: count it from the out-row only
                        if both {
                            self_w += w / 2.0;
                        } else if from_out {
                            self_w += w;
                        }
                    } else if row.last().is_some_and(|l| l.0 == v) {
                        row.last_mut().expect("checked").1 += w;
                    } else {
                        row.push((v, w));
                    }
                }
                (row, self_w)
            })
            .collect();
        WGraph::from_rows(rows)
    }

    /// One node per community of `labels` (`0..count`): members' edges to
    /// other communities summed, edges inside a community become its
    /// self-loop. Linear time, in parallel over communities.
    fn aggregate(&self, labels: &[u32], count: usize) -> WGraph {
        // Members of each community (counting sort)
        let mut first = vec![0usize; count + 1];
        for &l in labels {
            first[l as usize + 1] += 1;
        }
        for c in 0..count {
            first[c + 1] += first[c];
        }
        let mut fill = first.clone();
        let mut members = vec![0u32; labels.len()];
        for (u, &l) in labels.iter().enumerate() {
            members[fill[l as usize]] = u as u32;
            fill[l as usize] += 1;
        }
        let rows = (0..count as u32)
            .into_par_iter()
            .with_min_len(64)
            .map_init(
                || Tally::new(count),
                |tally, c| {
                    let (mut inside, mut self_w) = (0.0, 0.0);
                    for &u in &members[first[c as usize]..first[c as usize + 1]] {
                        self_w += self.self_w[u as usize];
                        for (v, w) in self.row(u) {
                            let cv = labels[v as usize];
                            if cv == c {
                                inside += w; // seen from both ends
                            } else {
                                tally.add(cv, w);
                            }
                        }
                    }
                    let row = tally.list.iter().map(|&d| (d, tally.sum[d as usize])).collect();
                    tally.clear();
                    (row, self_w + inside / 2.0)
                },
            )
            .collect();
        WGraph::from_rows(rows)
    }

    fn two_m(&self) -> f64 {
        self.k.iter().sum()
    }
}

/// Relabel to `0..count` in order of first appearance; returns the count.
fn compact(labels: &mut [u32]) -> usize {
    let mut count = 0u32;
    if labels.iter().all(|&l| (l as usize) < labels.len()) {
        let mut map = vec![NONE; labels.len()];
        for l in labels.iter_mut() {
            if map[*l as usize] == NONE {
                map[*l as usize] = count;
                count += 1;
            }
            *l = map[*l as usize];
        }
    } else {
        let mut map = std::collections::HashMap::new();
        for l in labels.iter_mut() {
            *l = *map.entry(*l).or_insert_with(|| {
                count += 1;
                count - 1
            });
        }
    }
    count as usize
}

/// Sums of weights from one node to neighbouring communities.
struct Tally {
    sum: Vec<f64>,
    listed: Vec<bool>,
    list: Vec<u32>,
}

impl Tally {
    fn new(n: usize) -> Self {
        Tally { sum: vec![0.0; n], listed: vec![false; n], list: Vec::new() }
    }

    fn add(&mut self, c: u32, w: f64) {
        if !self.listed[c as usize] {
            self.listed[c as usize] = true;
            self.list.push(c);
        }
        self.sum[c as usize] += w;
    }

    fn clear(&mut self) {
        for &c in &self.list {
            self.sum[c as usize] = 0.0;
            self.listed[c as usize] = false;
        }
        self.list.clear();
    }
}

/// Step 1: move nodes between communities while it improves modularity.
fn move_nodes(g: &WGraph, comm: &mut [u32], gamma: f64, rng: &mut StdRng) {
    let n = g.len();
    let two_m = g.two_m();
    let mut tot = vec![0.0; n];
    let mut size = vec![0usize; n];
    for u in 0..n {
        tot[comm[u] as usize] += g.k[u];
        size[comm[u] as usize] += 1;
    }
    let mut empty: Vec<u32> = (0..n as u32).filter(|&c| size[c as usize] == 0).collect();
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.shuffle(rng);
    let mut queue: VecDeque<u32> = order.into();
    let mut queued = vec![true; n];
    let mut tally = Tally::new(n);
    let stop = crate::cancel::stop();
    while let Some(u) = queue.pop_front() {
        if stop.requested() {
            return;
        }
        queued[u as usize] = false;
        let (cu, ku) = (comm[u as usize], g.k[u as usize]);
        for (v, w) in g.row(u) {
            tally.add(comm[v as usize], w);
        }
        tot[cu as usize] -= ku;
        size[cu as usize] -= 1;
        // Gain of joining community c (up to a constant factor)
        let gain = |c: u32, tally: &Tally| tally.sum[c as usize] - gamma * ku * tot[c as usize] / two_m;
        let (mut best, mut best_gain) = (cu, gain(cu, &tally));
        for &c in &tally.list {
            let g = gain(c, &tally);
            if g > best_gain {
                (best, best_gain) = (c, g);
            }
        }
        // Being alone gains 0
        if best_gain < 0.0 && size[cu as usize] > 0 {
            best = empty.pop().expect("a node that leaves a shared community frees a community id");
        }
        tally.clear();
        comm[u as usize] = best;
        tot[best as usize] += ku;
        size[best as usize] += 1;
        if size[cu as usize] == 0 && best != cu {
            empty.push(cu);
        }
        if best != cu {
            for (v, _) in g.row(u) {
                if !queued[v as usize] && comm[v as usize] != best {
                    queued[v as usize] = true;
                    queue.push_back(v);
                }
            }
        }
    }
}

/// Step 2: split each community into well-connected subcommunities.
fn refine(g: &WGraph, comm: &[u32], gamma: f64, theta: f64, rng: &mut StdRng) -> Vec<u32> {
    let n = g.len();
    let two_m = g.two_m();
    let mut comm_tot = vec![0.0; n];
    for u in 0..n {
        comm_tot[comm[u] as usize] += g.k[u];
    }
    let mut refined: Vec<u32> = (0..n as u32).collect();
    let mut rtot = g.k.clone();
    let mut rsize = vec![1usize; n];
    // Weight from each subcommunity to the rest of its community
    let mut ext: Vec<f64> = (0..n as u32)
        .map(|u| g.row(u).filter(|&(v, _)| comm[v as usize] == comm[u as usize]).map(|(_, w)| w).sum())
        .collect();
    let well_connected = |ext: f64, tot: f64, c: u32| ext >= gamma * tot * (comm_tot[c as usize] - tot) / two_m;

    let mut order: Vec<u32> = (0..n as u32).collect();
    order.shuffle(rng);
    let mut tally = Tally::new(n);
    let mut options: Vec<(u32, f64)> = Vec::new();
    let stop = crate::cancel::stop();
    for u in order {
        if stop.requested() {
            break;
        }
        let (c, ku) = (comm[u as usize], g.k[u as usize]);
        if rsize[refined[u as usize] as usize] != 1 || !well_connected(ext[u as usize], ku, c) {
            continue;
        }
        for (v, w) in g.row(u) {
            if comm[v as usize] == c {
                tally.add(refined[v as usize], w);
            }
        }
        // Staying alone gains 0; merging into T gains w(u, T) - γ k_u K_T / 2m
        let own = refined[u as usize];
        options.clear();
        options.push((own, 0.0));
        for &t in &tally.list {
            if t == own || !well_connected(ext[t as usize], rtot[t as usize], c) {
                continue;
            }
            let gain = tally.sum[t as usize] - gamma * ku * rtot[t as usize] / two_m;
            if gain >= 0.0 {
                options.push((t, gain));
            }
        }
        // Random choice with probability ∝ exp(gain / θ)
        let top = options.iter().map(|o| o.1).fold(0.0, f64::max);
        let weights: Vec<f64> = options.iter().map(|o| ((o.1 - top) / theta).exp()).collect();
        let mut r = rng.random::<f64>() * weights.iter().sum::<f64>();
        let mut pick = options[options.len() - 1].0;
        for (o, w) in options.iter().zip(&weights) {
            if r < *w {
                pick = o.0;
                break;
            }
            r -= w;
        }
        if pick != own {
            let to_t = tally.sum[pick as usize];
            ext[pick as usize] += ext[u as usize] - 2.0 * to_t;
            rtot[pick as usize] += ku;
            rsize[pick as usize] += 1;
            rtot[own as usize] = 0.0;
            rsize[own as usize] = 0;
            refined[u as usize] = pick;
        }
        tally.clear();
    }
    refined
}

/// Split every community into its connected parts (in `g`).
fn split_disconnected(g: &WGraph, comm: &mut [u32]) {
    let n = g.len();
    let mut part = vec![NONE; n];
    let mut next = 0u32;
    let mut stack = Vec::new();
    for s in 0..n as u32 {
        if part[s as usize] != NONE {
            continue;
        }
        part[s as usize] = next;
        stack.push(s);
        while let Some(u) = stack.pop() {
            for (v, _) in g.row(u) {
                if part[v as usize] == NONE && comm[v as usize] == comm[u as usize] {
                    part[v as usize] = next;
                    stack.push(v);
                }
            }
        }
        next += 1;
    }
    comm.copy_from_slice(&part);
}

/// Communities found by the Leiden algorithm (largest first; members in
/// index order). Randomised; the same seed gives the same result.
pub fn leiden(p: &Projection, opts: &Leiden) -> Result<Vec<Vec<u32>>, GraphError> {
    if !(opts.resolution >= 0.0 && opts.resolution.is_finite()) {
        return Err(GraphError::InvalidArgument(format!("resolution must be >= 0, got {}", opts.resolution)));
    }
    if !(opts.randomness > 0.0 && opts.randomness.is_finite()) {
        return Err(GraphError::InvalidArgument(format!("randomness must be > 0, got {}", opts.randomness)));
    }
    let base = WGraph::of(p);
    let n = base.len();
    if base.two_m() <= 0.0 {
        // No edges: every node on its own
        return Ok(groups(&(0..n as u32).collect::<Vec<_>>()));
    }
    let mut rng = StdRng::seed_from_u64(opts.seed);
    let mut labels: Vec<u32> = (0..n as u32).collect();
    let stop = crate::cancel::stop();
    for _ in 0..opts.max_iter.max(1) {
        if stop.requested() {
            break;
        }
        let next = run(&base, labels.clone(), opts, &mut rng);
        if next == labels {
            break;
        }
        labels = next;
    }
    Ok(groups(&labels))
}

/// One run of Leiden on `base`, starting from the partition `comm`;
/// returns the new partition, compacted in order of first appearance.
fn run(base: &WGraph, mut comm: Vec<u32>, opts: &Leiden, rng: &mut StdRng) -> Vec<u32> {
    let gamma = opts.resolution;
    // node[u]: the aggregate node original node u belongs to
    let mut node: Vec<u32> = (0..base.len() as u32).collect();
    let mut aggregated: Option<WGraph> = None;
    let stop = crate::cancel::stop();
    loop {
        let g = aggregated.as_ref().unwrap_or(base);
        move_nodes(g, &mut comm, gamma, rng);
        if stop.requested() {
            break;
        }
        if compact(&mut comm.clone()) == g.len() {
            break; // every node on its own: nothing left to merge
        }
        let mut refined = refine(g, &comm, gamma, opts.randomness, rng);
        let count = compact(&mut refined);
        if count == g.len() {
            break; // refinement merged nothing, so aggregating wouldn't shrink the graph
        }
        // The aggregate nodes start in their subcommunity's community
        let mut next_comm = vec![0u32; count];
        for u in 0..g.len() {
            next_comm[refined[u] as usize] = comm[u];
        }
        node.iter_mut().for_each(|a| *a = refined[*a as usize]);
        compact(&mut next_comm);
        aggregated = Some(g.aggregate(&refined, count));
        comm = next_comm;
    }
    let mut labels: Vec<u32> = node.iter().map(|&a| comm[a as usize]).collect();
    split_disconnected(base, &mut labels);
    compact(&mut labels);
    labels
}

/// Modularity of a partition (`labels[u]`: community of node u, any `u32`
/// values), with resolution γ: `Σ_c L_c / m − γ (K_c / 2m)²`, where `m` is
/// the total edge weight, `L_c` the weight inside community `c` and `K_c`
/// its total degree. Same as `networkx.community.modularity`; 0 for a
/// graph without edges.
pub fn modularity(p: &Projection, labels: &[u32], resolution: f64) -> Result<f64, GraphError> {
    if labels.len() != p.node_count() {
        return Err(GraphError::InvalidArgument("modularity needs one community per node".into()));
    }
    let g = WGraph::of(p);
    let two_m = g.two_m();
    if two_m <= 0.0 {
        return Ok(0.0);
    }
    let mut labels = labels.to_vec();
    let count = compact(&mut labels);
    let (mut inside, mut tot) = (vec![0.0; count], vec![0.0; count]);
    for u in 0..g.len() as u32 {
        let c = labels[u as usize] as usize;
        tot[c] += g.k[u as usize];
        inside[c] += g.self_w[u as usize];
        for (v, w) in g.row(u) {
            if u < v && labels[v as usize] as usize == c {
                inside[c] += w;
            }
        }
    }
    let m = two_m / 2.0;
    Ok(inside.iter().zip(&tot).map(|(l, k)| l / m - resolution * (k / two_m) * (k / two_m)).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};

    fn labels_of(groups: &[Vec<u32>], n: usize) -> Vec<u32> {
        let mut labels = vec![NONE; n];
        for (i, g) in groups.iter().enumerate() {
            for &u in g {
                assert_eq!(labels[u as usize], NONE, "node in two communities");
                labels[u as usize] = i as u32;
            }
        }
        assert!(labels.iter().all(|&l| l != NONE));
        labels
    }

    /// Two cliques of `k` joined by one edge, plus an isolated node.
    fn cliques(k: usize) -> Projection {
        let mut edges = Vec::new();
        for base in [0, k] {
            for a in 0..k {
                for b in a + 1..k {
                    edges.push((base + a, base + b));
                }
            }
        }
        edges.push((k - 1, k));
        from_edges(2 * k + 1, &edges, Direction::Out)
    }

    #[test]
    fn finds_cliques() {
        let p = cliques(6);
        for seed in 0..5 {
            let got = leiden(&p, &Leiden { seed, ..Default::default() }).unwrap();
            assert_eq!(got, [(0..6).collect::<Vec<u32>>(), (6..12).collect(), vec![12]]);
        }
        // Resolution 0: everything connected in one community
        let one = leiden(&p, &Leiden { resolution: 0.0, ..Default::default() }).unwrap();
        assert_eq!(one.len(), 2);
    }

    #[test]
    fn modularity_by_hand() {
        let p = from_edges(4, &[(0, 1), (2, 3), (1, 2)], Direction::Out);
        // m = 3; communities {0,1}, {2,3}: L = 1 each, K = 3 each
        let q = modularity(&p, &[0, 0, 1, 1], 1.0).unwrap();
        assert!((q - 2.0 * (1.0 / 3.0 - 0.25)).abs() < 1e-12);
        let both = from_edges(4, &[(0, 1), (2, 3), (1, 2)], Direction::Both);
        assert!((modularity(&both, &[5, 5, 9, 9], 1.0).unwrap() - q).abs() < 1e-12);
        // Self-loops count once inside, twice in the degree
        let looped = from_edges(2, &[(0, 0), (0, 1)], Direction::Both);
        let want = 2.0 / 2.0 - (4.0f64 / 4.0).powi(2);
        assert!((modularity(&looped, &[0, 0], 1.0).unwrap() - want).abs() < 1e-12);
        assert!(modularity(&p, &[0], 1.0).is_err());
    }

    #[test]
    fn random_graphs() {
        for (seed, dir) in sweep() {
            let p = random(seed, 60, 150, dir, seed % 2 == 0);
            let got = leiden(&p, &Leiden { seed, ..Default::default() }).unwrap();
            let labels = labels_of(&got, 60);
            let q = modularity(&p, &labels, 1.0).unwrap();
            // Better than every node alone and than one community per
            // weakly connected component
            let alone = modularity(&p, &(0..60).collect::<Vec<_>>(), 1.0).unwrap();
            let wcc = crate::algo::weakly_connected_components(&p);
            let by_component = modularity(&p, &labels_of(&wcc, 60), 1.0).unwrap();
            assert!(q > alone && q >= by_component - 1e-12, "{q} {alone} {by_component}");
            // Communities are connected
            let g = WGraph::of(&p);
            let mut split = labels.clone();
            split_disconnected(&g, &mut split);
            assert_eq!(compact(&mut split), got.len());
            // Deterministic for a seed
            assert_eq!(got, leiden(&p, &Leiden { seed, ..Default::default() }).unwrap());
        }
        assert!(leiden(&cliques(3), &Leiden { randomness: 0.0, ..Default::default() }).is_err());
        assert!(leiden(&from_edges(0, &[], Direction::Out), &Leiden::default()).unwrap().is_empty());
        assert_eq!(leiden(&from_edges(2, &[], Direction::Out), &Leiden::default()).unwrap(), [vec![0], vec![1]]);
    }

    /// Modularity of `labels` on a WGraph, from the definition.
    fn q(g: &WGraph, labels: &[u32]) -> f64 {
        let two_m = g.two_m();
        let mut inside = std::collections::HashMap::new();
        let mut tot = std::collections::HashMap::new();
        for u in 0..g.len() as u32 {
            let c = labels[u as usize];
            *tot.entry(c).or_insert(0.0) += g.k[u as usize];
            *inside.entry(c).or_insert(0.0) += g.self_w[u as usize];
            for (v, w) in g.row(u) {
                if u < v && labels[v as usize] == c {
                    *inside.entry(c).or_insert(0.0) += w;
                }
            }
        }
        tot.iter().map(|(c, k)| inside.get(c).unwrap_or(&0.0) / (two_m / 2.0) - (k / two_m).powi(2)).sum()
    }

    #[test]
    fn aggregation_keeps_weights_and_modularity() {
        for (seed, dir) in sweep() {
            let p = random(seed, 40, 120, dir, seed % 2 == 0);
            let g = WGraph::of(&p);
            // Symmetric rows, weights preserved
            for u in 0..g.len() as u32 {
                for (v, w) in g.row(u) {
                    assert!(g.row(v).any(|(x, y)| x == u && y == w));
                }
            }
            let mut labels: Vec<u32> = (0..40).map(|u| (u * 7 + seed as u32) % 9).collect();
            let count = compact(&mut labels);
            let agg = g.aggregate(&labels, count);
            assert!((agg.two_m() - g.two_m()).abs() < 1e-9);
            let singletons: Vec<u32> = (0..count as u32).collect();
            assert!((q(&agg, &singletons) - q(&g, &labels)).abs() < 1e-12);
            // Coarser partitions of the aggregate match the base graph too
            let halves: Vec<u32> = (0..count as u32).map(|c| c % 2).collect();
            let lifted: Vec<u32> = labels.iter().map(|&l| l % 2).collect();
            assert!((q(&agg, &halves) - q(&g, &lifted)).abs() < 1e-12);
        }
    }
}
