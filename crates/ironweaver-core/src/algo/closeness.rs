// algo/closeness.rs
//
// Closeness and harmonic centrality: how near a node is to the others, from
// the distances *to* it (for directed projections; networkx does the same).
// One search per node, in parallel; parallel edges collapse to the lightest
// and self-loops are ignored.

use rayon::prelude::*;

use super::sssp::Search;
use crate::{GraphError, Projection};

fn per_node(p: &Projection, weighted: bool, f: impl Fn(&Search) -> f64 + Sync + Send) -> Result<Vec<f64>, GraphError> {
    if weighted && !p.is_weighted() {
        return Err(GraphError::InvalidArgument(
            "weighted closeness needs a projection with edge weights (project with weight=...)".into(),
        ));
    }
    let n = p.node_count();
    Ok((0..n as u32)
        .into_par_iter()
        .with_min_len(16)
        .map_init(
            || Search::new(n),
            |s, u| {
                // Distances to u: search against the edges
                s.run(p, u, weighted, true);
                f(s)
            },
        )
        .collect())
}

/// Closeness centrality: `(r - 1) / (sum of distances from the r - 1 nodes
/// that reach it)`, times `(r - 1) / (n - 1)` if `wf_improved` (Wasserman
/// and Faust's correction for disconnected graphs); 0 for nodes nothing
/// reaches. Same as `networkx.closeness_centrality`.
pub fn closeness_centrality(p: &Projection, weighted: bool, wf_improved: bool) -> Result<Vec<f64>, GraphError> {
    let n = p.node_count();
    per_node(p, weighted, |s| {
        let total: f64 = s.order.iter().map(|&v| s.dist[v as usize]).sum();
        let reached = (s.order.len() - 1) as f64;
        if total <= 0.0 || n <= 1 {
            return 0.0;
        }
        let c = reached / total;
        if wf_improved {
            c * reached / (n - 1) as f64
        } else {
            c
        }
    })
}

/// Harmonic centrality: the sum of `1 / distance` from every other node
/// that reaches it. Same as `networkx.harmonic_centrality`.
pub fn harmonic_centrality(p: &Projection, weighted: bool) -> Result<Vec<f64>, GraphError> {
    // fold, not sum: an empty f64 sum is -0.0
    per_node(p, weighted, |s| {
        s.order.iter().map(|&v| s.dist[v as usize]).filter(|&d| d > 0.0).fold(0.0, |acc, d| acc + 1.0 / d)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::Direction;

    /// Distances to each node by Floyd-Warshall.
    #[allow(clippy::needless_range_loop)]
    fn to_distances(p: &Projection, weighted: bool) -> Vec<Vec<f64>> {
        let n = p.node_count();
        let mut d = vec![vec![f64::INFINITY; n]; n];
        for u in 0..n {
            d[u][u] = 0.0;
            let ws = p.out_weights(u as u32).filter(|_| weighted);
            for (i, &v) in p.out_neighbors(u as u32).iter().enumerate() {
                if v as usize != u {
                    let w = ws.map_or(1.0, |w| w[i]);
                    d[u][v as usize] = d[u][v as usize].min(w);
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    if d[i][k] + d[k][j] < d[i][j] {
                        d[i][j] = d[i][k] + d[k][j];
                    }
                }
            }
        }
        d
    }

    #[test]
    fn matches_floyd_warshall() {
        for (seed, dir) in sweep() {
            for weighted in [false, true] {
                let n = 30;
                let p = random(seed, n, 40 + 3 * seed as usize, dir, weighted);
                let d = to_distances(&p, weighted);
                let close = closeness_centrality(&p, weighted, true).unwrap();
                let plain = closeness_centrality(&p, weighted, false).unwrap();
                let harmonic = harmonic_centrality(&p, weighted).unwrap();
                for v in 0..n {
                    let from: Vec<f64> = (0..n).filter(|&u| u != v && d[u][v].is_finite()).map(|u| d[u][v]).collect();
                    let total: f64 = from.iter().sum();
                    let r = from.len() as f64;
                    let want = if total > 0.0 { r / total } else { 0.0 };
                    assert!((plain[v] - want).abs() < 1e-9);
                    assert!((close[v] - want * r / (n - 1) as f64).abs() < 1e-9);
                    let h: f64 = from.iter().map(|d| 1.0 / d).sum();
                    assert!((harmonic[v] - h).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn edge_cases() {
        let p = from_edges(3, &[(0, 1), (1, 2)], Direction::Out);
        assert_eq!(closeness_centrality(&p, false, true).unwrap(), [0.0, 0.5, 2.0 / 3.0]);
        let h = harmonic_centrality(&p, false).unwrap();
        assert!(h == [0.0, 1.0, 1.5] && h[0].is_sign_positive());
        assert!(closeness_centrality(&p, true, true).is_err());
        let one = from_edges(1, &[(0, 0)], Direction::Out);
        assert_eq!(closeness_centrality(&one, false, true).unwrap(), [0.0]);
    }
}
