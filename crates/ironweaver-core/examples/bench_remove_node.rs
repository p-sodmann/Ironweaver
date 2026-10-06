//! Isolate node deletion from graph construction/destruction.
//!
//! cargo run --release -p ironweaver-core --example bench_remove_node -- parallel-out 16000 10 4
//! Arguments: shape, size, measured samples, deletions/sample. Size is the
//! incident edge count except in leaf controls, where it counts surviving edges.
//! Emits CSV; three warmup samples are excluded. Use a fixed CPU and alternate
//! independently built baseline/candidate executables (see benchmarks/compare_remove_node.py).
use std::hint::black_box;
use std::time::Instant;

use ironweaver_core::{Graph, NodeIx};
use rand::{seq::SliceRandom, SeedableRng};

fn fixture(shape: &str, n: usize) -> (Graph<(), ()>, NodeIx, usize) {
    let mut g = Graph::new();
    let hub = g.add_node("hub", ()).unwrap();
    let width = match shape {
        "parallel-out" | "parallel-in" | "bidirectional" => 1,
        "mixed" => 16,
        "leaf-out" | "leaf-in" => n + 1,
        "two-hubs-out" | "two-hubs-in" => n + 2,
        "fan-survivors" | "dense" => n,
        "star" | "shuffled-star" => n,
        "self-loops" | "isolated" => 0,
        _ => panic!("unknown shape: {shape}"),
    };
    let mut neighbors: Vec<_> = (0..width).map(|i| g.add_node(format!("n{i}"), ()).unwrap()).collect();
    if shape == "shuffled-star" {
        neighbors.shuffle(&mut rand::rngs::StdRng::seed_from_u64(42));
    }
    if shape == "leaf-out" {
        g.add_edge(hub, neighbors[0], ()).unwrap();
    } else if shape == "leaf-in" {
        g.add_edge(neighbors[0], hub, ()).unwrap();
    }
    if shape == "two-hubs-out" {
        g.add_edge(hub, neighbors[0], ()).unwrap();
        g.add_edge(hub, neighbors[1], ()).unwrap();
    } else if shape == "two-hubs-in" {
        g.add_edge(neighbors[0], hub, ()).unwrap();
        g.add_edge(neighbors[1], hub, ()).unwrap();
    }
    for i in 0..n {
        match shape {
            "isolated" => (),
            "two-hubs-out" => {
                g.add_edge(neighbors[i + 2], neighbors[0], ()).unwrap();
                g.add_edge(neighbors[i + 2], neighbors[1], ()).unwrap();
            }
            "two-hubs-in" => {
                g.add_edge(neighbors[0], neighbors[i + 2], ()).unwrap();
                g.add_edge(neighbors[1], neighbors[i + 2], ()).unwrap();
            }
            "fan-survivors" | "dense" => {
                g.add_edge(hub, neighbors[i], ()).unwrap();
                let degree = if shape == "dense" { n } else { 64 };
                for j in 0..degree {
                    g.add_edge(neighbors[(i + j) % n], neighbors[i], ()).unwrap();
                }
            }
            "leaf-out" => {
                g.add_edge(neighbors[i + 1], neighbors[0], ()).unwrap();
            }
            "leaf-in" => {
                g.add_edge(neighbors[0], neighbors[i + 1], ()).unwrap();
            }
            "self-loops" => {
                g.add_edge(hub, hub, ()).unwrap();
            }
            _ => {
                let other = neighbors[i % width];
                let incoming = shape == "parallel-in" || (shape == "bidirectional" && i % 2 == 0);
                let (from, to) = if incoming { (other, hub) } else { (hub, other) };
                g.add_edge(from, to, ()).unwrap();
                if shape == "mixed" {
                    // Interleave edges that must survive the hub deletion.
                    g.add_edge(neighbors[(i + 1) % width], other, ()).unwrap();
                }
            }
        }
    }
    let remaining = match shape {
        "mixed" | "leaf-out" | "leaf-in" => n,
        "two-hubs-out" | "two-hubs-in" => 2 * n,
        "fan-survivors" => 64 * n,
        "dense" => n * n,
        _ => 0,
    };
    (g, hub, remaining)
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let shape = args.get(1).map(String::as_str).unwrap_or("parallel-out");
    let n = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(16000);
    let samples = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(10);
    let batch = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(4);
    assert!(samples > 0 && batch > 0);
    let (template, hub, remaining) = fixture(shape, n);
    println!("shape,edges,sample,ns_per_delete");
    for sample in 0..samples + 3 {
        let mut elapsed = 0u128;
        for _ in 0..batch {
            let mut g = template.clone();
            let start = Instant::now();
            let removed = black_box(g.remove_node(black_box(hub)));
            elapsed += start.elapsed().as_nanos();
            assert_eq!(removed.unwrap().0, "hub");
            assert_eq!(g.node_count(), template.node_count() - 1);
            assert_eq!(g.edge_count(), remaining);
            assert!(g.node(hub).is_none());
            // Keep graph destruction out of the measured interval.
            black_box(&g);
        }
        if sample >= 3 {
            println!("{shape},{n},{},{:.3}", sample - 3, elapsed as f64 / batch as f64);
        }
    }
}
