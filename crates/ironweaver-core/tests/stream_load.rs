// Loading binary files from a reader: the same graphs as the slice-based
// loader, the same errors for damaged files, whatever the read sizes.

use std::io::Read;

use ironweaver_core::format::{self, LoadGraph};
use ironweaver_core::{Attrs, Date, DateTime, Graph, GraphError, Record, Value};

type G = Graph<Record, Record>;

/// A reader handing out 1..=7 bytes per call.
struct Trickle<'a> {
    data: &'a [u8],
    step: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.step = self.step % 7 + 1;
        let n = self.step.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

/// A value as text, dict keys sorted.
fn canon(v: &Value) -> String {
    match v {
        Value::List(items) => format!("[{}]", items.iter().map(canon).collect::<Vec<_>>().join(", ")),
        Value::Dict(d) => canon_map(d),
        other => format!("{other:?}"),
    }
}

fn canon_map(m: &Attrs) -> String {
    let mut items: Vec<String> = m.iter().map(|(k, v)| format!("{k}={}", canon(v))).collect();
    items.sort();
    format!("{{{}}}", items.join(", "))
}

/// Everything a load must restore, in order.
fn state(g: &G) -> Vec<String> {
    let mut out = Vec::new();
    for (ix, n) in g.nodes() {
        let (attr, meta) = (canon_map(&n.data.attr), canon_map(&n.data.meta));
        let edges = |list: &[ironweaver_core::EdgeIx]| -> Vec<u64> {
            list.iter().map(|&e| g.edge(e).unwrap().id().0).collect()
        };
        out.push(format!(
            "{} {:?} {attr} {meta} out {:?} in {:?}",
            n.id(),
            g.label_names(ix).unwrap(),
            edges(n.out_edges()),
            edges(n.in_edges())
        ));
    }
    // Edge slots are in document order after a load: compare sorted
    let mut edges = Vec::new();
    for (e, x) in g.edges() {
        let attr = canon_map(&x.data.attr);
        edges.push(format!(
            "edge {} {}->{} {:?} {attr}",
            x.id().0,
            g.node(x.source()).unwrap().id(),
            g.node(x.target()).unwrap().id(),
            g.edge_type_name(e)
        ));
    }
    edges.sort();
    out.extend(edges);
    out.push(format!("next {}", g.next_edge_id().0));
    out
}

fn sample(n: usize) -> G {
    let mut g = G::new();
    let mut prev = None;
    for i in 0..n {
        let mut attr = Attrs::new();
        attr.insert("i".into(), Value::from(i as i64));
        attr.insert("name".into(), Value::from(format!("node \"{i}\" \\ ü")));
        attr.insert("f".into(), Value::from(i as f64 / 3.0));
        if i % 3 == 0 {
            attr.insert("raw".into(), Value::Bytes((0..=255u8).collect()));
            attr.insert("day".into(), Value::Date(Date::from_ymd(2024, 2, 29).unwrap()));
            let when: DateTime = "2024-05-01T12:30:00.000001-03:30".parse().unwrap();
            attr.insert("when".into(), Value::DateTime(when));
            attr.insert("nested".into(), Value::List(vec![Value::None, Value::from(true), Value::Dict(attr.clone())]));
        }
        let ix = g.add_node(format!("n{i}"), Record { attr, meta: Attrs::new() }).unwrap();
        if i % 2 == 0 {
            g.add_label(ix, "Even").unwrap();
        }
        if let Some(p) = prev {
            g.insert_edge(p, ix, None, Some("next"), Record::with_attr([("w", Value::from(0.5))])).unwrap();
            g.add_edge(ix, p, Record::default()).unwrap();
            g.add_edge(ix, ix, Record::default()).unwrap();
        }
        prev = Some(ix);
    }
    // Gaps in the ids, and edges in an order that differs from their ids
    let first = g.edges().next().map(|(e, _)| e);
    if let Some(first) = first {
        g.remove_edge(first).unwrap();
    }
    if n > 3 {
        let a = g.node_ix("n3").unwrap();
        let b = g.node_ix("n1").unwrap();
        g.add_edge(b, a, Record::default()).unwrap();
    }
    g
}

