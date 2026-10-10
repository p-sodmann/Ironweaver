//! Same seeded topology, labels, attributes and query as Ironweaver DB's match benchmark.
use std::collections::BinaryHeap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use ironweaver_core::budget::Budget;
use ironweaver_core::query::{for_each_match_limited, Bound, Pattern};
use ironweaver_core::{EdgeId, Graph, GraphError, Limited, Record, Value};

type Row = (Vec<String>, Vec<Vec<EdgeId>>);

#[derive(Debug)]
struct Run {
    rows: Vec<Row>,
    matches: usize,
    work: Limited<()>,
    trace: u64,
}

// Match DB's visitor: enumerate within the work budget, retaining the first
// 100 rows by node ids then edge ids. Queueing, locks and cursors are excluded.
fn run(g: &Graph<Record, Record>, p: &Pattern, budget: Budget, verify: bool) -> Run {
    let mut heap = BinaryHeap::new();
    let mut matches = 0;
    let mut trace = 0u64;
    let work = for_each_match_limited::<_, _, GraphError>(g, p, budget, |m| {
        matches += 1;
        if verify {
            for byte in format!("{m:?}").bytes() {
                trace = trace.wrapping_mul(0x100000001b3) ^ u64::from(byte);
            }
        }
        let row: Row = (
            m.nodes.iter().map(|&ix| g.node(ix).unwrap().id().to_owned()).collect(),
            m.edges
                .iter()
                .map(|bound| match bound {
                    Bound::Edge(e) => vec![g.edge(*e).unwrap().id()],
                    Bound::Path(path) => path.iter().map(|&e| g.edge(e).unwrap().id()).collect(),
                })
                .collect(),
        );
        heap.push(row);
        if heap.len() > 101 {
            heap.pop();
        }
        Ok(true)
    })
    .unwrap();
    let mut rows = heap.into_sorted_vec();
    rows.truncate(100);
    Run { rows, matches, work, trace }
}

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
    let n: usize = std::env::var("IWDB_BENCH_NODES").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let samples: usize = std::env::var("SAMPLES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let millis: u64 = std::env::var("SAMPLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let verify_groups: usize = std::env::var("VERIFY_GROUPS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let only_case = std::env::var("CASE").ok();
    let mut g = Graph::<Record, Record>::new();
    let nodes: Vec<_> = (0..n)
        .map(|i| {
            let node = g
                .add_node(
                    format!("n{i}"),
                    Record::with_attr([
                        ("age", Value::Int((i % 80) as i64)),
                        ("group", Value::Int((i % 1000) as i64)),
                        ("x", Value::Float((i % 1000) as f64)),
                        ("y", Value::Float((i / 1000) as f64)),
                    ]),
                )
                .unwrap();
            g.add_label(node, "Person").unwrap();
            node
        })
        .collect();
    for u in 0..n {
        let mut rng = Rng::new(u as u64);
        for _ in 0..4 {
            let v = rng.below(n);
            let w = 1 + rng.below(10) as i64;
            g.insert_edge(nodes[u], nodes[v], None, Some("KNOWS"), Record::with_attr([("w", Value::Int(w))])).unwrap();
        }
    }
    g.create_index::<GraphError>(&["age".into()]).unwrap();
    let db_budget = Budget::UNLIMITED.max_visited(100_000).max_edges(1_000_000).truncate();
    for (case, label, budget) in [
        ("db-two-hops-100", ":Person", db_budget),
        ("bounded-core-100", ":Person", db_budget.max_results(100)),
        ("rare-two-hops-100", ":Person:Rare", db_budget),
        ("unlabeled-two-hops-100", "", db_budget),
        ("sparse-arena-100", ":Person", db_budget),
    ] {
        if only_case.as_ref().is_some_and(|only| only != case) {
            continue;
        }
        if case == "rare-two-hops-100" {
            for &node in nodes.iter().step_by(100) {
                g.add_label(node, "Rare").unwrap();
            }
        }
        if case == "sparse-arena-100" {
            // Keep just 100 nodes at the end of the arena: scanning deleted
            // slots here would defeat the label index even for a universal label.
            for &node in nodes.iter().take(n.saturating_sub(100)) {
                g.remove_node(node).unwrap();
            }
        }
        let query = |group| {
            let text = if case == "sparse-arena-100" {
                "(a:Person)".to_owned()
            } else {
                format!("(a{label} {{group: {group}}})-[:KNOWS]->(b)-[:KNOWS]->(c)")
            };
            Pattern::parse(&text).unwrap()
        };
        let mut fingerprint = 0u64;
        let mut results = 0usize;
        let mut visited = 0usize;
        let mut edges = 0usize;
        let mut matches = 0usize;
        let mut truncated = 0usize;
        // Compare ordered nodes, edges and budget counters across spread-out groups.
        for i in 0..verify_groups {
            let group = (i * 997) % 1000;
            let found = run(&g, &query(group), budget, true);
            results += found.rows.len();
            visited += found.work.visited;
            edges += found.work.edges;
            matches += found.matches;
            truncated += usize::from(found.work.truncated);
            fingerprint ^= found.trace;
            let text = format!("{found:?}");
            for byte in text.bytes() {
                fingerprint = fingerprint.wrapping_mul(0x100000001b3) ^ u64::from(byte);
            }
        }
        let mut times = Vec::new();
        let mut pick = Rng::new(7);
        for _ in 0..3 {
            black_box(run(&g, &query(pick.below(n) % 1000), budget, false));
        }
        for _ in 0..samples {
            let mut pick = Rng::new(7);
            let start = Instant::now();
            let mut count = 0;
            while start.elapsed() < Duration::from_millis(millis) {
                black_box(run(black_box(&g), &query(pick.below(n) % 1000), budget, false));
                count += 1;
            }
            times.push(start.elapsed().as_secs_f64() * 1e6 / count as f64);
        }
        println!(
            "{}",
            serde_json::json!({"case": case, "nodes": n, "verify_groups": verify_groups, "fingerprint": format!("{fingerprint:016x}"), "results": results, "matches": matches, "truncated": truncated, "visited": visited, "edges": edges, "samples_us": times})
        );
    }
}
