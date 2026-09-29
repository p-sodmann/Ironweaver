// algo/centrality.rs

use rayon::prelude::*;

use crate::{GraphError, Projection};

/// Degree divided by `n - 1` (1.0 for every node of a graph with at most
/// one node), counting edges leaving (`incoming = false`) or entering each
/// node, one per edge. For networkx's `degree_centrality` of a directed
/// graph (in + out), use a `Both` projection.
pub fn degree_centrality(p: &Projection, incoming: bool) -> Vec<f64> {
    let n = p.node_count();
    if n <= 1 {
        return vec![1.0; n];
    }
    let mut degree = vec![0usize; n];
    for u in 0..n as u32 {
        if incoming {
            for &v in p.out_neighbors(u) {
                degree[v as usize] += 1;
            }
        } else {
            degree[u as usize] = p.out_degree(u);
        }
    }
    let scale = 1.0 / (n - 1) as f64;
    degree.into_iter().map(|d| d as f64 * scale).collect()
}

/// PageRank options; the defaults match networkx.
#[derive(Clone, Debug)]
pub struct PageRank {
    /// Damping factor: probability of following an edge rather than jumping.
    pub alpha: f64,
    /// Jump (and dangling-node) distribution by dense node index; uniform if
    /// `None`. Normalised to sum 1.
    pub personalization: Option<Vec<f64>>,
    pub max_iter: usize,
    /// Converged when the L1 change of the ranks is below `n * tol`. 0 runs
    /// exactly `max_iter` iterations and returns the ranks (as LDBC
    /// Graphalytics does).
    pub tol: f64,
}

impl Default for PageRank {
    fn default() -> Self {
        PageRank { alpha: 0.85, personalization: None, max_iter: 100, tol: 1e-6 }
    }
}