#[test]
fn reader_loads_what_the_slice_loader_loads() {
    let meta: Attrs = [("title".to_string(), Value::from("t"))].into();
    // Small, and larger than the read buffer
    for n in [0, 1, 5, 3000] {
        let g = sample(n);
        for half in [false, true] {
            let bytes = format::to_binary(&g, &meta, half).unwrap();
            if n == 3000 {
                assert!(bytes.len() > 200_000, "{}", bytes.len());
            }
            let (want, want_meta) = format::from_binary(&bytes).unwrap();
            let (got, got_meta) = format::from_binary_reader(&bytes[..]).unwrap();
            assert_eq!(state(&got), state(&want));
            assert_eq!(got_meta, want_meta);
            let (trickled, _) = format::from_binary_reader(Trickle { data: &bytes, step: 0 }).unwrap();
            assert_eq!(state(&trickled), state(&want));
            if !half {
                assert_eq!(state(&got), state(&g));
            }
        }
    }
}

#[test]
fn old_and_golden_files_load_from_a_reader() {
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/");
    for name in ["legacy_graph.bin", "legacy_graph_f16.bin", "v2_graph.bin", "v2_graph_f16.bin"] {
        let bytes = std::fs::read(format!("{data}{name}")).unwrap();
        let (want, want_meta) = format::from_binary(&bytes).unwrap();
        let file = std::fs::File::open(format!("{data}{name}")).unwrap();
        let (got, got_meta) = format::from_binary_reader(file).unwrap();
        assert_eq!(state(&got), state(&want), "{name}");
        assert_eq!(got_meta, want_meta, "{name}");
    }
}

#[test]
fn damaged_files_fail_the_same_way() {
    let file = format::to_binary(&sample(40), &Attrs::new(), false).unwrap();
    let check = |bytes: &[u8], want: &str| {
        let slice = format::from_binary(bytes).err().unwrap().to_string();
        let reader = format::from_binary_reader(bytes).err().unwrap().to_string();
        let trickle = format::from_binary_reader(Trickle { data: bytes, step: 3 }).err().unwrap().to_string();
        assert!(reader.contains(want), "{want}: {reader}");
        assert_eq!(reader, trickle);
        if !want.is_empty() {
            assert!(slice.contains(want), "{want}: {slice}");
        }
    };
    check(&file[..file.len() - 1], "truncated");
    check(&file[..20], "truncated");
    check(&file[..5], "");
    let mut short = file[..300].to_vec(); // payload cut, trailer kept
    short.extend(&file[file.len() - 16..]);
    check(&short, "length mismatch");
    for at in [20, 100, file.len() / 2, file.len() - 20] {
        let mut flipped = file.clone();
        flipped[at] ^= 0x41;
        check(&flipped, "checksum mismatch");
    }
    let mut newer = file.clone();
    newer[8] = 3;
    check(&newer, "version 3 is not supported");
    let mut longer = file.clone();
    longer.extend(b"extra");
    check(&longer, "truncated (no trailer)");
    // A valid frame around garbage: a decoding error, not a panic
    let mut garbage = file[..16].to_vec();
    let payload = [0xffu8; 40];
    garbage.extend(payload);
    garbage.extend((payload.len() as u64).to_le_bytes());
    garbage.extend(crc32fast::hash(&payload).to_le_bytes());
    garbage.extend(b"IWND");
    check(&garbage, "");
}

#[test]
fn callback_errors_and_graph_errors_come_back() {
    let g = sample(10);
    let bytes = format::to_binary(&g, &Attrs::new(), false).unwrap();
    #[derive(Debug, PartialEq)]
    enum MyError {
        Graph(GraphError),
        Rejected(String),
    }
    impl From<GraphError> for MyError {
        fn from(e: GraphError) -> Self {
            MyError::Graph(e)
        }
    }
    let result = LoadGraph::build_from_reader(
        &bytes[..],
        |n| if n.id() == "n4" { Err(MyError::Rejected(n.id().to_string())) } else { Ok(()) },
        |_| Ok(()),
    );
    assert_eq!(result.err(), Some(MyError::Rejected("n4".into())));

    // Payloads made by the caller, graph-level meta returned as loaded values
    let meta: Attrs = [("k".to_string(), Value::from(1))].into();
    let bytes = format::to_binary(&g, &meta, false).unwrap();
    let (loaded, meta) = LoadGraph::build_from_reader(
        &bytes[..],
        |n| Ok::<_, GraphError>(n.labels().count()),
        |e| Ok(e.edge_type().map(str::to_owned)),
    )
    .unwrap();
    assert_eq!(meta.to_attrs()["k"], Value::from(1));
    assert_eq!(loaded.node_count(), g.node_count());
    assert_eq!(*loaded.node_by_id("n2").map(|n| &n.data).unwrap(), 1);
    assert!(loaded.edges().any(|(_, e)| e.data.as_deref() == Some("next")));
}

