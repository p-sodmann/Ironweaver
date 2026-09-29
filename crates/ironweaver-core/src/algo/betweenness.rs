// algo/betweenness.rs
//
// Betweenness centrality (Brandes), exact or estimated from a sample of
// sources, in parallel over the sources. Same results (and scaling) as
// `networkx.betweenness_centrality`: shortest paths are counted on the
// simple graph (parallel edges collapse to the lightest, self-loops are
// ignored), and a `Both` projection counts as undirected.

use rand::rngs::StdRng;
use rand::seq::index::sample;
use rand::SeedableRng;

use super::sssp::{Search, SimpleRow};
use super::{check_nodes, ordered_sum};
use crate::{Direction, GraphError, Projection};

/// Betweenness options; the defaults match networkx.
#[derive(Clone, Debug)]
pub struct Betweenness {
    /// Divide by the number of (source, target) pairs.
    pub normalized: bool,
    /// Count the endpoints of each path too.
    pub endpoints: bool,
    /// Use the projection's weights (it must have them); hop counts if false.
    pub weighted: bool,
    /// Only paths from these sources (dense indices, e.g. a sample; see
    /// [`sample_sources`]); the result is scaled up to estimate the full
    /// value. Every node if `None`.
    pub sources: Option<Vec<u32>>,
}

impl Default for Betweenness {
    fn default() -> Self {
        Betweenness { normalized: true, endpoints: false, weighted: false, sources: None }
    }
}

/// `k` distinct nodes chosen uniformly at random (all of them if `k >= n`),
/// in index order.
pub fn sample_sources(p: &Projection, k: usize, seed: u64) -> Vec<u32> {
    let n = p.node_count();
    if k >= n {
        return (0..n as u32).collect();
    }
    let mut picked: Vec<u32> = sample(&mut StdRng::seed_from_u64(seed), n, k).into_iter().map(|u| u as u32).collect();
    picked.sort_unstable();
    picked
}

/// Per-thread state: the search plus the path counts, predecessors and
/// dependencies of one source.
struct Brandes {
    search: Search,
    sigma: Vec<f64>,
    delta: Vec<f64>,
    /// Predecessors of `order[i]` are `preds[pred_start[i]..pred_start[i + 1]]`.
    preds: Vec<u32>,
    pred_start: Vec<usize>,
}

impl Brandes {
    fn new(n: usize) -> Self {
        Brandes {
            search: Search::new(n),
            sigma: vec![0.0; n],
            delta: vec![0.0; n],
            preds: Vec::new(),
            pred_start: Vec::new(),
        }
    }

    /// Add the dependencies of every node on `source` to `acc`.
    fn accumulate(&mut self, p: &Projection, source: u32, opts: &Betweenness, acc: &mut [f64]) {
        let s = &mut self.search;
        s.run(p, source, opts.weighted, false);
        // Path counts in order of distance: a node's predecessors are the
        // in-neighbours on a shortest path to it, all settled before it
        self.preds.clear();
        self.pred_start.clear();
        for (i, &w) in s.order.iter().enumerate() {
            self.pred_start.push(self.preds.len());
            if i == 0 {
                self.sigma[w as usize] = 1.0;
                continue;
            }
            let dw = s.dist[w as usize];
            let mut sigma = 0.0;
            for (v, weight) in SimpleRow::of(p, w, true, opts.weighted) {
                let dv = s.dist[v as usize];
                if dv < dw && dv + weight == dw {
                    sigma += self.sigma[v as usize];
                    self.preds.push(v);
                }
            }
            self.sigma[w as usize] = sigma;
        }
        self.pred_start.push(self.preds.len());

        if opts.endpoints {
            acc[source as usize] += (s.order.len() - 1) as f64;
        }
        for i in (0..s.order.len()).rev() {
            let w = s.order[i] as usize;
            let coeff = (1.0 + self.delta[w]) / self.sigma[w];
            for &v in &self.preds[self.pred_start[i]..self.pred_start[i + 1]] {
                self.delta[v as usize] += self.sigma[v as usize] * coeff;
            }
            if w != source as usize {
                acc[w] += self.delta[w] + if opts.endpoints { 1.0 } else { 0.0 };
            }
        }
        for &w in &s.order {
            self.sigma[w as usize] = 0.0;
            self.delta[w as usize] = 0.0;
        }
    }
}

