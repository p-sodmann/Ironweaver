// Ties between equal-cost paths are decided by node index, never by the
// order edges were added, the order of heap pushes or the number of
// threads: walking back from the target, each step goes to the
// lowest-indexed neighbour through which a cheapest route arrives. Checked
// against a brute-force reference on graphs full of ties (grids, small
// integer weights), for `find_path` (BFS, Dijkstra, A*), the parallel batch
// (`batch::shortest_paths`, `shortest_paths_astar`) and the first path of
// `k_shortest_paths`, with the edges in different orders and with thread
// pools of different sizes.

use ironweaver_core::batch::{self, AStar, DensePath};
use ironweaver_core::pathfinding::{find_path, Coords, EdgeCost, Heuristic, Metric, PathQuery};
use ironweaver_core::{algo, Direction, Graph, GraphError, NodeIx, Projection, Record, Value};

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    fn below(&mut self, m: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % m
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i as u64 + 1) as usize);
        }
    }
}

/// A test graph: `n` nodes with 2-D coordinates, edges `(from, to, weight)`.
struct Case {
    n: usize,
    coords: Vec<f64>,
    edges: Vec<(usize, usize, f64)>,
}

/// A `w` x `h` grid with edges both ways between neighbours, entering a
/// tile costing 1 or 2 (or 1 everywhere if `unit`); many equal-cost paths.
fn grid(w: usize, h: usize, unit: bool, rng: &mut Lcg) -> Case {
    let n = w * h;
    let cost: Vec<f64> = (0..n).map(|_| if unit { 1.0 } else { 1.0 + rng.below(2) as f64 }).collect();
    let mut edges = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let u = y * w + x;
            if x + 1 < w {
                edges.push((u, u + 1, cost[u + 1]));
                edges.push((u + 1, u, cost[u]));
            }
            if y + 1 < h {
                edges.push((u, u + w, cost[u + w]));
                edges.push((u + w, u, cost[u]));
            }
        }
    }
    let coords = (0..n).flat_map(|i| [(i % w) as f64, (i / w) as f64]).collect();
    Case { n, coords, edges }
}

/// Random edges with weights 1..=3 (parallel edges and self-loops
/// included); coordinates all 0, so A* estimates are 0.
fn random(n: usize, m: usize, rng: &mut Lcg) -> Case {
    let edges = (0..m)
        .map(|_| (rng.below(n as u64) as usize, rng.below(n as u64) as usize, 1.0 + rng.below(3) as f64))
        .collect();
    Case { n, coords: vec![0.0; 2 * n], edges }
}

/// The edges as followed in `direction`.
fn arcs(case: &Case, direction: Direction) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    for &(a, b, w) in &case.edges {
        if direction != Direction::In {
            out.push((a, b, w));
        }
        if direction != Direction::Out {
            out.push((b, a, w));
        }
    }
    out
}

