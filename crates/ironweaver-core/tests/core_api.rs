// End-to-end tests of the public core API on `Graph<Record, Record>`.

use std::collections::HashMap;

use ironweaver_core::format;
use ironweaver_core::pathfinding::{self, find_path, Coords, Heuristic, Metric, PathQuery};
use ironweaver_core::random_walks::{random_walks, WalkOptions};
use ironweaver_core::{Direction, Graph, GraphError, NodeIx, Record, Value};

type G = Graph<Record, Record>;

fn weighted(w: f64) -> Record {
    Record::with_attr([("weight", Value::from(w))])
}

//   a -> b -> d
//   a -> c -> d      plus d -> e
fn diamond() -> (G, HashMap<&'static str, NodeIx>) {
    let mut g = G::new();
    let mut ix = HashMap::new();
    for id in ["a", "b", "c", "d", "e"] {
        ix.insert(id, g.add_node(id, Record::with_attr([("name", Value::from(id))])).unwrap());
    }
    g.add_edge(ix["a"], ix["b"], weighted(1.0)).unwrap();
    g.add_edge(ix["b"], ix["d"], weighted(5.0)).unwrap();
    g.add_edge(ix["a"], ix["c"], weighted(2.0)).unwrap();
    g.add_edge(ix["c"], ix["d"], weighted(1.0)).unwrap();
    g.add_edge(ix["d"], ix["e"], Record::default()).unwrap();
    (g, ix)
}

fn ids(g: &G, nodes: &[NodeIx]) -> Vec<String> {
    nodes.iter().map(|&n| g.node(n).unwrap().id().to_string()).collect()
}

#[test]
fn dijkstra_bfs_and_limits() {
    let (g, ix) = diamond();
    let p = find_path::<_, _, GraphError>(&g, ix["a"], ix["e"], &mut PathQuery::dijkstra()).unwrap().unwrap();
    assert_eq!(ids(&g, &p.nodes), ["a", "c", "d", "e"]);
    assert_eq!(p.cost, 4.0);
    assert!(p.expanded.is_some());

    let p = find_path::<_, _, GraphError>(&g, ix["a"], ix["e"], &mut PathQuery::bfs()).unwrap().unwrap();
    assert_eq!(p.nodes.len(), 4);
    assert_eq!(p.cost, 3.0);

    let mut q = PathQuery::dijkstra();
    q.max_cost = Some(3.5);
    assert!(find_path::<_, _, GraphError>(&g, ix["a"], ix["e"], &mut q).unwrap().is_none());

    let mut q = PathQuery::bfs();
    q.direction = Direction::In;
    let p = find_path::<_, _, GraphError>(&g, ix["e"], ix["a"], &mut q).unwrap().unwrap();
    assert_eq!(ids(&g, &p.nodes)[0], "e");
}

#[test]
fn weight_errors() {
    let (mut g, ix) = diamond();
    g.add_edge(ix["e"], ix["a"], weighted(-1.0)).unwrap();
    let err = find_path::<_, _, GraphError>(&g, ix["e"], ix["a"], &mut PathQuery::dijkstra()).unwrap_err();
    assert!(matches!(err, GraphError::InvalidArgument(m) if m.contains("non-negative")));
    let (mut g, ix) = diamond();
    g.add_edge(ix["e"], ix["a"], Record::with_attr([("weight", Value::from("heavy"))])).unwrap();
    let err = find_path::<_, _, GraphError>(&g, ix["e"], ix["a"], &mut PathQuery::dijkstra()).unwrap_err();
    assert_eq!(err, GraphError::InvalidType("Edge attribute 'weight' must be a number".into()));
}

