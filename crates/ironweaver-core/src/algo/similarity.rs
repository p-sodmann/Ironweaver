// algo/similarity.rs
//
// Neighbourhood similarity between nodes (link prediction, "people who
// know the same people"), on the simple undirected graph underlying the
// projection (see `Undirected`): for given pairs, or the top k most similar
// nodes of every node. The scores match networkx's link-prediction
// functions (`jaccard_coefficient`, `adamic_adar_index`, ...).

use rayon::prelude::*;
use std::str::FromStr;

use super::{check_nodes, Undirected};
use crate::{GraphError, Projection};

/// A similarity score of two nodes with neighbour sets `A` and `B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Similarity {
    /// `|A ∩ B| / |A ∪ B|` (0 if both are empty).
    Jaccard,
    /// `|A ∩ B| / min(|A|, |B|)` (0 if either is empty).
    Overlap,
    /// `|A ∩ B|`.
    CommonNeighbors,
    /// Sum of `1 / ln(degree)` over the common neighbours.
    AdamicAdar,
    /// Sum of `1 / degree` over the common neighbours.
    ResourceAllocation,
    /// `|A| * |B|` (pairs only: it doesn't need common neighbours).
    PreferentialAttachment,
}

impl Similarity {
    pub const NAMES: [&'static str; 6] =
        ["jaccard", "overlap", "common_neighbors", "adamic_adar", "resource_allocation", "preferential_attachment"];

    /// What a common neighbour of degree `d` adds.
    fn weight(self, d: usize) -> f64 {
        match self {
            Similarity::AdamicAdar if d > 1 => 1.0 / (d as f64).ln(),
            Similarity::AdamicAdar => 0.0,
            Similarity::ResourceAllocation => 1.0 / d as f64,
            _ => 1.0,
        }
    }

    /// The score from the summed common-neighbour weights and the degrees.
    fn score(self, common: f64, da: usize, db: usize) -> f64 {
        match self {
            Similarity::Jaccard => {
                let union = (da + db) as f64 - common;
                if union > 0.0 {
                    common / union
                } else {
                    0.0
                }
            }
            Similarity::Overlap => {
                if da.min(db) > 0 {
                    common / da.min(db) as f64
                } else {
                    0.0
                }
            }
            Similarity::PreferentialAttachment => (da * db) as f64,
            _ => common,
        }
    }
}

impl FromStr for Similarity {
    type Err = GraphError;

    fn from_str(s: &str) -> Result<Self, GraphError> {
        Ok(match s {
            "jaccard" => Similarity::Jaccard,
            "overlap" => Similarity::Overlap,
            "common_neighbors" => Similarity::CommonNeighbors,
            "adamic_adar" => Similarity::AdamicAdar,
            "resource_allocation" => Similarity::ResourceAllocation,
            "preferential_attachment" => Similarity::PreferentialAttachment,
            other => {
                return Err(GraphError::InvalidArgument(format!(
                    "unknown similarity '{}'; expected one of: {}",
                    other,
                    Similarity::NAMES.join(", ")
                )))
            }
        })
    }
}

/// Summed weights of the common neighbours of two sorted, distinct lists.
fn common(u: &Undirected, a: &[u32], b: &[u32], metric: Similarity) -> f64 {
    let (mut i, mut j, mut sum) = (0, 0, 0.0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                sum += metric.weight(u.degree(a[i]));
                i += 1;
                j += 1;
            }
        }
    }
    sum
}

/// The similarity of each pair of nodes (dense indices), in parallel.
pub fn similarity(p: &Projection, pairs: &[(u32, u32)], metric: Similarity) -> Result<Vec<f64>, GraphError> {
    let flat: Vec<u32> = pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    check_nodes(p, &flat)?;
    let u = Undirected::of(p);
    Ok(pairs
        .par_iter()
        .with_min_len(256)
        .map(|&(a, b)| {
            let (na, nb) = (u.neighbors(a), u.neighbors(b));
            let c = if metric == Similarity::PreferentialAttachment { 0.0 } else { common(&u, na, nb, metric) };
            metric.score(c, na.len(), nb.len())
        })
        .collect())
}

