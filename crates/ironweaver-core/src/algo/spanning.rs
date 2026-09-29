// algo/spanning.rs
//
// Minimum (or maximum) spanning forest by Kruskal's algorithm, edges taken
// as undirected.

use crate::{Direction, Projection};

/// The edges of a minimum spanning forest (a spanning tree of every
/// connected component), edges taken as undirected and weighted by the
/// projection's weights (1 if unweighted); self-loops are ignored. With
/// `maximum`, the heaviest forest instead. Each edge is `(a, b, weight)`
/// with `a < b`, in the order Kruskal's algorithm picks them (by weight,
/// ties by `(a, b)`).
pub fn spanning_forest(p: &Projection, maximum: bool) -> Vec<(u32, u32, f64)> {
    let n = p.node_count() as u32;
    let both = p.direction() == Direction::Both;
    let mut edges: Vec<(u32, u32, f64)> = Vec::with_capacity(p.edge_count());
    for u in 0..n {
        let weights = p.out_weights(u);
        for (i, &v) in p.out_neighbors(u).iter().enumerate() {
            // An undirected projection lists every edge from both ends
            if u == v || (both && v < u) {
                continue;
            }
            edges.push((u.min(v), u.max(v), weights.map_or(1.0, |w| w[i])));
        }
    }
    edges.sort_by(|x, y| {
        let by_weight = if maximum { y.2.total_cmp(&x.2) } else { x.2.total_cmp(&y.2) };
        by_weight.then((x.0, x.1).cmp(&(y.0, y.1)))
    });

    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            let grand = parent[parent[x as usize] as usize];
            parent[x as usize] = grand;
            x = grand;
        }
        x
    }
    let mut parent: Vec<u32> = (0..n).collect();
    let mut forest = Vec::with_capacity((n as usize).saturating_sub(1));
    for (a, b, w) in edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra.max(rb) as usize] = ra.min(rb);
            forest.push((a, b, w));
            if forest.len() + 1 == n as usize {
                break;
            }
        }
    }
    forest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::algo::weakly_connected_components;

    /// Prim's algorithm on a dense matrix, per component: the total weight.
    fn prim_total(p: &Projection, maximum: bool) -> f64 {
        let n = p.node_count();
        let sign = if maximum { -1.0 } else { 1.0 };
        let mut w = vec![vec![f64::INFINITY; n]; n];
        for u in 0..n as u32 {
            let ws = p.out_weights(u);
            for (i, &v) in p.out_neighbors(u).iter().enumerate() {
                if u != v {
                    let x = sign * ws.map_or(1.0, |w| w[i]);
                    let (a, b) = (u as usize, v as usize);
                    w[a][b] = w[a][b].min(x);
                    w[b][a] = w[a][b];
                }
            }
        }
        let mut done = vec![false; n];
        let mut total = 0.0;
        for root in 0..n {
            if done[root] {
                continue;
            }
            let mut best = vec![f64::INFINITY; n];
            best[root] = 0.0;
            loop {
                let next =
                    (0..n).filter(|&v| !done[v] && best[v].is_finite()).min_by(|&a, &b| best[a].total_cmp(&best[b]));
                let Some(v) = next else { break };
                done[v] = true;
                total += best[v];
                for x in 0..n {
                    if !done[x] && w[v][x] < best[x] {
                        best[x] = w[v][x];
                    }
                }
            }
        }
        sign * total
    }

    #[test]
    fn matches_prim() {
        for (seed, dir) in sweep() {
            for maximum in [false, true] {
                let p = random(seed, 30, 45, dir, true);
                let forest = spanning_forest(&p, maximum);
                let total: f64 = forest.iter().map(|e| e.2).sum();
                assert!((total - prim_total(&p, maximum)).abs() < 1e-9);
                let components = weakly_connected_components(&p).len();
                assert_eq!(forest.len(), 30 - components);
                assert!(forest.iter().all(|&(a, b, _)| a < b));
            }
        }
    }

    #[test]
    fn unweighted_and_empty() {
        let p = from_edges(4, &[(0, 1), (1, 2), (2, 0), (3, 3)], crate::Direction::Out);
        assert_eq!(spanning_forest(&p, false), [(0, 1, 1.0), (0, 2, 1.0)]);
        assert!(spanning_forest(&from_edges(0, &[], crate::Direction::Out), false).is_empty());
    }
}