/// The reference: exact distances from `s` (Bellman-Ford; weights are
/// small integers, so sums are exact), then from `t` back to the
/// lowest-indexed predecessor on a cheapest route at each step.
fn reference(case: &Case, direction: Direction, unit: bool, s: usize, t: usize) -> Option<Vec<usize>> {
    let arcs: Vec<(usize, usize, f64)> =
        arcs(case, direction).into_iter().map(|(a, b, w)| (a, b, if unit { 1.0 } else { w })).collect();
    let mut dist = vec![f64::INFINITY; case.n];
    dist[s] = 0.0;
    loop {
        let mut changed = false;
        for &(a, b, w) in &arcs {
            if dist[a] + w < dist[b] {
                dist[b] = dist[a] + w;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if dist[t].is_infinite() {
        return None;
    }
    let mut path = vec![t];
    let mut v = t;
    while v != s {
        v = arcs.iter().filter(|&&(a, b, w)| b == v && a != v && dist[a] + w == dist[v]).map(|a| a.0).min().unwrap();
        path.push(v);
    }
    path.reverse();
    Some(path)
}

fn weight(w: f64) -> Record {
    Record::with_attr([("weight", Value::from(w))])
}

/// The case as a graph, nodes added in index order, edges in `order`.
fn graph(case: &Case, order: &[usize]) -> (Graph<Record, Record>, Vec<NodeIx>) {
    let mut g = Graph::new();
    let ix: Vec<NodeIx> = (0..case.n)
        .map(|i| {
            let attrs = [("x", Value::from(case.coords[2 * i])), ("y", Value::from(case.coords[2 * i + 1]))];
            g.add_node(i.to_string(), Record::with_attr(attrs)).unwrap()
        })
        .collect();
    for &k in order {
        let (a, b, w) = case.edges[k];
        g.add_edge(ix[a], ix[b], weight(w)).unwrap();
    }
    (g, ix)
}

/// The case as a projection from edge arrays, edges in `order`.
fn arrays(case: &Case, order: &[usize], direction: Direction) -> Projection {
    let src: Vec<i64> = order.iter().map(|&k| case.edges[k].0 as i64).collect();
    let dst: Vec<i64> = order.iter().map(|&k| case.edges[k].1 as i64).collect();
    let w: Vec<f64> = order.iter().map(|&k| case.edges[k].2).collect();
    let ids = (0..case.n).map(|i| i.to_string()).collect();
    Projection::from_edges(ids, direction, &src, &dst, Some(&w))
        .unwrap()
        .with_coords(2, case.coords.clone())
        .unwrap()
        .finish()
}

fn graph_path(
    g: &Graph<Record, Record>,
    ix: &[NodeIx],
    s: usize,
    t: usize,
    mut q: PathQuery<'_, Record, GraphError>,
) -> Option<Vec<usize>> {
    let r = find_path(g, ix[s], ix[t], &mut q).unwrap()?;
    Some(r.nodes.iter().map(|n| ix.iter().position(|m| m == n).unwrap()).collect())
}

/// One path (or none) per query, as node indices.
type Paths = Vec<Option<Vec<usize>>>;

fn nodes(r: &[Option<DensePath>]) -> Paths {
    r.iter().map(|p| p.as_ref().map(|p| p.nodes.iter().map(|&u| u as usize).collect())).collect()
}

fn pairs(n: usize, rng: &mut Lcg) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = (0..40).map(|_| (rng.below(n as u64) as u32, rng.below(n as u64) as u32)).collect();
    out.push((0, n as u32 - 1));
    out
}

fn cases() -> Vec<Case> {
    let mut rng = Lcg::new(7);
    let mut out = vec![grid(9, 7, true, &mut rng), grid(10, 8, false, &mut rng), grid(1, 6, true, &mut rng)];
    for round in 0..6 {
        let n = 20 + 10 * round;
        out.push(random(n, 3 * n, &mut rng));
    }
    out
}

#[test]
fn single_queries_follow_the_rule_whatever_the_edge_order() {
    let mut rng = Lcg::new(11);
    for case in cases() {
        let pairs = pairs(case.n, &mut rng);
        let identity: Vec<usize> = (0..case.edges.len()).collect();
        let mut shuffled = identity.clone();
        rng.shuffle(&mut shuffled);
        let mut reversed = identity.clone();
        reversed.reverse();
        for direction in [Direction::Out, Direction::In, Direction::Both] {
            let mut astar_paths = None;
            for order in [&identity, &shuffled, &reversed] {
                let (g, ix) = graph(&case, order);
                let mut astar = Vec::new();
                for &(s, t) in &pairs {
                    let (s, t) = (s as usize, t as usize);
                    let mut q = PathQuery::dijkstra();
                    q.direction = direction;
                    assert_eq!(graph_path(&g, &ix, s, t, q), reference(&case, direction, false, s, t));
                    let mut q = PathQuery::bfs();
                    q.direction = direction;
                    assert_eq!(graph_path(&g, &ix, s, t, q), reference(&case, direction, true, s, t));
                    let h = Heuristic::coords(&g, ix[t], Metric::Manhattan, Coords::default()).unwrap();
                    let mut q = PathQuery::astar(h);
                    q.direction = direction;
                    astar.push(graph_path(&g, &ix, s, t, q));
                }
                // A* returns a cheapest path, the same one for every edge order
                match &astar_paths {
                    None => astar_paths = Some(astar),
                    Some(first) => assert_eq!(first, &astar),
                }
            }
        }
    }
}

#[test]
fn batch_queries_follow_the_rule_for_any_edge_order_and_thread_count() {
    let mut rng = Lcg::new(13);
    for case in cases() {
        let pairs = pairs(case.n, &mut rng);
        let identity: Vec<usize> = (0..case.edges.len()).collect();
        let mut shuffled = identity.clone();
        rng.shuffle(&mut shuffled);
        for direction in [Direction::Out, Direction::In, Direction::Both] {
            let want: Vec<_> =
                pairs.iter().map(|&(s, t)| reference(&case, direction, false, s as usize, t as usize)).collect();
            let want_hops: Vec<_> =
                pairs.iter().map(|&(s, t)| reference(&case, direction, true, s as usize, t as usize)).collect();
            let (g, ix) = graph(&case, &identity);
            let from_graph = Projection::collect::<_, _, GraphError>(
                &g,
                direction,
                &EdgeCost::weighted(None, None),
                |_, _| Ok(true),
                |_, _| Ok(true),
            )
            .unwrap()
            .with_coords(2, case.coords.clone())
            .unwrap()
            .finish();
            // Graph A* (single queries) and batch A* apply the same rule
            let graph_astar: Vec<_> = pairs
                .iter()
                .map(|&(s, t)| {
                    let h = Heuristic::coords(&g, ix[t as usize], Metric::Manhattan, Coords::default()).unwrap();
                    let mut q = PathQuery::astar(h);
                    q.direction = direction;
                    graph_path(&g, &ix, s as usize, t as usize, q)
                })
                .collect();
            let astar = AStar::new(Metric::Manhattan, 1.0).unwrap();
            for p in [from_graph, arrays(&case, &identity, direction), arrays(&case, &shuffled, direction)] {
                for threads in [1, 3, 8] {
                    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
                    pool.install(|| {
                        assert_eq!(nodes(&batch::shortest_paths(&p, &pairs, true, None).unwrap()), want);
                        assert_eq!(nodes(&batch::shortest_paths(&p, &pairs, false, None).unwrap()), want_hops);
                        let a = batch::shortest_paths_astar(&p, &pairs, &astar, None).unwrap();
                        assert_eq!(nodes(&a), graph_astar);
                    });
                }
                for (k, &(s, t)) in pairs.iter().enumerate().take(10) {
                    for (weighted, want) in [(true, &want), (false, &want_hops)] {
                        let first = algo::k_shortest_paths(&p, s, t, 1, weighted).unwrap();
                        let first = first.first().map(|p| p.nodes.iter().map(|&u| u as usize).collect());
                        assert_eq!(first, want[k]);
                    }
                }
            }
        }
    }
}

#[test]
fn zero_weight_edges_give_valid_paths_independent_of_order() {
    // Zero-weight cycles: the rule only re-parents open nodes, so paths
    // stay simple; the result still doesn't depend on edge order or threads.
    let mut rng = Lcg::new(17);
    for round in 0..8 {
        let n = 15 + 5 * round;
        let mut case = random(n, 4 * n, &mut rng);
        for e in &mut case.edges {
            if rng.below(3) == 0 {
                e.2 = 0.0;
            }
        }
        let pairs = pairs(n, &mut rng);
        let identity: Vec<usize> = (0..case.edges.len()).collect();
        let mut shuffled = identity.clone();
        rng.shuffle(&mut shuffled);
        for direction in [Direction::Out, Direction::Both] {
            let mut seen: Option<(Paths, Paths)> = None;
            for order in [&identity, &shuffled] {
                let (g, ix) = graph(&case, order);
                let single: Vec<_> = pairs
                    .iter()
                    .map(|&(s, t)| {
                        let mut q = PathQuery::dijkstra();
                        q.direction = direction;
                        graph_path(&g, &ix, s as usize, t as usize, q)
                    })
                    .collect();
                let p = arrays(&case, order, direction);
                let batch = nodes(&batch::shortest_paths(&p, &pairs, true, None).unwrap());
                for path in single.iter().chain(&batch).flatten() {
                    let mut sorted = path.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    assert_eq!(sorted.len(), path.len(), "{:?} repeats a node", path);
                }
                match &seen {
                    None => seen = Some((single, batch)),
                    Some((s, b)) => assert_eq!((s, b), (&single, &batch)),
                }
            }
        }
    }
}

#[test]
fn bidirectional_bfs_picks_the_lowest_predecessors() {
    // 0 -> {1, 2, 3} -> 4 -> {5, 6} -> 7, edges added highest first: the
    // path walks back from 7 through the lowest node of each level.
    let case = Case {
        n: 8,
        coords: vec![0.0; 16],
        edges: vec![
            (6, 7, 1.0),
            (5, 7, 1.0),
            (4, 6, 1.0),
            (4, 5, 1.0),
            (3, 4, 1.0),
            (2, 4, 1.0),
            (1, 4, 1.0),
            (0, 3, 1.0),
            (0, 2, 1.0),
            (0, 1, 1.0),
        ],
    };
    let order: Vec<usize> = (0..case.edges.len()).collect();
    let (g, ix) = graph(&case, &order);
    let path = ironweaver_core::traversal::bidirectional_bfs(&g, ix[0], ix[7], None, Direction::Out, |_, _| {
        Ok::<_, GraphError>(true)
    })
    .unwrap()
    .unwrap();
    assert_eq!(path, [ix[0], ix[1], ix[4], ix[5], ix[7]]);
    // An edge filter is respected when choosing, too
    let path = ironweaver_core::traversal::bidirectional_bfs(&g, ix[0], ix[7], None, Direction::Out, |_, e| {
        Ok::<_, GraphError>(e.source() != ix[1] && e.target() != ix[5])
    })
    .unwrap()
    .unwrap();
    assert_eq!(path, [ix[0], ix[2], ix[4], ix[6], ix[7]]);
}
