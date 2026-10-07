//! Edge insertion isolated from node setup and destruction; independent graphs
//! provide a concurrency control without changing mutation guarantees.
use ironweaver_core::{Direction, EdgeId, Graph, NodeIx};
use rayon::prelude::*;
use std::{hint::black_box, time::Instant};

fn fixture(n: usize, m: usize) -> (Graph<(), ()>, Vec<NodeIx>) {
    let mut g = Graph::with_capacity(n, m);
    let nodes = (0..n).map(|i| g.add_node(format!("n{i}"), ()).unwrap()).collect();
    (g, nodes)
}
fn edges(g: &mut Graph<(), ()>, nodes: &[NodeIx], m: usize, shape: &str) {
    let mut rng = 42u64;
    for i in 0..m {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let u = (rng >> 32) as usize % nodes.len();
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let v = (rng >> 32) as usize % nodes.len();
        let (u, v) = match shape {
            "parallel" => (0, 1),
            "self" => (0, 0),
            "chain" => (i % nodes.len(), (i + 1) % nodes.len()),
            _ => (u, v),
        };
        if shape == "typed" {
            black_box(g.insert_edge(nodes[u], nodes[v], None, Some("next"), ()).unwrap());
        } else if shape == "explicit" {
            black_box(g.insert_edge(nodes[u], nodes[v], Some(EdgeId(i as u64 * 7)), None, ()).unwrap());
        } else {
            black_box(g.add_edge(nodes[u], nodes[v], ()).unwrap());
        }
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let case = &args[1];
    let n: usize = args[2].parse().unwrap();
    let m = n * 5;
    let samples: usize = args.get(3).map_or(7, |s| s.parse().unwrap());
    println!("case,n,seconds");
    for sample in 0..samples + 2 {
        let seconds = if case == "concurrent" {
            let tasks: Vec<_> = (0..4).map(|_| fixture(n, m)).collect();
            let start = Instant::now();
            let graphs: Vec<_> = tasks
                .into_par_iter()
                .map(|(mut g, nodes)| {
                    edges(&mut g, &nodes, m, "random");
                    g
                })
                .collect();
            let elapsed = start.elapsed().as_secs_f64();
            assert!(graphs.iter().all(|g| g.edge_count() == m));
            black_box(graphs);
            elapsed
        } else {
            let (mut g, mut nodes) = fixture(n, m);
            // Controls use unchanged node insertion / duplicate validation,
            // failed edge insertion, or neighbor iteration over a built graph.
            if case == "neighbors" || case.starts_with("subgraph") {
                edges(&mut g, &nodes, m, "random");
            }
            if case == "subgraph-small" {
                for i in 0..3 {
                    nodes.push(g.add_node(format!("isolated{i}"), ()).unwrap());
                }
            }
            let stale = if case == "stale" {
                let ix = g.add_node("removed", ()).unwrap();
                g.remove_node(ix).unwrap();
                Some(ix)
            } else {
                None
            };
            let start = Instant::now();
            match case.as_str() {
                "subgraph" | "subgraph-small" => {
                    let keep = if case == "subgraph-small" { &nodes[n..] } else { &nodes[..] };
                    let out = g
                        .induced_subgraph(
                            keep.iter().copied(),
                            |_| Ok::<_, std::convert::Infallible>(()),
                            |_| Ok::<_, std::convert::Infallible>(()),
                        )
                        .unwrap();
                    assert_eq!(out.node_count(), keep.len());
                    assert_eq!(out.next_edge_id(), g.next_edge_id());
                    if case == "subgraph-small" {
                        assert_eq!(out.edge_count(), 0);
                    }
                    black_box(out);
                }
                "nodes" => {
                    for i in 0..n {
                        black_box(g.add_node(format!("extra{i}"), ()).unwrap());
                    }
                }
                "duplicates" => {
                    for i in 0..n {
                        black_box(g.add_node(format!("n{i}"), ()).unwrap_err());
                    }
                }
                "stale" => {
                    for _ in 0..m {
                        black_box(g.add_edge(stale.unwrap(), nodes[0], ()).unwrap_err());
                    }
                }
                "neighbors" => {
                    let count: usize = nodes.iter().map(|&ix| g.neighbors(ix, Direction::Both).count()).sum();
                    assert_eq!(black_box(count), 2 * m);
                }
                _ => edges(&mut g, &nodes, m, case),
            }
            let elapsed = start.elapsed().as_secs_f64();
            if !matches!(case.as_str(), "nodes" | "duplicates" | "stale" | "neighbors" | "subgraph" | "subgraph-small")
            {
                assert_eq!(g.edge_count(), m);
            }
            black_box(g);
            elapsed
        };
        if sample >= 2 {
            println!("{case},{n},{seconds:.9}");
        }
    }
}
