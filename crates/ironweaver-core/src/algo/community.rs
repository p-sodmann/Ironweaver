// algo/community.rs

use rayon::prelude::*;

use super::{groups, Undirected};
use crate::Projection;

/// Communities by synchronous label propagation (the LDBC Graphalytics
/// "CDLP" rule, edges taken as undirected): every node starts with its own
/// label; in each round every node takes the label most common among its
/// neighbours (ties: the smallest label), all at once. Stops when no label
/// changes, or after `max_iter` rounds (synchronous updates can oscillate).
/// Deterministic. Returns the communities (largest first) and the number of
/// rounds run.
pub fn label_propagation(p: &Projection, max_iter: usize) -> (Vec<Vec<u32>>, usize) {
    let u = Undirected::of(p);
    let mut labels: Vec<u32> = (0..u.len() as u32).collect();
    let mut rounds = 0;
    while rounds < max_iter {
        rounds += 1;
        let next: Vec<u32> = (0..u.len() as u32)
            .into_par_iter()
            .with_min_len(256)
            .map_init(Vec::new, |seen: &mut Vec<u32>, v| {
                let neighbors = u.neighbors(v);
                if neighbors.is_empty() {
                    return labels[v as usize];
                }
                seen.clear();
                seen.extend(neighbors.iter().map(|&w| labels[w as usize]));
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
}