/// For each of `sources` (every node if `None`), the `k` other nodes most
/// similar to it with a score above `min_score`, best first (ties: lower
/// index first). Only nodes sharing a neighbour are candidates, so
/// `PreferentialAttachment` is not available. Parallel over sources.
pub fn most_similar(
    p: &Projection,
    sources: Option<&[u32]>,
    k: usize,
    metric: Similarity,
    min_score: f64,
) -> Result<Vec<Vec<(u32, f64)>>, GraphError> {
    if metric == Similarity::PreferentialAttachment {
        return Err(GraphError::InvalidArgument(
            "preferential_attachment scores every pair; use it on pairs of nodes".into(),
        ));
    }
    if let Some(s) = sources {
        check_nodes(p, s)?;
    }
    let u = Undirected::of(p);
    let n = u.len();
    let stop = crate::cancel::stop();
    let all: Vec<u32>;
    let sources = match sources {
        Some(s) => s,
        None => {
            all = (0..n as u32).collect();
            &all
        }
    };
    Ok(sources
        .par_iter()
        .with_min_len(16)
        .map_init(
            || (vec![0f64; n], vec![false; n], Vec::<u32>::new()),
            |(sum, seen, touched), &a| {
                if stop.requested() {
                    return Vec::new();
                }
                // Common-neighbour weights with every node two hops away
                for &w in u.neighbors(a) {
                    let add = metric.weight(u.degree(w));
                    for &b in u.neighbors(w) {
                        if b != a {
                            if !seen[b as usize] {
                                seen[b as usize] = true;
                                touched.push(b);
                            }
                            sum[b as usize] += add;
                        }
                    }
                }
                let da = u.degree(a);
                let mut scored: Vec<(u32, f64)> = touched
                    .iter()
                    .map(|&b| (b, metric.score(sum[b as usize], da, u.degree(b))))
                    .filter(|&(_, s)| s > min_score)
                    .collect();
                for &b in touched.iter() {
                    sum[b as usize] = 0.0;
                    seen[b as usize] = false;
                }
                touched.clear();
                let by_score = |x: &(u32, f64), y: &(u32, f64)| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0));
                if scored.len() > k {
                    scored.select_nth_unstable_by(k, by_score);
                    scored.truncate(k);
                }
                scored.sort_unstable_by(by_score);
                scored
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::Direction;
    use std::collections::BTreeSet;

    fn sets(p: &Projection) -> Vec<BTreeSet<u32>> {
        let n = p.node_count();
        let mut s = vec![BTreeSet::new(); n];
        for a in 0..n as u32 {
            for &b in p.out_neighbors(a) {
                if a != b {
                    s[a as usize].insert(b);
                    s[b as usize].insert(a);
                }
            }
        }
        s
    }

    fn brute(s: &[BTreeSet<u32>], a: usize, b: usize, m: Similarity) -> f64 {
        let common: Vec<u32> = s[a].intersection(&s[b]).copied().collect();
        let (da, db) = (s[a].len() as f64, s[b].len() as f64);
        let c = common.len() as f64;
        match m {
            Similarity::Jaccard => {
                let union = s[a].union(&s[b]).count() as f64;
                if union == 0.0 {
                    0.0
                } else {
                    c / union
                }
            }
            Similarity::Overlap => {
                if da.min(db) == 0.0 {
                    0.0
                } else {
                    c / da.min(db)
                }
            }
            Similarity::CommonNeighbors => c,
            Similarity::AdamicAdar => common.iter().map(|&w| 1.0 / (s[w as usize].len() as f64).ln()).sum(),
            Similarity::ResourceAllocation => common.iter().map(|&w| 1.0 / s[w as usize].len() as f64).sum(),
            Similarity::PreferentialAttachment => da * db,
        }
    }

    #[test]
    fn matches_brute_force() {
        for (seed, dir) in sweep().take(9) {
            let p = random(seed, 25, 70, dir, false);
            let s = sets(&p);
            let pairs: Vec<(u32, u32)> = (0..25).flat_map(|a| (0..25).map(move |b| (a, b))).collect();
            for name in Similarity::NAMES {
                let m: Similarity = name.parse().unwrap();
                let got = similarity(&p, &pairs, m).unwrap();
                for (&(a, b), g) in pairs.iter().zip(&got) {
                    if a == b && m == Similarity::AdamicAdar {
                        continue; // degree-1 common neighbours count 0 here, 1/ln(1) there
                    }
                    let want = brute(&s, a as usize, b as usize, m);
                    assert!((g - want).abs() < 1e-12, "{name} {a} {b}: {g} vs {want}");
                }
                if m == Similarity::PreferentialAttachment {
                    continue;
                }
                let k = 4;
                let top = most_similar(&p, None, k, m, 0.0).unwrap();
                for a in 0..25usize {
                    let mut want: Vec<(u32, f64)> = (0..25u32)
                        .filter(|&b| b as usize != a && !s[a].is_disjoint(&s[b as usize]))
                        .map(|b| (b, brute(&s, a, b as usize, m)))
                        .filter(|&(_, x)| x > 0.0)
                        .collect();
                    want.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
                    want.truncate(k);
                    assert_eq!(top[a].len(), want.len());
                    for (g, w) in top[a].iter().zip(&want) {
                        assert!(g.0 == w.0 && (g.1 - w.1).abs() < 1e-12, "{name} {a}: {:?} vs {:?}", top[a], want);
                    }
                }
            }
        }
    }

    #[test]
    fn options() {
        let p = from_edges(4, &[(0, 2), (1, 2), (1, 3)], Direction::Out);
        assert_eq!(similarity(&p, &[(0, 1)], Similarity::Jaccard).unwrap(), [0.5]);
        let top = most_similar(&p, Some(&[1]), 5, Similarity::CommonNeighbors, 0.0).unwrap();
        assert_eq!(top, [vec![(0, 1.0)]]);
        assert!(most_similar(&p, None, 5, Similarity::PreferentialAttachment, 0.0).is_err());
        assert!(similarity(&p, &[(0, 7)], Similarity::Jaccard).is_err());
        assert!("cosine".parse::<Similarity>().is_err());
    }
}
