// `Projection::set_weights`: after changing weights in place, every
// algorithm gives the same result as on a projection built from scratch
// with the new weights; infinite weights make edges impassable.

use ironweaver_core::algo::{self, Betweenness, FastRP, Leiden, Node2Vec, PageRank};
use ironweaver_core::batch::{self, AStar};
use ironweaver_core::pathfinding::{EdgeCost, Metric};
use ironweaver_core::{Direction, Graph, GraphError, Projection, Record, Value};

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    fn below(&mut self, m: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % m
    }

    /// A weight: mostly 0.5..6 in steps of 1/16 (exact sums), sometimes 0
    /// or infinite.
    fn weight(&mut self) -> f64 {
        match self.below(10) {
            0 | 1 => f64::INFINITY,
            2 => 0.0,
            _ => 0.5 + self.below(90) as f64 / 16.0,
        }
    }
}

const DIRECTIONS: [Direction; 3] = [Direction::Out, Direction::In, Direction::Both];

/// Random edges (self-loops and parallel edges included) between `n` nodes.
fn random_edges(rng: &mut Lcg, n: usize, m: usize) -> (Vec<i64>, Vec<i64>, Vec<f64>) {
    let mut src = Vec::new();
    let mut dst = Vec::new();
    let mut w = Vec::new();
    for _ in 0..m {
        let s = rng.below(n as u64) as i64;
        // Some parallel edges and self-loops on purpose
        let t = match rng.below(8) {
            0 => s,
            1 if !src.is_empty() => {
                let k = rng.below(src.len() as u64) as usize;
                src.push(src[k]);
                dst.push(dst[k]);
                w.push(rng.weight());
                continue;
            }
            _ => rng.below(n as u64) as i64,
        };
        src.push(s);
        dst.push(t);
        w.push(rng.weight());
    }
    (src, dst, w)
}

fn ids(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("n{i}")).collect()
}

fn coords(n: usize) -> Vec<f64> {
    // A loose layout; the heuristic scale used below keeps A* admissible
    // or not, either way both projections must agree
    (0..n).flat_map(|i| [(i % 7) as f64, (i / 7) as f64]).collect()
}

fn build(n: usize, direction: Direction, src: &[i64], dst: &[i64], w: &[f64]) -> Projection {
    Projection::from_edges(ids(n), direction, src, dst, Some(w)).unwrap().with_coords(2, coords(n)).unwrap().finish()
}