#[test]
fn astar_matches_dijkstra_on_a_grid() {
    let n = 12;
    let mut g = G::new();
    let mut at = HashMap::new();
    for x in 0..n {
        for y in 0..n {
            let r = Record::with_attr([("x", Value::from(x as f64)), ("y", Value::from(y as f64))]);
            at.insert((x, y), g.add_node(format!("{x},{y}"), r).unwrap());
        }
    }
    for x in 0..n {
        for y in 0..n {
            for (dx, dy) in [(1, 0), (0, 1)] {
                if let Some(&to) = at.get(&(x + dx, y + dy)) {
                    let w = 1.0 + ((x * 7 + y * 3) % 5) as f64;
                    g.add_edge(at[&(x, y)], to, weighted(w)).unwrap();
                    g.add_edge(to, at[&(x, y)], weighted(w)).unwrap();
                }
            }
        }
    }
    let (s, t) = (at[&(0, 0)], at[&(n - 1, n - 1)]);
    let d = find_path::<_, _, GraphError>(&g, s, t, &mut PathQuery::dijkstra()).unwrap().unwrap();
    let h = Heuristic::coords(&g, t, Metric::Euclidean, Coords::default()).unwrap();
    let a = find_path::<_, _, GraphError>(&g, s, t, &mut PathQuery::astar(h)).unwrap().unwrap();
    assert_eq!(a.cost, d.cost);
    assert!(a.expanded.unwrap() <= d.expanded.unwrap());

    // Custom (table) heuristic
    let zero = Heuristic::Custom(Box::new(|_| Ok(0.0)));
    let c = find_path::<_, _, GraphError>(&g, s, t, &mut PathQuery::astar(zero)).unwrap().unwrap();
    assert_eq!(c.cost, d.cost);
}

#[test]
fn batch_queries_match_single_queries() {
    use ironweaver_core::batch::{distances, shortest_paths, Snapshot};
    use ironweaver_core::pathfinding::EdgeCost;

    // Deterministic pseudo-random graph (LCG), 400 nodes, 2000 edges.
    let mut state = 12345u64;
    let mut next = move |m: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % m
    };
    let mut g = G::new();
    let ix: Vec<NodeIx> = (0..400).map(|i| g.add_node(format!("n{i}"), Record::default()).unwrap()).collect();
    for _ in 0..2000 {
        let (a, b, w) = (next(400) as usize, next(400) as usize, 1.0 + next(90) as f64 / 10.0);
        g.add_edge(ix[a], ix[b], weighted(w)).unwrap();
    }
    let pairs: Vec<(NodeIx, NodeIx)> = (0..200).map(|_| (ix[next(400) as usize], ix[next(400) as usize])).collect();

    for dir in [Direction::Out, Direction::In, Direction::Both] {
        let snap = Snapshot::build::<_, _, GraphError>(&g, dir, &EdgeCost::weighted(None, None)).unwrap();
        let batch = shortest_paths(&snap, &pairs, None).unwrap();
        for (&(s, t), got) in pairs.iter().zip(&batch) {
            let mut q = PathQuery::dijkstra();
            q.direction = dir;
            let single = find_path::<_, _, GraphError>(&g, s, t, &mut q).unwrap();
            match (single, got) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    assert!((a.cost - b.cost).abs() < 1e-9);
                    assert_eq!((b.nodes[0], *b.nodes.last().unwrap()), (s, t));
                }
                (a, b) => panic!("mismatch for {s:?}->{t:?}: {a:?} vs {b:?}"),
            }
        }
        // Distances agree with the pair queries
        let sources: Vec<NodeIx> = pairs.iter().map(|p| p.0).take(20).collect();
        for (&s, reached) in sources.iter().zip(distances(&snap, &sources, None).unwrap()) {
            let d: HashMap<NodeIx, f64> = reached.into_iter().collect();
            for (&(ps, t), got) in pairs.iter().zip(&batch) {
                if ps == s {
                    match (d.get(&t), got) {
                        (None, None) => {}
                        (Some(a), Some(b)) => assert!((a - b.cost).abs() < 1e-9),
                        (a, b) => panic!("distance mismatch: {a:?} vs {b:?}"),
                    }
                }
            }
        }
    }
}

#[test]
fn registry_resolution_and_validation() {
    assert_eq!(pathfinding::resolve(None, false, &[]).unwrap().name, "bfs");
    assert_eq!(pathfinding::resolve(None, true, &[]).unwrap().name, "dijkstra");
    assert_eq!(pathfinding::resolve(None, true, &["coords"]).unwrap().name, "astar");
    assert!(pathfinding::resolve(Some("nope"), false, &[]).is_err());
    let bfs = pathfinding::resolve(Some("bfs"), false, &[]).unwrap();
    assert!(pathfinding::check_options(bfs, &["max_depth"]).is_ok());
    assert!(matches!(pathfinding::check_options(bfs, &["coords"]), Err(GraphError::InvalidType(_))));
    assert!(pathfinding::edge_cost(bfs, Some("w".into()), None).is_err());
}

