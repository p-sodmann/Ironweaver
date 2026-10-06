// algo/structure.rs
//
// Local structure on the simple undirected graph underlying a projection
// (see `Undirected`): the same results as networkx on `nx.Graph(G)` with
// self-loops removed.

use rayon::prelude::*;

use super::{intersection_size, Undirected};
use crate::Projection;

/// Triangles through each node, and each node's degree.
///
/// Each triangle is found once, from its lowest-ranked node, where nodes
/// rank by (degree, index): every node keeps only its higher-ranked
/// neighbours ("forward" lists, at most `sqrt(2m)` long), and a triangle
/// `a < b < c` (by rank) is `b` and `c` in `forward(a)` with `c` in
/// `forward(b)`. That's O(m^1.5) work however skewed the degrees are
/// (counting all neighbour pairs is quadratic in the hub degrees).
fn triangles_and_degrees(u: &Undirected) -> Vec<(u64, usize)> {
    let n = u.len();
    let higher = |a: u32, b: u32| (u.degree(b), b) > (u.degree(a), a);
    let forward: Vec<Vec<u32>> = (0..n as u32)
        .into_par_iter()
        .map(|a| u.neighbors(a).iter().copied().filter(|&b| higher(a, b)).collect())
        .collect();
    // Counts per piece of work (at most ~8 per thread, each with two
    // node-sized arrays), then summed; `mark[x] == a + 1` flags x as in
    // forward(a), so nothing is cleared between nodes
    let piece = (n / (8 * rayon::current_num_threads())).max(64);
    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("triangles", Some(n as u64));
    let counts = (0..n as u32)
        .into_par_iter()
        .with_min_len(piece)
        .fold(
            || (vec![0u64; n], vec![0u32; n]),
            |(mut count, mut mark), a| {
                report.tick(a as usize, n);
                let fa = &forward[a as usize];
                if fa.len() < 2 || stop.requested() {
                    return (count, mark);
                }
                for &b in fa {
                    mark[b as usize] = a + 1;
                }
                for &b in fa {
                    for &c in &forward[b as usize] {
                        if mark[c as usize] == a + 1 {
                            count[a as usize] += 1;
                            count[b as usize] += 1;
                            count[c as usize] += 1;
                        }
                    }
                }
                (count, mark)
            },
        )
        .map(|(count, _)| count)
        .reduce(
            || vec![0u64; n],
            |mut x, y| {
                x.iter_mut().zip(y).for_each(|(a, b)| *a += b);
                x
            },
        );
    counts.into_iter().enumerate().map(|(a, t)| (t, u.degree(a as u32))).collect()
}

/// Number of triangles each node is part of (edges taken as undirected).
pub fn triangles(p: &Projection) -> Vec<u64> {
    triangles_and_degrees(&Undirected::of(p)).into_iter().map(|(t, _)| t).collect()
}

/// Local clustering coefficient: the fraction of pairs of a node's
/// neighbours that are neighbours of each other (0 for nodes with fewer than
/// two neighbours). Edges are taken as undirected.
pub fn clustering(p: &Projection) -> Vec<f64> {
    triangles_and_degrees(&Undirected::of(p))
        .into_iter()
        .map(|(t, d)| if d < 2 { 0.0 } else { 2.0 * t as f64 / (d * (d - 1)) as f64 })
        .collect()
}

/// Local clustering coefficient of a directed graph, the LDBC Graphalytics
/// "LCC" definition: for a node with the set N of distinct neighbours (in or
/// out, not itself), the number of directed edges between members of N
/// (each direction counts, parallel edges once) divided by |N| (|N| - 1).
/// On an undirected (`Both`) projection this equals [`clustering`].
pub fn clustering_directed(p: &Projection) -> Vec<f64> {
    let u = Undirected::of(p);
    // Distinct out-neighbours without self-loops
    let out: Vec<Vec<u32>> = (0..p.node_count() as u32)
        .into_par_iter()
        .map(|v| {
            let mut l: Vec<u32> = p.out_neighbors(v).iter().copied().filter(|&w| w != v).collect();
            l.dedup();
            l
        })
        .collect();
    (0..u.len() as u32)
        .into_par_iter()
        .with_min_len(64)
        .map(|v| {
            let nb = u.neighbors(v);
            if nb.len() < 2 {
                return 0.0;
            }
            let edges: usize = nb.iter().map(|&w| intersection_size(&out[w as usize], nb)).sum();
            edges as f64 / (nb.len() * (nb.len() - 1)) as f64
        })
        .collect()
}

