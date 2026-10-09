//! Dependency-free PageRank timing harness. See performance_results/pagerank-2026-10-09.
use std::hint::black_box;
use std::time::{Duration, Instant};

use ironweaver_core::pathfinding::EdgeCost;
use ironweaver_core::{algo, Direction, Graph, GraphError, Projection, Record, Value};

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n as u64) as usize
    }
}

fn main() {
    let n = std::env::var("IWDB_BENCH_NODES").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    assert!(n > 0);
    let mut g = Graph::<Record, Record>::default();
    let nodes: Vec<_> = (0..n)
        .map(|i| {
            let u = g.add_node(format!("n{i}"), Record::default()).unwrap();
            g.add_label(u, "Person").unwrap();
            u
        })
        .collect();
    for u in 0..n {
        let mut rng = Rng::new(u as u64);
        for _ in 0..4 {
            let v = rng.below(n);
            // Consume both draws, exactly as ironweaver-db's benchmark does.
            let w = 1 + rng.below(10) as i64;
            let e = g.add_edge(nodes[u], nodes[v], Record::with_attr([("w", Value::Int(w))])).unwrap();
            g.set_edge_type(e, Some("KNOWS")).unwrap();
        }
    }
    let unit = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::Unit).unwrap();
    let weighted =
        Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::weighted(Some("w".into()), None)).unwrap();
    let samples: usize = std::env::var("SAMPLES").ok().and_then(|v| v.parse().ok()).unwrap_or(9);
    let millis: u64 = std::env::var("SAMPLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    for (case, p, opts) in [
        ("db-default", &unit, algo::PageRank::default()),
        ("unit-20", &unit, algo::PageRank { max_iter: 20, tol: 0.0, ..Default::default() }),
        ("weighted-20", &weighted, algo::PageRank { max_iter: 20, tol: 0.0, ..Default::default() }),
    ] {
        let expected = algo::pagerank(p, &opts).unwrap();
        let progress = ironweaver_core::cancel::Progress::default();
        let token = ironweaver_core::cancel::Token::default();
        ironweaver_core::cancel::run_with_progress(&token, &progress, || algo::pagerank(p, &opts)).unwrap().unwrap();
        let iterations = progress.snapshot().done;
        let fingerprint = expected.iter().fold(0u64, |h, x| h.rotate_left(7) ^ x.to_bits());
        for _ in 0..3 {
            black_box(algo::pagerank(p, &opts).unwrap());
        }
        let mut times = Vec::new();
        for _ in 0..samples {
            let start = Instant::now();
            let mut count = 0;
            while start.elapsed() < Duration::from_millis(millis) {
                black_box(algo::pagerank(black_box(p), black_box(&opts)).unwrap());
                count += 1;
            }
            times.push(start.elapsed().as_secs_f64() * 1e6 / count as f64);
        }
        println!(
            "{}",
            serde_json::json!({"case": case, "nodes": n, "threads": rayon::current_num_threads(), "iterations": iterations, "fingerprint": format!("{fingerprint:016x}"), "mass": expected.iter().sum::<f64>(), "samples_us": times})
        );
    }
}
