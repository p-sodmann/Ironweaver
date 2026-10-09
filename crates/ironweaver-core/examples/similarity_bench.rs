//! Pairwise similarity benchmarks, including hub/leaf and balanced controls.
//! Run with `RAYON_NUM_THREADS=1 cargo run -p ironweaver-core --release --example similarity_bench`.
//! Graph/projection construction is excluded; each timed call scores 1,024 pairs.
use ironweaver_core::{
    algo::{similarity, Similarity},
    pathfinding::EdgeCost,
    Direction, Graph, GraphError, Projection, Record,
};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn projection(degree: usize, small: usize, position: &str) -> Projection {
    let n = 17 + 2 * degree;
    let mut g = Graph::<Record, Record>::with_capacity(n, degree + 16 * small);
    let nodes: Vec<_> = (0..n).map(|i| g.add_node(format!("n{i}"), Record::default()).unwrap()).collect();
    for i in 0..degree {
        g.add_edge(nodes[0], nodes[17 + 2 * i], Record::default()).unwrap();
    }
    for source in 1..=16 {
        for i in 0..small {
            let at = match position {
                "prefix" => i,
                "suffix" => degree - small + i,
                _ => (i * degree / small + source - 1) % degree,
            };
            let target = 17 + 2 * at + usize::from(position == "disjoint");
            g.add_edge(nodes[source], nodes[target], Record::default()).unwrap();
        }
    }
    Projection::build::<_, _, GraphError>(&g, Direction::Both, &EdgeCost::Unit).unwrap()
}

fn main() {
    let duration = Duration::from_millis(std::env::var("IW_BENCH_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(200));
    let pairs: Vec<_> = (0..1024).map(|i| if i % 2 == 0 { (0, 1 + i % 16) } else { (1 + i % 16, 0) }).collect();
    println!("workload,seconds_per_call,iterations");
    for (degree, small, position) in [
        (128, 1, "suffix"),
        (4096, 8, "spread"),
        (32768, 8, "spread"),
        (4096, 8, "prefix"),
        (4096, 8, "disjoint"),
        (4096, 128, "spread"),
        (128, 128, "spread"),
        (4096, 4096, "spread"),
    ] {
        let p = projection(degree, small, position);
        for (name, metric) in [
            ("common_neighbors", Similarity::CommonNeighbors),
            ("jaccard", Similarity::Jaccard),
            ("adamic_adar", Similarity::AdamicAdar),
        ] {
            let workload = format!("{degree}/{small}/{position}/{name}");
            if std::env::var("IW_BENCH_FILTER").is_ok_and(|filter| !workload.contains(&filter)) {
                continue;
            }
            let scores = similarity(&p, &pairs, metric).unwrap();
            assert_eq!(scores.len(), pairs.len());
            assert!(scores.iter().all(|s| s.is_finite() && *s >= 0.0));
            if metric == Similarity::CommonNeighbors {
                let want = if position == "disjoint" { 0.0 } else { small as f64 };
                assert!(scores.iter().all(|&s| s == want));
            }
            let t = Instant::now();
            let mut count = 0;
            while t.elapsed() < duration || count < 3 {
                black_box(similarity(black_box(&p), black_box(&pairs), metric).unwrap());
                count += 1;
            }
            println!("{workload},{},{count}", t.elapsed().as_secs_f64() / count as f64);
        }
    }
}