/// Betweenness centrality of every node: the (scaled) number of shortest
/// paths between other nodes that pass through it. Parallel over sources;
/// the result does not depend on the number of threads.
pub fn betweenness_centrality(p: &Projection, opts: &Betweenness) -> Result<Vec<f64>, GraphError> {
    let n = p.node_count();
    if opts.weighted && !p.is_weighted() {
        return Err(GraphError::InvalidArgument(
            "weighted betweenness needs a projection with edge weights (project with weight=...)".into(),
        ));
    }
    let all: Vec<u32>;
    let sources: &[u32] = match &opts.sources {
        Some(s) => {
            check_nodes(p, s)?;
            if s.is_empty() {
                return Err(GraphError::InvalidArgument("betweenness needs at least one source".into()));
            }
            s
        }
        None => {
            all = (0..n as u32).collect();
            &all
        }
    };
    let stop = crate::cancel::stop();
    let mut bc = ordered_sum(
        n,
        sources,
        || Brandes::new(n),
        |b, &s, acc| {
            if !stop.requested() {
                b.accumulate(p, s, opts, acc)
            }
        },
    );

    // Scaling, as networkx's `_rescale`
    let sampled = opts.sources.as_ref().filter(|s| s.len() != n);
    let big_n = if opts.endpoints { n } else { n.saturating_sub(1) } as f64;
    if big_n < 2.0 {
        return Ok(bc);
    }
    let k = sampled.map_or(big_n, |s| s.len() as f64);
    let correction = if p.direction() == Direction::Both { 2.0 } else { 1.0 };
    match sampled {
        Some(s) if !opts.endpoints => {
            let (source_scale, other_scale) = if opts.normalized {
                (if k > 1.0 { 1.0 / ((k - 1.0) * (big_n - 1.0)) } else { f64::NAN }, 1.0 / (k * (big_n - 1.0)))
            } else {
                (if k > 1.0 { big_n / ((k - 1.0) * correction) } else { f64::NAN }, big_n / (k * correction))
            };
            let mut is_source = vec![false; n];
            for &u in s {
                is_source[u as usize] = true;
            }
            for (u, b) in bc.iter_mut().enumerate() {
                *b *= if is_source[u] { source_scale } else { other_scale };
            }
        }
        _ => {
            let scale = if opts.normalized { 1.0 / (k * (big_n - 1.0)) } else { big_n / (k * correction) };
            if scale != 1.0 {
                bc.iter_mut().for_each(|b| *b *= scale);
            }
        }
    }
    Ok(bc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};

    /// All-pairs brute force: for every (s, t), the fraction of shortest
    /// s-t paths through v, by counting paths through the distance matrix.
    fn reference(p: &Projection, weighted: bool) -> Vec<f64> {
        let n = p.node_count();
        let mut w = vec![vec![f64::INFINITY; n]; n];
        for u in 0..n as u32 {
            for (v, wt) in SimpleRow::of(p, u, false, weighted) {
                w[u as usize][v as usize] = wt;
            }
        }
        let d: Vec<Vec<f64>> = (0..n as u32)
            .map(|s| {
                let mut search = Search::new(n);
                search.run(p, s, weighted, false);
                search.dist
            })
            .collect();
        // sigma[s][t]: number of shortest paths, by increasing distance
        let mut sigma = vec![vec![0f64; n]; n];
        for s in 0..n {
            let mut order: Vec<usize> = (0..n).filter(|&t| d[s][t].is_finite()).collect();
            order.sort_by(|&a, &b| d[s][a].total_cmp(&d[s][b]));
            for &t in &order {
                sigma[s][t] = if t == s {
                    1.0
                } else {
                    (0..n).filter(|&v| v != t && d[s][v] + w[v][t] == d[s][t]).map(|v| sigma[s][v]).sum()
                };
            }
        }
        let mut bc = vec![0.0; n];
        for s in 0..n {
            for t in 0..n {
                if s == t || !d[s][t].is_finite() {
                    continue;
                }
                for v in 0..n {
                    if v != s && v != t && d[s][v] + d[v][t] == d[s][t] {
                        bc[v] += sigma[s][v] * sigma[v][t] / sigma[s][t];
                    }
                }
            }
        }
        bc
    }

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9 * (1.0 + y.abs()))
    }

    #[test]
    fn matches_brute_force() {
        for (seed, dir) in sweep() {
            for weighted in [false, true] {
                let p = random(seed, 25, 30 + 4 * seed as usize, dir, weighted);
                let opts = Betweenness { normalized: false, weighted, ..Default::default() };
                let got = betweenness_centrality(&p, &opts).unwrap();
                let mut want = reference(&p, weighted);
                if dir == Direction::Both {
                    want.iter_mut().for_each(|x| *x /= 2.0);
                }
                assert!(close(&got, &want), "seed {seed} {dir:?} weighted {weighted}");
                // Normalised: divided by (n - 1)(n - 2), times 2 if undirected
                let norm = betweenness_centrality(&p, &Betweenness { normalized: true, ..opts.clone() }).unwrap();
                let f = if dir == Direction::Both { 2.0 } else { 1.0 } / (24.0 * 23.0);
                assert!(close(&norm, &got.iter().map(|x| x * f).collect::<Vec<_>>()));
            }
        }
    }

    #[test]
    fn endpoints_and_sampling() {
        // Path 0 -> 1 -> 2 -> 3
        let p = from_edges(4, &[(0, 1), (1, 2), (2, 3)], Direction::Out);
        let raw = |opts: Betweenness| betweenness_centrality(&p, &Betweenness { normalized: false, ..opts }).unwrap();
        assert_eq!(raw(Betweenness::default()), [0.0, 2.0, 2.0, 0.0]);
        // Endpoints: each node also counts the paths it starts or ends
        assert_eq!(raw(Betweenness { endpoints: true, ..Default::default() }), [3.0, 5.0, 5.0, 3.0]);
        // Sampling every node is the exact result
        let all = Betweenness { sources: Some(vec![0, 1, 2, 3]), ..Default::default() };
        assert_eq!(raw(all), [0.0, 2.0, 2.0, 0.0]);
        // One source: non-sources scaled up by (n - 1) / k; sources are
        // undefined (NaN) with one source, like networkx
        let one = raw(Betweenness { sources: Some(vec![0]), ..Default::default() });
        assert!(one[0].is_nan() && one[1..] == [6.0, 3.0, 0.0]);
        assert_eq!(sample_sources(&p, 10, 1), [0, 1, 2, 3]);
        let s = sample_sources(&p, 2, 7);
        assert!(s.len() == 2 && s[0] < s[1] && s == sample_sources(&p, 2, 7));
        assert!(betweenness_centrality(&p, &Betweenness { weighted: true, ..Default::default() }).is_err());
        assert!(betweenness_centrality(&p, &Betweenness { sources: Some(vec![9]), ..Default::default() }).is_err());
        let empty = from_edges(0, &[], Direction::Out);
        assert!(betweenness_centrality(&empty, &Betweenness::default()).unwrap().is_empty());
    }
}