#[test]
fn random_walks_are_seeded() {
    let (g, _) = diamond();
    let mut opts = WalkOptions::new(4, 50);
    opts.seed = Some(7);
    opts.include_edge_types = true;
    let a = random_walks::<_, _, GraphError>(&g, Some("a"), opts.clone()).unwrap();
    let b = random_walks::<_, _, GraphError>(&g, Some("a"), opts.clone()).unwrap();
    assert_eq!(a, b);
    assert!(a.iter().all(|w| w[0] == "a"));
    assert!(a.iter().any(|w| w.contains(&"unknown".to_string())));

    let mut strat = WalkOptions::new(3, 30);
    strat.stratified = true;
    strat.seed = Some(1);
    assert!(!random_walks::<_, _, GraphError>(&g, None, strat).unwrap().is_empty());

    let err = random_walks::<_, _, GraphError>(&g, None, WalkOptions::new(3, 1)).unwrap_err();
    assert_eq!(err.to_string(), "start_node_id may only be None when stratified=True");
    let err = random_walks::<_, _, GraphError>(&g, Some("zz"), WalkOptions::new(3, 1)).unwrap_err();
    assert_eq!(err.to_string(), "Start node with id 'zz' not found");
}

#[test]
fn format_round_trips() {
    let (mut g, ix) = diamond();
    g.node_mut(ix["a"]).unwrap().data.meta.insert("note".into(), Value::from("m"));
    g.node_mut(ix["b"])
        .unwrap()
        .data
        .attr
        .insert("tags".into(), Value::List(vec![Value::from("x"), Value::Int(2), Value::None]));
    let meta = HashMap::from([("title".to_string(), Value::from("t"))]);

    let json = format::to_json(&g, &meta, false).unwrap();
    let (loaded, loaded_meta) = format::from_json(&json).unwrap();
    assert_eq!(loaded_meta, meta);
    assert_same(&g, &loaded);

    let bin = format::to_binary(&g, &meta, false).unwrap();
    let (loaded, _) = format::from_binary(&bin).unwrap();
    assert_same(&g, &loaded);

    let half = format::to_binary(&g, &meta, true).unwrap();
    let (loaded, _) = format::from_binary(&half).unwrap();
    let w = loaded.edges().find(|(_, e)| e.data.attr.contains_key("weight")).unwrap().1;
    assert!(matches!(w.data.attr["weight"], Value::Half(_)));
}

fn assert_same(a: &G, b: &G) {
    assert_eq!(a.node_count(), b.node_count());
    assert_eq!(a.edge_count(), b.edge_count());
    for (_, n) in a.nodes() {
        let m = b.node_by_id(n.id()).unwrap();
        assert_eq!(n.data, m.data);
        let targets = |g: &G, edges: &[ironweaver_core::EdgeIx]| -> Vec<String> {
            edges.iter().map(|&e| g.node(g.edge(e).unwrap().target()).unwrap().id().to_string()).collect()
        };
        assert_eq!(targets(a, n.out_edges()), targets(b, m.out_edges()));
        assert_eq!(n.in_edges().len(), m.in_edges().len());
    }
}

#[test]
fn legacy_files_load() {
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/");
    let json = std::fs::read(format!("{data}legacy_graph.json")).unwrap();
    let bin = std::fs::read(format!("{data}legacy_graph.bin")).unwrap();
    let half = std::fs::read(format!("{data}legacy_graph_f16.bin")).unwrap();
    for (g, meta) in
        [format::from_json(&json).unwrap(), format::from_binary(&bin).unwrap(), format::from_binary(&half).unwrap()]
    {
        assert_eq!(meta["title"], Value::from("legacy"));
        let mut ids: Vec<&str> = g.nodes().map(|(_, n)| n.id()).collect();
        ids.sort();
        assert_eq!(ids, ["a", "b", "c"]);
        let a = g.node_by_id("a").unwrap();
        assert_eq!(a.data.attr["name"], Value::from("alpha"));
        assert_eq!(a.data.meta["note"], Value::from("m"));
        assert_eq!(a.out_edges().len(), 2);
        assert_eq!(a.in_edges().len(), 1);
    }
}

#[test]
fn missing_endpoint_is_an_error() {
    let doc = br#"{"nodes":{"a":{"id":"a","attr":{},"meta":{},"edge_ids":[],"inverse_edge_ids":[]}},
                  "edges":{"e":{"id":"e","from_id":"a","to_id":"zz","attr":{},"meta":{}}},
                  "meta":{},"metadata":{}}"#;
    assert_eq!(format::from_json(doc).unwrap_err(), GraphError::InvalidArgument("To node zz not found".into()));
}