#[test]
fn crafted_deep_values_are_rejected() {
    let mut v = Value::from("MARK");
    for _ in 1..3 {
        v = Value::List(vec![v]);
    }
    let mut g = G::new();
    g.add_node("a", Record::with_attr([("k", v)])).unwrap();
    let file = format::to_binary(&g, &Attrs::new(), false).unwrap();
    let payload = &file[16..file.len() - 16];
    let at = payload.windows(4).position(|w| w == b"MARK").unwrap() - 4;
    let mut crafted = payload[..at].to_vec();
    for _ in 0..100_000 {
        crafted.extend([6u8, 1]);
    }
    crafted.extend(&payload[at..]);
    let mut framed = file[..16].to_vec();
    framed.extend(&crafted);
    framed.extend((crafted.len() as u64).to_le_bytes());
    framed.extend(crc32fast::hash(&crafted).to_le_bytes());
    framed.extend(b"IWND");
    let err = format::from_binary_reader(&framed[..]).err().unwrap();
    assert!(err.to_string().contains("nested more than"), "{err}");
}

#[test]
fn index_definitions_are_saved_and_rebuilt() {
    let path = |p: &str| p.split('.').map(str::to_owned).collect::<Vec<_>>();
    let mut g = sample(30);
    assert!(g.create_index::<GraphError>(&path("i")).unwrap());
    assert!(g.create_index::<GraphError>(&path("nested.day")).unwrap());
    let meta = Attrs::new();
    let bin = format::to_binary(&g, &meta, false).unwrap();
    let json = format::to_json(&g, &meta, false).unwrap();
    let text = String::from_utf8(json.clone()).unwrap();
    assert!(
        text.contains(
            r#""indexes":{"List":[{"List":[{"String":"i"}]},{"List":[{"String":"nested"},{"String":"day"}]}]}"#
        ),
        "{text}"
    );
    assert_eq!(LoadGraph::from_json_slice(&json).unwrap().index_paths().unwrap(), [path("i"), path("nested.day")]);
    for loaded in [
        format::from_binary(&bin).unwrap().0,
        format::from_binary_reader(&bin[..]).unwrap().0,
        format::from_json(&json).unwrap().0,
    ] {
        assert_eq!(loaded.index_paths(), [path("i").as_slice(), path("nested.day").as_slice()]);
        assert!(!loaded.indexes_dirty());
        let found = loaded.find_nodes(&path("i"), &Value::from(7)).unwrap().unwrap();
        assert_eq!(found.iter().map(|&n| loaded.node(n).unwrap().id()).collect::<Vec<_>>(), ["n7"]);
    }
    // The generic builder leaves them dirty but exact
    let doc = LoadGraph::from_binary_slice(&bin).unwrap();
    let built: G = doc
        .build(
            |n| Ok::<_, GraphError>(Record { attr: n.attr().to_attrs(), meta: Attrs::new() }),
            |_| Ok(Record::default()),
        )
        .unwrap();
    assert!(built.indexes_dirty());
    assert_eq!(built.find_nodes(&path("i"), &Value::from(7)).unwrap().unwrap().len(), 1);

    // No indexes: no field, so files without indexes are unchanged
    let plain = String::from_utf8(format::to_json(&sample(3), &meta, false).unwrap()).unwrap();
    assert!(!plain.contains("indexes"));
    // A malformed field is a format error
    let bad = text.replacen(r#"{"String":"i"}"#, r#"{"Int":1}"#, 1);
    assert!(matches!(format::from_json(bad.as_bytes()), Err(GraphError::Format(_))));
}
