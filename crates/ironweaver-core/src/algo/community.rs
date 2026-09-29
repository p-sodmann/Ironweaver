// algo/community.rs

use rayon::prelude::*;

use super::groups;
use crate::{Direction, Projection};

/// Push the labels of the distinct nodes in a sorted list, except `skip`.
fn push_distinct(seen: &mut Vec<u32>, list: &[u32], skip: u32, labels: &[u32]) {
    let mut last = None;
    for &w in list {
        if w != skip && last != Some(w) {
            seen.push(labels[w as usize]);
        }
        last = Some(w);
    }
}

/// Communities by synchronous label propagation, the LDBC Graphalytics
/// "CDLP" rule: every node starts with its own label; in each round every
/// node takes the label most common among its neighbours (ties: the
/// smallest label, i.e. the earliest node in projection order), all at
/// once. On a directed projection a node's neighbours are its distinct in-
/// and out-neighbours counted separately, so a neighbour joined both ways
/// counts twice; on an undirected (`Both`) projection each neighbour counts
/// once. Parallel edges and self-loops don't count. Stops when no label
/// changes, or after `max_iter` rounds (synchronous updates can oscillate).
/// Deterministic. Returns the communities (largest first) and the number of
/// rounds run.
pub fn label_propagation(p: &Projection, max_iter: usize) -> (Vec<Vec<u32>>, usize) {
    let n = p.node_count();
    let directed = p.direction() != Direction::Both;
    let mut labels: Vec<u32> = (0..n as u32).collect();
    let mut rounds = 0;
    while rounds < max_iter {
        rounds += 1;
        let next: Vec<u32> = (0..n as u32)
            .into_par_iter()
            .with_min_len(256)
            .map_init(Vec::new, |seen: &mut Vec<u32>, v| {
                seen.clear();
                push_distinct(seen, p.out_neighbors(v), v, &labels);
                if directed {
                    push_distinct(seen, p.in_neighbors(v), v, &labels);
                }
                if seen.is_empty() {
                    return labels[v as usize];
                }
                seen.sort_unstable();
                // Most frequent label; ties go to the smallest (first run)
                let (mut best, mut best_count) = (seen[0], 0);
                let mut i = 0;
                while i < seen.len() {
                    let j = i + seen[i..].iter().take_while(|&&l| l == seen[i]).count();
                    if j - i > best_count {
                        (best, best_count) = (seen[i], j - i);
                    }
                    i = j;
                }
                best
            })
            .collect();
        let changed = next != labels;
        labels = next;
        if !changed {
            break;
        }
    }
    (groups(&labels), rounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::algo::weakly_connected_components;
    use crate::Direction;

    #[test]
    fn cliques_joined_by_one_edge() {
        let mut edges = Vec::new();
        for base in [0, 5] {
            for a in 0..5 {
                for b in a + 1..5 {
                    edges.push((base + a, base + b));
                }
            }
        }
        edges.push((4, 5));
        let p = from_edges(11, &edges, Direction::Out);
        let (communities, rounds) = label_propagation(&p, 20);
        assert_eq!(communities, [vec![0, 1, 2, 3, 4], vec![5, 6, 7, 8, 9], vec![10]]);
        assert!(rounds < 20);
    }

    #[test]
    fn communities_stay_within_components() {
        for (seed, dir) in sweep() {
            let p = random(seed, 50, 40 + 5 * seed as usize, dir, false);
            let (communities, _) = label_propagation(&p, 30);
            let mut component = vec![0; 50];
            for (i, c) in weakly_connected_components(&p).iter().enumerate() {
                for &v in c {
                    component[v as usize] = i;
                }
            }
            assert_eq!(communities.iter().map(Vec::len).sum::<usize>(), 50);
            for c in &communities {
                assert!(c.iter().all(|&v| component[v as usize] == component[c[0] as usize]));
            }
        }
    }

    /// The CDLP rule written out plainly.
    fn reference(p: &Projection, rounds: usize) -> Vec<Vec<u32>> {
        let n = p.node_count() as u32;
        let distinct = |list: &[u32], v: u32| -> Vec<u32> {
            let mut l: Vec<u32> = list.iter().copied().filter(|&w| w != v).collect();
            l.dedup();
            l
        };
        let mut labels: Vec<u32> = (0..n).collect();
        for _ in 0..rounds {
            let next: Vec<u32> = (0..n)
                .map(|v| {
                    let mut nb = distinct(p.out_neighbors(v), v);
                    if p.direction() != Direction::Both {
                        nb.extend(distinct(p.in_neighbors(v), v));
                    }
                    let mut counts = std::collections::BTreeMap::new();
                    for w in nb {
                        *counts.entry(labels[w as usize]).or_insert(0) += 1;
                    }
                    let best = counts.values().copied().max().unwrap_or(0);
                    counts.into_iter().find(|&(_, c)| c == best).map_or(labels[v as usize], |(l, _)| l)
                })
                .collect();
            if next == labels {
                break;
            }
            labels = next;
        }
        groups(&labels)
    }

    #[test]
    fn matches_the_rule() {
        for (seed, dir) in sweep() {
            let p = random(seed, 40, 90, dir, false);
            for rounds in [1, 2, 5, 20] {
                assert_eq!(label_propagation(&p, rounds).0, reference(&p, rounds), "seed {seed} {dir:?} {rounds}");
            }
        }
        // Directed: a neighbour joined both ways counts twice. 0 <-> 2,
        // 1 -> 0, 3 -> 0: node 0 takes label 2 (not the smallest, 1)
        let p = from_edges(5, &[(0, 2), (2, 0), (1, 0), (3, 0), (4, 2), (2, 4)], Direction::Out);
        let (g, _) = label_propagation(&p, 1);
        let with_0 = g.iter().find(|c| c.contains(&0)).unwrap();
        assert!(!with_0.contains(&1), "{g:?}");
    }
}