/// Core number of each node: the largest `k` such that the node belongs to
/// a subgraph where every node has at least `k` neighbours (edges taken as
/// undirected; Batagelj–Zaversnik, O(E)).
pub fn core_number(p: &Projection) -> Vec<u32> {
    let u = Undirected::of(p);
    let n = u.len();
    let mut degree: Vec<usize> = (0..n as u32).map(|v| u.degree(v)).collect();
    let max = degree.iter().copied().max().unwrap_or(0);
    // Nodes sorted by degree (`order`), `pos` of each node in it, and the
    // first position of each degree (`bin`)
    let mut bin = vec![0usize; max + 1];
    for &d in &degree {
        bin[d] += 1;
    }
    let mut first = 0;
    for b in bin.iter_mut() {
        let count = *b;
        *b = first;
        first += count;
    }
    let mut pos = vec![0usize; n];
    let mut order = vec![0u32; n];
    let mut next = bin.clone();
    for v in 0..n {
        pos[v] = next[degree[v]];
        order[pos[v]] = v as u32;
        next[degree[v]] += 1;
    }
    let stop = crate::cancel::stop();
    let report = crate::cancel::progress();
    report.start("core number", Some(n as u64));
    for i in 0..n {
        report.tick(i, n);
        if i.is_multiple_of(crate::cancel::TICK) && stop.requested() {
            break;
        }
        let v = order[i];
        for &w in u.neighbors(v) {
            let (w, dv) = (w as usize, degree[v as usize]);
            if degree[w] > dv {
                // Move w to the front of its degree's block, then shrink it
                let dw = degree[w];
                let front = bin[dw];
                let x = order[front] as usize;
                if x != w {
                    order.swap(front, pos[w]);
                    pos[x] = pos[w];
                    pos[w] = front;
                }
                bin[dw] += 1;
                degree[w] -= 1;
            }
        }
    }
    degree.into_iter().map(|d| d as u32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::testing::{from_edges, random, sweep};
    use crate::Direction;

    fn adjacency(p: &Projection) -> Vec<Vec<bool>> {
        let n = p.node_count();
        let mut a = vec![vec![false; n]; n];
        for u in 0..n as u32 {
            for &v in p.out_neighbors(u) {
                if u != v {
                    a[u as usize][v as usize] = true;
                    a[v as usize][u as usize] = true;
                }
            }
        }
        a
    }

    #[test]
    fn triangles_and_clustering_match_brute_force() {
        for (seed, dir) in sweep() {
            let p = random(seed, 25, 40 + 8 * seed as usize, dir, false);
            let a = adjacency(&p);
            let n = a.len();
            let t = triangles(&p);
            let c = clustering(&p);
            for x in 0..n {
                let nb: Vec<usize> = (0..n).filter(|&y| a[x][y]).collect();
                let mut want = 0;
                for (i, &y) in nb.iter().enumerate() {
                    for &z in &nb[i + 1..] {
                        want += a[y][z] as u64;
                    }
                }
                assert_eq!(t[x], want, "seed {seed} {dir:?} node {x}");
                let d = nb.len() as f64;
                let cc = if nb.len() < 2 { 0.0 } else { 2.0 * want as f64 / (d * (d - 1.0)) };
                assert!((c[x] - cc).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn core_numbers_match_peeling() {
        for (seed, dir) in sweep() {
            let p = random(seed, 40, 30 + 10 * seed as usize, dir, false);
            let a = adjacency(&p);
            let n = a.len();
            // Reference: repeatedly delete nodes of degree < k
            let mut want = vec![0u32; n];
            for k in 1..n as u32 {
                let mut alive = vec![true; n];
                loop {
                    let low: Vec<usize> = (0..n)
                        .filter(|&x| alive[x] && ((0..n).filter(|&y| alive[y] && a[x][y]).count() as u32) < k)
                        .collect();
                    if low.is_empty() {
                        break;
                    }
                    for x in low {
                        alive[x] = false;
                    }
                }
                for x in 0..n {
                    if alive[x] {
                        want[x] = k;
                    }
                }
            }
            assert_eq!(core_number(&p), want, "seed {seed} {dir:?}");
        }
    }

    #[test]
    fn directed_clustering_by_brute_force() {
        for (seed, dir) in sweep() {
            let p = random(seed, 25, 40 + 8 * seed as usize, dir, false);
            let n = p.node_count();
            let mut arc = vec![vec![false; n]; n];
            for x in 0..n as u32 {
                for &y in p.out_neighbors(x) {
                    if x != y {
                        arc[x as usize][y as usize] = true;
                    }
                }
            }
            let got = clustering_directed(&p);
            for x in 0..n {
                let nb: Vec<usize> = (0..n).filter(|&y| y != x && (arc[x][y] || arc[y][x])).collect();
                let k = nb.len();
                let e: usize = nb.iter().map(|&a| nb.iter().filter(|&&b| arc[a][b]).count()).sum();
                let want = if k < 2 { 0.0 } else { e as f64 / (k * (k - 1)) as f64 };
                assert!((got[x] - want).abs() < 1e-12, "seed {seed} {dir:?} node {x}");
            }
            if dir == Direction::Both {
                assert_eq!(got, clustering(&p));
            }
        }
    }

    #[test]
    fn small_cases() {
        // A 4-clique 0-3 with a tail 3 - 4 - 5, a self-loop on 5, parallel edges
        let edges = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3), (3, 4), (4, 5), (5, 5), (1, 0)];
        let p = from_edges(7, &edges, Direction::Out);
        assert_eq!(triangles(&p), [3, 3, 3, 3, 0, 0, 0]);
        assert_eq!(core_number(&p), [3, 3, 3, 3, 1, 1, 0]);
        assert_eq!(clustering(&p)[3], 0.5);
    }
}
