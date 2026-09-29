// algo/embedding.rs
//
// Node embeddings by FastRP (Chen et al., 2019, "Fast and Accurate Network
// Embeddings via Very Sparse Random Projection"): every node starts from a
// very sparse random vector; each iteration replaces a node's vector by the
// weighted sum of its neighbours' vectors, normalised; the embedding is a
// weighted sum of the iterations. Nodes with similar neighbourhoods get
// similar vectors. No training: a few sparse matrix products, in parallel.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;

use super::mix;
use crate::{GraphError, Projection};

/// FastRP options.
#[derive(Clone, Debug)]
pub struct FastRP {
    /// Length of each embedding.
    pub dimension: usize,
    /// Weight of each iteration's vectors in the result; its length is the
    /// number of iterations. The first iteration sees direct neighbours,
    /// the second two hops, and so on.
    pub iteration_weights: Vec<f64>,
    /// Weight of the node's own random vector in the result.
    pub self_influence: f64,
    /// Exponent β: each random vector is scaled by `degree^β` (negative
    /// values damp high-degree nodes).
    pub normalization_strength: f64,
    pub seed: u64,
}

impl Default for FastRP {
    fn default() -> Self {
        FastRP {
            dimension: 128,
            iteration_weights: vec![0.0, 1.0, 1.0],
            self_influence: 0.0,
            normalization_strength: 0.0,
            seed: 0,
        }
    }
}

fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

/// FastRP embeddings along the projection's edges (use a `Both` projection
/// to treat them as undirected), weighted by the projection's weights if it
/// has them. Returns `node_count * dimension` values, node by node.
/// Deterministic for a seed, whatever the number of threads.
pub fn fastrp(p: &Projection, opts: &FastRP) -> Result<Vec<f32>, GraphError> {
    let d = opts.dimension;
    if d == 0 {
        return Err(GraphError::InvalidArgument("dimension must be at least 1".into()));
    }
    if opts.iteration_weights.is_empty() {
        return Err(GraphError::InvalidArgument("iteration_weights needs at least one iteration".into()));
    }
    let finite = |x: f64| x.is_finite();
    if !opts.iteration_weights.iter().copied().all(finite)
        || !finite(opts.self_influence)
        || !finite(opts.normalization_strength)
    {
        return Err(GraphError::InvalidArgument("FastRP weights must be finite numbers".into()));
    }
    let n = p.node_count();

    // Very sparse random projection: ±sqrt(3) with probability 1/6 each
    let scale = (3.0f32).sqrt();
    let beta = opts.normalization_strength;
    let mut random = vec![0f32; n * d];
    random.par_chunks_mut(d).enumerate().for_each(|(u, v)| {
        let mut rng = StdRng::seed_from_u64(mix(opts.seed, u as u64));
        for x in v.iter_mut() {
            let r: f32 = rng.gen();
            *x = if r < 1.0 / 6.0 {
                scale
            } else if r < 1.0 / 3.0 {
                -scale
            } else {
                0.0
            };
        }
        let degree = p.out_degree(u as u32);
        if beta != 0.0 && degree > 0 {
            let f = (degree as f64).powf(beta) as f32;
            v.iter_mut().for_each(|x| *x *= f);
        }
    });

    let mut result: Vec<f32> = random.iter().map(|x| x * opts.self_influence as f32).collect();
    let mut previous = random;
    let mut current = vec![0f32; n * d];
    for &weight in &opts.iteration_weights {
        current.par_chunks_mut(d).enumerate().for_each(|(u, v)| {
            v.iter_mut().for_each(|x| *x = 0.0);
            let weights = p.out_weights(u as u32);
            for (i, &w) in p.out_neighbors(u as u32).iter().enumerate() {
                let f = weights.map_or(1.0, |ws| ws[i] as f32);
                let from = &previous[w as usize * d..(w as usize + 1) * d];
                v.iter_mut().zip(from).for_each(|(x, y)| *x += f * y);
            }
            normalize(v);
        });
        let w = weight as f32;
        if w != 0.0 {
            result.par_iter_mut().zip(&current).for_each(|(r, c)| *r += w * c);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::from_edges;
    use crate::Direction;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b))
    }

    #[test]
    fn similar_neighbourhoods_embed_close() {
        // Two 6-cliques joined by one edge
        let mut edges = Vec::new();
        for base in [0, 6] {
            for a in 0..6 {
                for b in a + 1..6 {
                    edges.push((base + a, base + b));
                }
            }
        }
        edges.push((5, 6));
        let p = from_edges(12, &edges, Direction::Both);
        let opts = FastRP { dimension: 64, seed: 3, ..Default::default() };
        let e = fastrp(&p, &opts).unwrap();
        assert_eq!(e.len(), 12 * 64);
        let row = |u: usize| &e[u * 64..(u + 1) * 64];
        let same = cosine(row(0), row(1));
        let other = cosine(row(0), row(10));
        assert!(same > 0.9 && other < same - 0.3, "{same} {other}");
        // Deterministic
        assert_eq!(e, fastrp(&p, &opts).unwrap());
        assert_ne!(e, fastrp(&p, &FastRP { seed: 4, ..opts.clone() }).unwrap());
    }

    #[test]
    fn options() {
        let p = from_edges(3, &[(0, 1)], Direction::Out);
        let e = fastrp(&p, &FastRP { dimension: 8, iteration_weights: vec![1.0], ..Default::default() }).unwrap();
        // Node 0 takes node 1's random vector, normalised; nodes without
        // out-edges stay zero
        let norm: f32 = e[..8].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5 || norm == 0.0);
        assert!(e[8..].iter().all(|&x| x == 0.0));
        assert!(fastrp(&p, &FastRP { dimension: 0, ..Default::default() }).is_err());
        assert!(fastrp(&p, &FastRP { iteration_weights: vec![], ..Default::default() }).is_err());
        assert!(fastrp(&p, &FastRP { self_influence: f64::NAN, ..Default::default() }).is_err());
    }
}
