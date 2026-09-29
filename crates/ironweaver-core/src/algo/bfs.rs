// algo/bfs.rs

use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{check_nodes, NONE};
use crate::{GraphError, Projection};

// Direction-optimizing switch (Beamer et al.): go bottom-up when the
// frontier's edges exceed the unexplored edges / ALPHA, back to top-down
// when the frontier shrinks below n / BETA nodes.
const ALPHA: usize = 14;
const BETA: usize = 24;

/// Hop distance from the nearest of `sources` to every node along the
/// projection's edges (`NONE` if unreachable, or farther than `max_depth`).
///
/// Level-synchronous and parallel; switches between pushing from the
/// frontier (top-down) and letting unvisited nodes look for a parent in it
/// (bottom-up), which is much faster on low-diameter graphs.
pub fn bfs_levels(p: &Projection, sources: &[u32], max_depth: Option<u32>) -> Result<Vec<u32>, GraphError> {
    check_nodes(p, sources)?;
    let n = p.node_count();
    let level: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(NONE)).collect();
    let mut frontier: Vec<u32> = Vec::new();
    for &s in sources {
        if level[s as usize].swap(0, Ordering::Relaxed) == NONE {
            frontier.push(s);
        }
    }
    let mut unexplored: usize = (0..n as u32).map(|u| p.out_degree(u)).sum();
    let mut bottom_up = false;
    let mut depth = 0u32;
    while !frontier.is_empty() && max_depth.is_none_or(|m| depth < m) {
        let frontier_edges: usize = frontier.par_iter().map(|&u| p.out_degree(u)).sum();
        if !bottom_up && frontier_edges > unexplored / ALPHA {
            bottom_up = true;
        } else if bottom_up && frontier.len() < n / BETA {
            bottom_up = false;
        }
        unexplored = unexplored.saturating_sub(frontier_edges);
        let next_level = depth + 1;
        frontier = if bottom_up {
            (0..n as u32)
                .into_par_iter()
                .with_min_len(1024)
                .filter(|&v| {
                    level[v as usize].load(Ordering::Relaxed) == NONE
                        && p.in_neighbors(v).iter().any(|&u| level[u as usize].load(Ordering::Relaxed) == depth)
                })
                .collect()
        } else {
            frontier
                .par_iter()
                .flat_map_iter(|&u| {
                    p.out_neighbors(u).iter().copied().filter(|&v| {
                        level[v as usize]
                            .compare_exchange(NONE, next_level, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok()
                    })
                })
                .collect()
        };
        if bottom_up {
            // Set after the scan, so nodes found in this round don't act as
            // parents for others in the same round
            for &v in &frontier {
                level[v as usize].store(next_level, Ordering::Relaxed);
            }
        }
        depth = next_level;
    }
    Ok(level.into_iter().map(AtomicU32::into_inner).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::Direction;
    use std::collections::VecDeque;

    fn reference(p: &Projection, sources: &[u32], max_depth: Option<u32>) -> Vec<u32> {
        let mut level = vec![NONE; p.node_count()];
        let mut queue = VecDeque::new();
        for &s in sources {
            if level[s as usize] == NONE {
                level[s as usize] = 0;
                queue.push_back(s);
            }
        }
        while let Some(u) = queue.pop_front() {
            if max_depth.is_some_and(|m| level[u as usize] >= m) {
                continue;
            }
            for &v in p.out_neighbors(u) {
                if level[v as usize] == NONE {
                    level[v as usize] = level[u as usize] + 1;
                    queue.push_back(v);
                }
            }
        }
        level
    }

    #[test]
    fn matches_sequential_bfs() {
        for (seed, dir) in sweep() {
            // Dense enough that the bottom-up steps kick in
            let p = random(seed, 3000, 30_000, dir, false);
            for (sources, depth) in [(vec![0], None), (vec![1, 2, 2, 50], None), (vec![5], Some(2)), (vec![], None)] {
                assert_eq!(bfs_levels(&p, &sources, depth).unwrap(), reference(&p, &sources, depth));
            }
        }
        let chain: Vec<(usize, usize)> = (0..999).map(|i| (i, i + 1)).collect();
        let p = from_edges(1000, &chain, Direction::Out);
        assert_eq!(bfs_levels(&p, &[0], None).unwrap()[999], 999);
        assert!(bfs_levels(&p, &[1000], None).is_err());
    }
}
