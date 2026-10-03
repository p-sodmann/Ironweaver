// End-to-end tests of the public core API on `Graph<Record, Record>`.

use std::collections::HashMap;

use ironweaver_core::format;
use ironweaver_core::pathfinding::{self, find_path, Coords, Heuristic, Metric, PathQuery};
use ironweaver_core::random_walks::{random_walks, WalkOptions};
use ironweaver_core::{Date, DateTime, Direction, Graph, GraphError, NodeIx, Record, Value};

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
    use ironweaver_core::batch::{distances, shortest_paths};
    use ironweaver_core::pathfinding::EdgeCost;
    use ironweaver_core::Projection;

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
        let p = Projection::build::<_, _, GraphError>(&g, dir, &EdgeCost::weighted(None, None)).unwrap();
        let dense: Vec<(u32, u32)> =
            pairs.iter().map(|&(s, t)| (p.index_of(s).unwrap(), p.index_of(t).unwrap())).collect();
        let batch = shortest_paths(&p, &dense, true, None).unwrap();
        let hops = shortest_paths(&p, &dense, false, None).unwrap();
        for ((&(s, t), got), got_hops) in pairs.iter().zip(&batch).zip(&hops) {
            let mut q = PathQuery::dijkstra();
            q.direction = dir;
            let single = find_path::<_, _, GraphError>(&g, s, t, &mut q).unwrap();
            match (single, got) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    assert!((a.cost - b.cost).abs() < 1e-9);
                    assert_eq!((p.node(b.nodes[0]), p.node(*b.nodes.last().unwrap())), (s, t));
                    // Consecutive nodes are adjacent in the projection
                    for w in b.nodes.windows(2) {
                        assert!(p.out_neighbors(w[0]).binary_search(&w[1]).is_ok());
                    }
                }
                (a, b) => panic!("mismatch for {s:?}->{t:?}: {a:?} vs {b:?}"),
            }
            let mut q = PathQuery::bfs();
            q.direction = dir;
            let single = find_path::<_, _, GraphError>(&g, s, t, &mut q).unwrap();
            assert_eq!(single.map(|a| a.cost), got_hops.as_ref().map(|b| b.cost));
        }
        // Distances agree with the pair queries
        let sources: Vec<u32> = dense.iter().map(|d| d.0).take(20).collect();
        for (&s, reached) in sources.iter().zip(distances(&p, &sources, true, None).unwrap()) {
            let d: HashMap<u32, f64> = reached.into_iter().collect();
            for (&(ps, t), got) in dense.iter().zip(&batch) {
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
    let when: DateTime = "2024-05-01T12:30:00.000001-03:30".parse().unwrap();
    let c = &mut g.node_mut(ix["c"]).unwrap().data.attr;
    c.insert("when".into(), Value::DateTime(when));
    c.insert("local".into(), Value::DateTime("1969-07-20T20:17:40".parse().unwrap()));
    c.insert("day".into(), Value::Date(Date::from_ymd(1900, 3, 1).unwrap()));
    c.insert("raw".into(), Value::Bytes((0..=255).collect()));
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

/// Floats that need care in JSON: signed zeros, NaN, the infinities,
/// subnormals and extremes, at full and half precision.
fn special_floats() -> Vec<Value> {
    use half::f16;
    let floats = [
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE / 4.0,
        -f64::MIN_POSITIVE / 4.0,
        f64::MAX,
        f64::MIN,
        5e-324,
    ];
    let halves = [0.0, -0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e-7, 65504.0];
    let floats = floats.into_iter().map(Value::Float);
    floats.chain(halves.into_iter().map(|h| Value::Half(f16::from_f32(h)))).collect()
}

/// `v` as bits, so -0.0 differs from 0.0 and NaN equals NaN.
fn float_bits(v: &Value) -> (bool, u64) {
    match v {
        Value::Float(f) if f.is_nan() => (false, f64::NAN.to_bits()),
        Value::Float(f) => (false, f.to_bits()),
        Value::Half(h) => (true, u64::from(h.to_bits())),
        other => panic!("not a float: {other:?}"),
    }
}

#[test]
fn special_floats_round_trip() {
    let values = special_floats();
    let mut g = G::new();
    let list = Value::List(values.clone());
    g.add_node("a", Record::with_attr([("list", list)])).unwrap();
    for (i, v) in values.iter().enumerate() {
        g.add_node(format!("n{i}"), Record::with_attr([("x", v.clone())])).unwrap();
    }
    let meta = HashMap::new();
    let check = |loaded: &G, how: &str| {
        let Value::List(items) = &loaded.node_by_id("a").unwrap().data.attr["list"] else { panic!() };
        let want: Vec<_> = values.iter().map(float_bits).collect();
        assert_eq!(items.iter().map(float_bits).collect::<Vec<_>>(), want, "{how}");
        for (i, v) in values.iter().enumerate() {
            let x = &loaded.node_by_id(&format!("n{i}")).unwrap().data.attr["x"];
            assert_eq!(float_bits(x), float_bits(v), "{how}: {v:?}");
        }
    };
    for pretty in [false, true] {
        let json = format::to_json(&g, &meta, pretty).unwrap();
        check(&format::from_json(&json).unwrap().0, "json");
    }
    let bin = format::to_binary(&g, &meta, false).unwrap();
    check(&format::from_binary(&bin).unwrap().0, "binary");
    check(&format::from_binary_reader(&bin[..]).unwrap().0, "binary stream");
}

#[test]
fn json_floats_keep_their_sign() {
    let mut g = G::new();
    g.add_node("a", Record::with_attr([("x", Value::Float(12345.5))])).unwrap();
    let json = String::from_utf8(format::to_json(&g, &HashMap::new(), false).unwrap()).unwrap();
    assert!(json.contains(r#"{"Float":12345.5}"#), "unexpected document: {json}");
    let with_x = |x: &str| json.replace("12345.5", x);
    for (text, want) in [("-0", -0.0), ("-0.0", -0.0), ("-0e0", -0.0), ("-0.000E+12", -0.0), ("0", 0.0), ("-1.5", -1.5)]
    {
        let (g, _) = format::from_json(with_x(text).as_bytes()).unwrap();
        let Value::Float(f) = g.node_by_id("a").unwrap().data.attr["x"] else { panic!() };
        assert_eq!(f.to_bits(), f64::to_bits(want), "{text}");
    }
    for bad in ["1e400", r#""nan""#, r#""1.5""#, "null", "true"] {
        let err = format::from_json(with_x(bad).as_bytes()).err().unwrap_or_else(|| panic!("{bad} loaded"));
        assert!(err.to_string().contains("Float"), "{bad}: {err}");
    }
}

fn assert_same(a: &G, b: &G) {
    assert_eq!(a.node_count(), b.node_count());
    assert_eq!(a.edge_count(), b.edge_count());
    for (_, n) in a.nodes() {
        let m = b.node_by_id(n.id()).unwrap();
        assert_eq!(n.data, m.data);
        for (k, v) in &n.data.attr {
            if let Value::DateTime(t) = v {
                // Same instant and the same offset
                assert!(matches!(m.data.attr[k], Value::DateTime(u) if u.offset == t.offset), "{k}");
            }
        }
        let targets = |g: &G, edges: &[ironweaver_core::EdgeIx]| -> Vec<String> {
            edges.iter().map(|&e| g.node(g.edge(e).unwrap().target()).unwrap().id().to_string()).collect()
        };
        assert_eq!(targets(a, n.out_edges()), targets(b, m.out_edges()));
        assert_eq!(n.in_edges().len(), m.in_edges().len());
    }
}

#[test]
fn legacy_json_loads_and_legacy_binary_is_refused() {
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/");
    let json = std::fs::read(format!("{data}legacy_graph.json")).unwrap();
    let bin = std::fs::read(format!("{data}legacy_graph.bin")).unwrap();
    let half = std::fs::read(format!("{data}legacy_graph_f16.bin")).unwrap();
    // Version 1 binary files are no longer read: a clear error that says
    // how to convert them
    for bytes in [&bin, &half] {
        for err in [format::from_binary(bytes).err().unwrap(), format::from_binary_reader(&bytes[..]).err().unwrap()] {
            assert!(matches!(err, GraphError::Format(_)), "{err}");
            let err = err.to_string();
            assert!(err.contains("unsupported ironweaver binary format version 1"), "{err}");
            assert!(err.contains("save_to_json"), "{err}");
        }
    }
    // Other files without the header are not taken for version 1 files
    let mut v2 = format::to_binary(&Graph::<Record, Record>::new(), &HashMap::new(), false).unwrap();
    v2[0] = b'X';
    for bytes in [&json, &v2, &b"IRONWEAV"[..5].to_vec(), &vec![0xff; 64]] {
        for err in [format::from_binary(bytes).err().unwrap(), format::from_binary_reader(&bytes[..]).err().unwrap()] {
            let err = err.to_string();
            assert!(err.starts_with("invalid ironweaver binary file"), "{err}");
            assert!(!err.contains("version 1"), "{err}");
        }
    }
    // Version 1 JSON files still load
    let (g, meta) = format::from_json(&json).unwrap();
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
        // Version 1 conventions migrate: edge attr "type" -> the edge type
        let ab = a.out_edges()[0];
        assert_eq!(g.edge_type_name(ab), Some("knows"));
        assert!(!g.edge(ab).unwrap().data.attr.contains_key("type"));
        assert!(g.edge(ab).unwrap().data.attr["weight"].loose_eq(&Value::from(0.25)));
    }
}

#[test]
fn format_v2_keeps_ids_labels_and_types() {
    use ironweaver_core::EdgeId;
    let (mut g, ix) = diamond();
    g.add_label(ix["a"], "Start").unwrap();
    g.add_label(ix["a"], "Node").unwrap();
    let e = g.insert_edge(ix["e"], ix["a"], None, Some("back"), Record::default()).unwrap();
    let back = g.edge(e).unwrap().id();
    // Gaps: remove an edge, and the newest one's successor
    let first = g.edge_ix(EdgeId(0)).unwrap();
    g.remove_edge(first).unwrap();
    let tmp = g.add_edge(ix["a"], ix["b"], Record::default()).unwrap();
    g.remove_edge(tmp).unwrap();
    let next = g.next_edge_id();
    let meta = HashMap::new();
    for loaded in [
        format::from_json(&format::to_json(&g, &meta, false).unwrap()).unwrap().0,
        format::from_binary(&format::to_binary(&g, &meta, false).unwrap()).unwrap().0,
    ] {
        assert_same(&g, &loaded);
        let a = loaded.node_ix("a").unwrap();
        assert_eq!(loaded.label_names(a).unwrap(), ["Start", "Node"]);
        assert_eq!(loaded.nodes_with_label("Node"), [a]);
        let mut want: Vec<EdgeId> = g.edges().map(|(_, e)| e.id()).collect();
        let mut got: Vec<EdgeId> = loaded.edges().map(|(_, e)| e.id()).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want);
        assert_eq!(loaded.edge_type_name(loaded.edge_ix(back).unwrap()), Some("back"));
        assert_eq!(loaded.edge_ix(EdgeId(0)), None);
        assert_eq!(loaded.next_edge_id(), next); // removed ids are not reused
    }
    let json = String::from_utf8(format::to_json(&g, &meta, false).unwrap()).unwrap();
    assert!(json.contains(r#""version":{"String":"2.0"}"#), "{json}");
    assert!(format::from_json(json.replace(r#"{"String":"2.0"}"#, r#"{"String":"3.0"}"#).as_bytes()).is_err());
}

#[test]
fn missing_endpoint_is_an_error() {
    let doc = br#"{"nodes":{"a":{"id":"a","attr":{},"meta":{},"edge_ids":[],"inverse_edge_ids":[]}},
                  "edges":{"e":{"id":"e","from_id":"a","to_id":"zz","attr":{},"meta":{}}},
                  "meta":{},"metadata":{}}"#;
    assert_eq!(format::from_json(doc).unwrap_err(), GraphError::InvalidArgument("To node zz not found".into()));
}