/// PageRank by power iteration, in parallel (each node pulls from its
/// in-neighbours). Edges carry their projection weight (1 if unweighted);
/// parallel edges add up. Nodes without outgoing weight ("dangling") jump
/// by the personalization distribution. Same results as `networkx.pagerank`.
/// Fails for invalid options or if it doesn't converge in `max_iter` (unless
/// `tol` is 0: then it runs exactly `max_iter` iterations).
pub fn pagerank(p: &Projection, opts: &PageRank) -> Result<Vec<f64>, GraphError> {
    let n = p.node_count();
    let alpha = opts.alpha;
    if !(0.0..=1.0).contains(&alpha) {
        return Err(GraphError::InvalidArgument(format!("alpha must be between 0 and 1, got {}", alpha)));
    }
    if opts.tol.is_nan() || opts.tol < 0.0 {
        return Err(GraphError::InvalidArgument(format!("tol must be positive (or 0), got {}", opts.tol)));
    }
    if n == 0 {
        return Ok(Vec::new());
    }
    let jump: Vec<f64> = match &opts.personalization {
        None => vec![1.0 / n as f64; n],
        Some(v) => {
            if v.len() != n {
                return Err(GraphError::InvalidArgument("personalization needs one value per node".into()));
            }
            if v.iter().any(|x| !x.is_finite() || *x < 0.0) {
                return Err(GraphError::InvalidArgument("personalization values must be non-negative numbers".into()));
            }
            let sum: f64 = v.iter().sum();
            if sum <= 0.0 {
                return Err(GraphError::InvalidArgument("personalization values must not all be zero".into()));
            }
            v.iter().map(|x| x / sum).collect()
        }
    };
    let out_weight: Vec<f64> = (0..n as u32)
        .into_par_iter()
        .map(|u| match p.out_weights(u) {
            Some(w) => w.iter().sum(),
            None => p.out_degree(u) as f64,
        })
        .collect();

    let mut x = vec![1.0 / n as f64; n];
    let mut share = vec![0f64; n];
    for _ in 0..opts.max_iter {
        // What each node passes along each unit of outgoing weight
        share.par_iter_mut().enumerate().for_each(|(u, s)| {
            *s = if out_weight[u] > 0.0 { x[u] / out_weight[u] } else { 0.0 };
        });
        let dangling: f64 = (0..n).into_par_iter().filter(|&u| out_weight[u] <= 0.0).map(|u| x[u]).sum();
        let base = alpha * dangling + (1.0 - alpha);
        let next: Vec<f64> = (0..n as u32)
            .into_par_iter()
            .with_min_len(512)
            .map(|v| {
                let from = p.in_neighbors(v);
                let pulled: f64 = match p.in_weights(v) {
                    Some(w) => from.iter().zip(w).map(|(&u, &w)| share[u as usize] * w).sum(),
                    None => from.iter().map(|&u| share[u as usize]).sum(),
                };
                alpha * pulled + base * jump[v as usize]
            })
            .collect();
        let err: f64 = next.par_iter().zip(&x).map(|(a, b)| (a - b).abs()).sum();
        x = next;
        if err < n as f64 * opts.tol {
            return Ok(x);
        }
    }
    if opts.tol == 0.0 {
        return Ok(x);
    }
    Err(GraphError::InvalidArgument(format!("pagerank did not converge in {} iterations", opts.max_iter)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::Direction;

    /// Dense reference power iteration (the networkx formulation).
    fn reference(p: &Projection, alpha: f64, jump: &[f64]) -> Vec<f64> {
        let n = p.node_count();
        let mut w = vec![vec![0f64; n]; n];
        for u in 0..n as u32 {
            let ws: Vec<f64> = p.out_weights(u).map_or(vec![1.0; p.out_degree(u)], <[f64]>::to_vec);
            for (&v, wt) in p.out_neighbors(u).iter().zip(ws) {
                w[u as usize][v as usize] += wt;
            }
        }
        let out: Vec<f64> = w.iter().map(|r| r.iter().sum()).collect();
        let mut x = vec![1.0 / n as f64; n];
        for _ in 0..1000 {
            let dangling: f64 = (0..n).filter(|&u| out[u] == 0.0).map(|u| x[u]).sum();
            let mut next: Vec<f64> = jump.iter().map(|j| (alpha * dangling + 1.0 - alpha) * j).collect();
            for u in 0..n {
                for v in 0..n {
                    if w[u][v] > 0.0 {
                        next[v] += alpha * x[u] * w[u][v] / out[u];
                    }
                }
            }
            x = next;
        }
        x
    }

    #[test]
    fn matches_dense_reference() {
        for (seed, dir) in sweep() {
            for weighted in [false, true] {
                let p = random(seed, 30, 20 + 5 * seed as usize, dir, weighted);
                let got = pagerank(&p, &PageRank { tol: 1e-12, max_iter: 1000, ..Default::default() }).unwrap();
                let want = reference(&p, 0.85, &vec![1.0 / 30.0; 30]);
                assert!((got.iter().sum::<f64>() - 1.0).abs() < 1e-9);
                for (a, b) in got.iter().zip(&want) {
                    assert!((a - b).abs() < 1e-9, "seed {seed} {dir:?}: {a} vs {b}");
                }
                let mut jump = vec![0.0; 30];
                jump[3] = 2.0;
                jump[7] = 1.0;
                let opts =
                    PageRank { personalization: Some(jump.clone()), tol: 1e-12, max_iter: 1000, ..Default::default() };
                let got = pagerank(&p, &opts).unwrap();
                let norm: Vec<f64> = jump.iter().map(|j| j / 3.0).collect();
                let want = reference(&p, 0.85, &norm);
                for (a, b) in got.iter().zip(&want) {
                    assert!((a - b).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn options_and_degrees() {
        let p = from_edges(3, &[(0, 1), (0, 2), (1, 2)], Direction::Out);
        assert_eq!(degree_centrality(&p, false), [1.0, 0.5, 0.0]);
        assert_eq!(degree_centrality(&p, true), [0.0, 0.5, 1.0]);
        assert!(pagerank(&p, &PageRank { alpha: 1.5, ..Default::default() }).is_err());
        assert!(pagerank(&p, &PageRank { personalization: Some(vec![0.0; 3]), ..Default::default() }).is_err());
        assert!(pagerank(&p, &PageRank { personalization: Some(vec![1.0; 2]), ..Default::default() }).is_err());
        assert!(pagerank(&p, &PageRank { max_iter: 1, ..Default::default() }).is_err());
        assert!(pagerank(&p, &PageRank { tol: -1.0, ..Default::default() }).is_err());
        // tol 0: exactly max_iter iterations (0: the uniform start)
        assert_eq!(pagerank(&p, &PageRank { max_iter: 0, tol: 0.0, ..Default::default() }).unwrap(), [1.0 / 3.0; 3]);
        let two = pagerank(&p, &PageRank { max_iter: 2, tol: 0.0, ..Default::default() }).unwrap();
        assert!((two.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let empty = from_edges(0, &[], Direction::Out);
        assert!(pagerank(&empty, &PageRank::default()).unwrap().is_empty());
        assert!(degree_centrality(&empty, false).is_empty());
    }
}
