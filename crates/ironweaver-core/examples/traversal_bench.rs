//! Focused core benchmarks: graph creation and traversals, with sparse-reach controls.
use ironweaver_core::{
    traversal::{bfs, dfs},
    Graph, NodeIx,
};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn sample(name: &str, mut f: impl FnMut()) {
    if std::env::var("IW_BENCH_FILTER").is_ok_and(|filter| filter != name) {
        return;
    }
    f();
    let t = Instant::now();
    let mut count = 0;
    while t.elapsed() < Duration::from_millis(100) || count < 3 {
        f();
        count += 1;
    }
    println!("{name},{},{count}", t.elapsed().as_secs_f64() / count as f64);
}
fn build(n: usize, shape: &str) -> (Graph<(), ()>, Vec<NodeIx>) {
    let mut g = Graph::with_capacity(n, n * 6);
    let nodes: Vec<_> = (0..n).map(|i| g.add_node(format!("n{i}"), ()).unwrap()).collect();
    for u in 0..n - 1 {
        if shape != "isolated" {
            g.add_edge(nodes[u], nodes[u + 1], ()).unwrap();
        }
        if shape == "hub" {
            g.add_edge(nodes[0], nodes[u + 1], ()).unwrap();
        }
    }
    if shape == "random" {
        let mut rng = 42u64;
        for _ in 0..5 * n {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u = (rng >> 32) as usize % n;
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let v = (rng >> 32) as usize % n;
            g.add_edge(nodes[u], nodes[v], ()).unwrap();
        }
    }
    (g, nodes)
}
fn main() {
    for n in [1000, 20000, 100000] {
        for reserved in [false, true] {
            sample(&format!("create_nodes/{n}/{reserved}"), || {
                let mut g = if reserved { Graph::<(), ()>::with_capacity(n, 0) } else { Graph::new() };
                for i in 0..n {
                    black_box(g.add_node(format!("n{i}"), ()).unwrap());
                }
                black_box(g);
            });
        }
        let (existing, _) = build(n, "isolated");
        let mut duplicates = existing;
        sample(&format!("duplicate_nodes/{n}/control"), || {
            for i in 0..n {
                black_box(duplicates.add_node(format!("n{i}"), ()).unwrap_err());
            }
        });
        for shape in ["chain", "random", "hub", "isolated"] {
            let (g, nodes) = build(n, shape);
            sample(&format!("create_edges/{n}/{shape}"), || {
                let mut out = Graph::<(), ()>::with_capacity(n, g.edge_count());
                let ix: Vec<_> = (0..n).map(|i| out.add_node(format!("n{i}"), ()).unwrap()).collect();
                for (_, e) in g.edges() {
                    out.add_edge(ix[e.source().slot()], ix[e.target().slot()], ()).unwrap();
                }
                black_box(out);
            });
            for depth in [None, Some(3)] {
                sample(&format!("bfs/{n}/{shape}/{depth:?}"), || {
                    black_box(bfs(&g, nodes[0], depth, |_, _| Ok::<_, std::convert::Infallible>(true)).unwrap());
                });
                sample(&format!("dfs/{n}/{shape}/{depth:?}"), || {
                    black_box(dfs(&g, nodes[0], depth, |_, _| Ok::<_, std::convert::Infallible>(true)).unwrap());
                });
            }
        }
    }
}