fn close(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn assert_close(a: &[f64], b: &[f64], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for (x, y) in a.iter().zip(b) {
        assert!(close(*x, *y), "{what}: {x} vs {y}");
    }
}

/// Every adjacency row and weight agrees.
fn assert_same_rows(a: &Projection, b: &Projection) {
    for u in 0..a.node_count() as u32 {
        assert_eq!(a.out_neighbors(u), b.out_neighbors(u));
        assert_eq!(a.out_weights(u), b.out_weights(u), "out weights of {u}");
        assert_eq!(a.in_neighbors(u), b.in_neighbors(u));
        assert_eq!(a.in_weights(u), b.in_weights(u), "in weights of {u}");
    }
}

/// Searches: paths, distances, BFS levels, k shortest paths, centralities
/// based on shortest paths, spanning forests, node2vec walks. They treat an infinite
/// weight like a missing edge.
fn assert_same_searches(a: &Projection, b: &Projection) {
    let n = a.node_count() as u32;
    let pairs: Vec<(u32, u32)> = (0..n).flat_map(|s| (0..n).map(move |t| (s, t))).collect();
    let sources: Vec<u32> = (0..n).collect();
    for weighted in [true, false] {
        let pa = batch::shortest_paths(a, &pairs, weighted, None).unwrap();
        let pb = batch::shortest_paths(b, &pairs, weighted, None).unwrap();
        // Settled counts may differ when one side has extra (impassable) edges
        let strip = |v: Vec<Option<batch::DensePath>>| -> Vec<Option<(Vec<u32>, f64)>> {
            v.into_iter().map(|p| p.map(|p| (p.nodes, p.cost))).collect()
        };
        assert_eq!(strip(pa), strip(pb), "shortest paths, weighted {weighted}");
        let limit = Some(if weighted { 6.0 } else { 2.0 });
        for max_cost in [None, limit] {
            let da = batch::distances(a, &sources, weighted, max_cost).unwrap();
            let db = batch::distances(b, &sources, weighted, max_cost).unwrap();
            assert_eq!(da, db, "distances, weighted {weighted}, max_cost {max_cost:?}");
        }
        for (s, t) in [(0, n - 1), (1, n / 2), (n / 3, 2)] {
            let ka = algo::k_shortest_paths(a, s, t, 4, weighted).unwrap();
            let kb = algo::k_shortest_paths(b, s, t, 4, weighted).unwrap();
            let strip = |v: Vec<batch::DensePath>| -> Vec<(Vec<u32>, f64)> {
                v.into_iter().map(|p| (p.nodes, p.cost)).collect()
            };
            assert_eq!(strip(ka), strip(kb), "k shortest paths {s} -> {t}, weighted {weighted}");
        }
        let opts = Betweenness { normalized: false, weighted, ..Default::default() };
        assert_close(
            &algo::betweenness_centrality(a, &opts).unwrap(),
            &algo::betweenness_centrality(b, &opts).unwrap(),
            "betweenness",
        );
        for wf in [true, false] {
            assert_close(
                &algo::closeness_centrality(a, weighted, wf).unwrap(),
                &algo::closeness_centrality(b, weighted, wf).unwrap(),
                "closeness",
            );
        }
        assert_close(
            &algo::harmonic_centrality(a, weighted).unwrap(),
            &algo::harmonic_centrality(b, weighted).unwrap(),
            "harmonic",
        );
    }
    for astar in [AStar::new(Metric::Manhattan, 0.25).unwrap(), AStar::new(Metric::Euclidean, 3.0).unwrap()] {
        let strip = |v: Vec<Option<batch::DensePath>>| -> Vec<Option<(Vec<u32>, f64)>> {
            v.into_iter().map(|p| p.map(|p| (p.nodes, p.cost))).collect()
        };
        assert_eq!(
            strip(batch::shortest_paths_astar(a, &pairs, &astar, None).unwrap()),
            strip(batch::shortest_paths_astar(b, &pairs, &astar, None).unwrap()),
            "astar"
        );
    }
    for depth in [None, Some(2)] {
        assert_eq!(algo::bfs_levels(a, &[0], depth).unwrap(), algo::bfs_levels(b, &[0], depth).unwrap());
        assert_eq!(algo::bfs_levels(a, &[1, n - 1], depth).unwrap(), algo::bfs_levels(b, &[1, n - 1], depth).unwrap());
    }
    for maximum in [false, true] {
        assert_eq!(algo::spanning_forest(a, maximum), algo::spanning_forest(b, maximum), "spanning forest");
    }
    let nv = Node2Vec { walk_length: 12, walks_per_node: 3, p: 0.5, q: 2.0, seed: 9 };
    assert_eq!(algo::node2vec_walks(a, None, &nv).unwrap(), algo::node2vec_walks(b, None, &nv).unwrap());
}

/// Algorithms that use weights as strengths: an infinite weight counts 0.
fn assert_same_strengths(a: &Projection, b: &Projection) {
    let pr = PageRank { tol: 0.0, max_iter: 30, ..Default::default() };
    assert_close(&algo::pagerank(a, &pr).unwrap(), &algo::pagerank(b, &pr).unwrap(), "pagerank");
    let fr = FastRP { dimension: 8, seed: 3, ..Default::default() };
    let (fa, fb) = (algo::fastrp(a, &fr).unwrap(), algo::fastrp(b, &fr).unwrap());
    assert!(fa.iter().zip(&fb).all(|(x, y)| (x - y).abs() <= 1e-5), "fastrp");
    let ld = Leiden { seed: 5, ..Default::default() };
    let communities = algo::leiden(a, &ld).unwrap();
    assert_eq!(communities, algo::leiden(b, &ld).unwrap(), "leiden");
    let mut labels = vec![0u32; a.node_count()];
    for (c, members) in communities.iter().enumerate() {
        for &u in members {
            labels[u as usize] = c as u32;
        }
    }
    assert!(close(algo::modularity(a, &labels, 1.0).unwrap(), algo::modularity(b, &labels, 1.0).unwrap()));
}

#[test]
fn set_weights_matches_a_rebuild() {
    for seed in 0..24u64 {
        let mut rng = Lcg::new(seed);
        let n = 12 + rng.below(20) as usize;
        let m = 10 + rng.below(70) as usize;
        let (src, dst, mut w) = random_edges(&mut rng, n, m);
        for direction in DIRECTIONS {
            let mut p = build(n, direction, &src, &dst, &w);
            // Half the time the transposed adjacency exists already
            if seed % 2 == 0 {
                let _ = p.in_neighbors(0);
            }
            // A few rounds of changes, some edges listed twice (last wins)
            for _ in 0..3 {
                let changes = 1 + rng.below(m as u64 / 2 + 1) as usize;
                let mut edges: Vec<u32> = Vec::new();
                let mut weights = Vec::new();
                for _ in 0..changes {
                    let k = rng.below(m as u64) as u32;
                    let x = rng.weight();
                    edges.push(k);
                    weights.push(x);
                    w[k as usize] = x;
                }
                p.set_weights(&edges, &weights).unwrap();
                let rebuilt = build(n, direction, &src, &dst, &w);
                assert_same_rows(&p, &rebuilt);
            }
            let rebuilt = build(n, direction, &src, &dst, &w);
            assert_same_searches(&p, &rebuilt);
            assert_same_strengths(&p, &rebuilt);
        }
    }
}

#[test]
fn infinite_weights_are_impassable() {
    for seed in 0..16u64 {
        let mut rng = Lcg::new(100 + seed);
        let n = 10 + rng.below(16) as usize;
        let m = 10 + rng.below(60) as usize;
        let (src, dst, w) = random_edges(&mut rng, n, m);
        // The same graph without its infinite edges (for searches), and with
        // weight 0 in their place (for strengths)
        let finite: Vec<usize> = (0..m).filter(|&k| w[k].is_finite()).collect();
        let pick = |v: &[i64]| -> Vec<i64> { finite.iter().map(|&k| v[k]).collect() };
        let (fs, fd) = (pick(&src), pick(&dst));
        let fw: Vec<f64> = finite.iter().map(|&k| w[k]).collect();
        let zeroed: Vec<f64> = w.iter().map(|&x| if x.is_finite() { x } else { 0.0 }).collect();
        for direction in DIRECTIONS {
            let p = build(n, direction, &src, &dst, &w);
            assert_same_searches(&p, &build(n, direction, &fs, &fd, &fw));
            assert_same_strengths(&p, &build(n, direction, &src, &dst, &zeroed));
        }
    }
}

#[test]
fn a_blocked_grid_tile() {
    // A 3 x 3 grid, both directions of each edge given: blocking the middle
    // tile (all edges into it) forces the path around it, unblocking it
    // brings the straight path back.
    let id = |x: i64, y: i64| y * 3 + x;
    let (mut src, mut dst) = (Vec::new(), Vec::new());
    for y in 0..3 {
        for x in 0..3 {
            for (dx, dy) in [(1, 0), (0, 1)] {
                if x + dx < 3 && y + dy < 3 {
                    src.extend([id(x, y), id(x + dx, y + dy)]);
                    dst.extend([id(x + dx, y + dy), id(x, y)]);
                }
            }
        }
    }
    let w = vec![1.0; src.len()];
    let mut p = build(9, Direction::Out, &src, &dst, &w);
    let into_middle: Vec<u32> = (0..dst.len() as u32).filter(|&k| dst[k as usize] == 4).collect();
    assert_eq!(into_middle.len(), 4);
    let path = |p: &Projection| batch::shortest_paths(p, &[(1, 7)], true, None).unwrap()[0].clone().unwrap();
    assert_eq!(path(&p).nodes, [1, 4, 7]);
    p.set_weights(&into_middle, &[f64::INFINITY; 4]).unwrap();
    let around = path(&p);
    assert_eq!(around.cost, 4.0);
    assert!(!around.nodes.contains(&4));
    // Hop counts avoid it too
    let hops = batch::shortest_paths(&p, &[(1, 7)], false, None).unwrap()[0].clone().unwrap();
    assert_eq!(hops.cost, 4.0);
    assert_eq!(algo::bfs_levels(&p, &[1], None).unwrap()[4], algo::NONE);
    p.set_weights(&into_middle, &[1.0; 4]).unwrap();
    assert_eq!(path(&p).nodes, [1, 4, 7]);
}

#[test]
fn set_weights_checks_its_input() {
    let ids = || vec!["a".to_string(), "b".to_string()];
    let mut p =
        Projection::from_edges(ids(), Direction::Both, &[0i64, 1], &[1, 1], Some(&[1.0, 2.0])).unwrap().finish();
    let err = |r: Result<(), GraphError>| match r {
        Err(GraphError::InvalidArgument(msg)) => msg,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(err(p.set_weights(&[0i64, 2], &[1.0, 1.0])), "edges[1] is 2, not an edge index (0 to 1)");
    assert!(err(p.set_weights(&[-1i64], &[1.0])).starts_with("edges[0] is -1"));
    assert!(err(p.set_weights(&[0i64], &[1.0, 2.0])).contains("same length"));
    for bad in [-1.0, f64::NAN] {
        let msg = err(p.set_weights(&[0i64, 1], &[3.0, bad]));
        assert!(msg.starts_with("Edge weights must be non-negative") && msg.ends_with("(weights[1])"), "{msg}");
    }
    // Nothing changed by the failed calls
    assert_eq!(p.out_weights(0).unwrap(), [1.0]);
    assert_eq!(p.out_weights(1).unwrap(), [1.0, 2.0, 2.0]); // the self-loop from both ends
    p.set_weights(&[1i64], &[5.0]).unwrap();
    assert_eq!(p.out_weights(1).unwrap(), [1.0, 5.0, 5.0]);
    p.set_weights::<i64>(&[], &[]).unwrap();

    let mut unweighted = Projection::from_edges(ids(), Direction::Out, &[0i64], &[1], None).unwrap().finish();
    assert!(err(unweighted.set_weights(&[0i64], &[1.0])).contains("weighted projection"));
    let mut empty = Projection::from_edges::<i64>(ids(), Direction::Out, &[], &[], Some(&[])).unwrap().finish();
    assert_eq!(err(empty.set_weights(&[0i64], &[1.0])), "edges[0] is 0, but there are no edges");
}

#[test]
fn graph_projections_name_edges_in_slot_order() {
    type G = Graph<Record, Record>;
    let weighted = |w: f64| Record::with_attr([("weight", Value::from(w))]);
    let mut g = G::new();
    let ix: Vec<_> = (0..4).map(|i| g.add_node(format!("n{i}"), Record::default()).unwrap()).collect();
    let mut edges = Vec::new();
    for (k, &(s, t)) in [(0, 1), (1, 2), (2, 3), (0, 1), (3, 0), (1, 1)].iter().enumerate() {
        edges.push(g.add_edge(ix[s], ix[t], weighted(1.0 + k as f64)).unwrap());
    }
    // A removed edge's slot is reused by the next one
    g.remove_edge(edges[1]).unwrap();
    let reused = g.add_edge(ix[2], ix[0], weighted(9.0)).unwrap();
    assert_eq!(reused.slot(), edges[1].slot());
    let cost = EdgeCost::weighted(None, None);
    for direction in DIRECTIONS {
        // Leave out edges into n3
        let collect = |g: &G| {
            Projection::collect::<_, _, GraphError>(
                g,
                direction,
                &cost,
                |_, _| Ok(true),
                |_, e| Ok(e.target() != ix[3]),
            )
            .unwrap()
            .finish()
        };
        let mut p = collect(&g);
        assert_eq!(p.edge_count(), 5);
        // Slot order: the reused slot comes second
        let order = [edges[0], reused, edges[3], edges[4], edges[5]];
        for (k, &e) in order.iter().enumerate() {
            assert_eq!(p.edge_index(e), Some(k));
        }
        assert_eq!(p.edge_index(edges[2]), None); // filtered out
        assert_eq!(p.edge_index(edges[1]), None); // removed (its slot is reused)
                                                  // Change two weights in the graph and in place: same as projecting again
        for (k, w) in [(1usize, f64::INFINITY), (4, 0.5)] {
            g.edge_mut(order[k]).unwrap().data = weighted(w);
            p.set_weights(&[k as u32], &[w]).unwrap();
        }
        assert_same_rows(&p, &collect(&g));
        g.edge_mut(order[1]).unwrap().data = weighted(9.0);
        g.edge_mut(order[4]).unwrap().data = weighted(6.0);
    }
    let unweighted = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::Unit).unwrap();
    assert_eq!(unweighted.edge_index(edges[0]), None);
}

#[test]
fn memory_includes_the_edge_positions() {
    let (src, dst) = ([0i64, 1, 2], [1i64, 2, 0]);
    let ids = || vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let weighted = Projection::from_edges(ids(), Direction::Out, &src, &dst, Some(&[1.0; 3])).unwrap().finish();
    let unweighted = Projection::from_edges(ids(), Direction::Out, &src, &dst, None).unwrap().finish();
    // 8 bytes of weight plus 4 of position per edge
    assert_eq!(weighted.memory_usage() - unweighted.memory_usage(), 3 * 12);
}
