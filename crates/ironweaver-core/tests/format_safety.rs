// Loading untrusted documents and saving files safely.

use ironweaver_core::format::{self, MAX_DEPTH};
use ironweaver_core::{Attrs, Graph, Record, Value};

type G = Graph<Record, Record>;

/// A graph with one node whose attribute `k` is `depth` levels deep
/// (`depth - 1` nested lists around the string "MARK").
fn nested(depth: usize) -> G {
    let mut v = Value::from("MARK");
    for _ in 1..depth {
        v = Value::List(vec![v]);
    }
    let mut g = G::new();
    g.add_node("a", Record::with_attr([("k", v)])).unwrap();
    g
}

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).position(|w| w == needle).unwrap()
}

#[test]
fn deepest_allowed_value_round_trips() {
    let g = nested(MAX_DEPTH);
    let meta = Attrs::new();
    for half in [false, true] {
        let bytes = format::to_binary(&g, &meta, half).unwrap();
        format::from_binary(&bytes).unwrap();
    }
    let json = format::to_json(&g, &meta, false).unwrap();
    format::from_json(&json).unwrap();
}

#[test]
fn saving_too_deep_values_fails() {
    let g = nested(MAX_DEPTH + 1);
    let meta = Attrs::new();
    let err = format::to_binary(&g, &meta, false).unwrap_err();
    assert!(err.to_string().contains("nested more than"), "{err}");
    assert!(format::to_json(&g, &meta, false).is_err());
}

#[test]
fn loading_too_deep_json_fails() {
    let json = String::from_utf8(format::to_json(&nested(MAX_DEPTH), &Attrs::new(), false).unwrap()).unwrap();
    let deeper = json.replace(r#"{"String":"MARK"}"#, r#"{"List":[{"String":"MARK"}]}"#);
    let err = format::from_json(deeper.as_bytes()).err().unwrap();
    assert!(err.to_string().contains("nested more than"), "{err}");
}

#[test]
fn loading_json_with_crafted_unknown_fields_fails_without_overflowing_the_stack() {
    // Unknown fields are skipped, which recurses in the JSON parser
    let json = String::from_utf8(format::to_json(&nested(MAX_DEPTH), &Attrs::new(), false).unwrap()).unwrap();
    let junk = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
    for (from, to) in
        [(r#"{"nodes":"#, format!(r#"{{"junk":{junk},"nodes":"#)), (r#""attr":"#, format!(r#""junk":{junk},"attr":"#))]
    {
        let crafted = json.replacen(from, &to, 1);
        assert_ne!(crafted, json);
        let err = format::from_json(crafted.as_bytes()).err().unwrap();
        assert!(err.to_string().contains("JSON nested more than 255 levels deep"), "{err}");
    }
    // Brackets inside strings don't count
    let strings = json.replacen(r#"{"nodes":"#, &format!(r#"{{"junk":"\\\"{}","nodes":"#, "[{".repeat(1000)), 1);
    format::from_json(strings.as_bytes()).unwrap();
}

/// Re-frame a binary file around a modified payload (fresh length and CRC).
fn reframe(file: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = file[..16].to_vec();
    out.extend_from_slice(payload);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    out.extend_from_slice(b"IWND");
    out
}

#[test]
fn loading_crafted_deep_binary_fails_without_overflowing_the_stack() {
    // Version 2 (postcard): a one-item list is variant 6 then length 1,
    // both one-byte varints
    let level = [6u8, 1];
    let file = format::to_binary(&nested(2), &Attrs::new(), false).unwrap();
    let payload = &file[16..file.len() - 16];
    // The existing list header sits right before the string's tag and length
    let at = find(payload, b"MARK") - 2 - level.len();
    assert_eq!(payload[at..at + 2], level);
    for extra in [MAX_DEPTH - 1, 100_000] {
        let mut crafted = payload[..at].to_vec();
        for _ in 0..extra {
            crafted.extend(level);
        }
        crafted.extend(&payload[at..]);
        let err = format::from_binary(&reframe(&file, &crafted)).err().unwrap();
        assert!(err.to_string().contains("nested more than"), "{err}");
    }
}

#[test]
fn damaged_binary_files_are_rejected() {
    let file = format::to_binary(&nested(3), &Attrs::new(), false).unwrap();
    format::from_binary(&file).unwrap();
    let check = |bytes: &[u8], want: &str| {
        let err = format::from_binary(bytes).err().unwrap().to_string();
        assert!(err.contains(want), "{want}: {err}");
    };
    check(&file[..file.len() - 1], "truncated");
    check(&file[..20], "truncated");
    let mut short = file[..30].to_vec(); // payload cut, trailer kept
    short.extend(&file[file.len() - 16..]);
    check(&short, "length mismatch");
    let mut flipped = file.clone();
    flipped[20] ^= 1;
    check(&flipped, "checksum mismatch");
    let mut newer = file.clone();
    newer[8] = 3;
    check(&newer, "version 3 is not supported");
    // The header's flags and reserved bytes, for both loaders
    let header = |at: usize, byte: u8| {
        let mut bytes = file.clone();
        bytes[at] = byte;
        bytes
    };
    for (bytes, want) in [
        (header(10, 0x01), "flags 0x0001 are not supported"),
        (header(11, 0x80), "flags 0x8000 are not supported"),
        (header(12, 0xde), "reserved header bytes are not zero"),
        (header(15, 0x01), "reserved header bytes are not zero"),
    ] {
        check(&bytes, want);
        let err = format::from_binary_reader(&bytes[..]).err().unwrap().to_string();
        assert!(err.contains(want), "{want}: {err}");
    }
    // A valid frame around garbage: a parse error, not a panic
    check(&reframe(&file, &[0xff; 40]), "");
}

#[test]
fn write_atomic_replaces_the_file_or_leaves_it_alone() {
    let dir = std::env::temp_dir().join(format!("ironweaver-atomic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("graph.json");

    format::write_atomic(&path, |out| std::io::Write::write_all(out, b"first")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"first");

    // A failing writer keeps the old content and leaves no temporary file
    let err = format::write_atomic(&path, |out| {
        std::io::Write::write_all(out, b"partial")?;
        Err(std::io::Error::other("boom"))
    })
    .unwrap_err();
    assert_eq!(err.to_string(), "boom");
    assert_eq!(std::fs::read(&path).unwrap(), b"first");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

    format::write_atomic(&path, |out| std::io::Write::write_all(out, b"second")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

    // A missing directory is an error, not a panic
    assert!(format::write_atomic(dir.join("missing/graph.json"), |_| Ok(())).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn write_atomic_reports_a_failed_directory_sync() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("ironweaver-atomic-dir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Writable but not readable: the file can be created and renamed, but
    // the directory can't be opened to fsync it
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o300)).unwrap();
    let path = dir.join("graph.json");
    let result = format::write_atomic(&path, |out| std::io::Write::write_all(out, b"data"));
    let readable = std::fs::File::open(&dir).is_ok(); // root reads anything
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    if readable {
        result.unwrap();
    } else {
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("syncing the directory"), "{err}");
    }
    // The rename itself happened; no temporary file is left
    assert_eq!(std::fs::read(&path).unwrap(), b"data");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(windows)]
#[test]
fn write_atomic_reports_a_failed_directory_sync_on_windows() {
    use std::process::Command;

    let identity = Command::new("whoami").output().unwrap();
    assert!(identity.status.success(), "whoami failed: {identity:?}");
    let user = String::from_utf8(identity.stdout).unwrap();
    let user = user.trim();
    let dir = std::env::temp_dir().join(format!("ironweaver-atomic-dir-windows-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Deny extended-attribute writes on the directory only (not inherited
    // by children). Files can still be created and renamed, but the
    // directory cannot be opened with GENERIC_WRITE to sync it.
    let deny = Command::new("icacls").arg(&dir).arg("/deny").arg(format!("{user}:(WEA)")).output().unwrap();
    assert!(deny.status.success(), "icacls /deny failed: {deny:?}");
    let path = dir.join("graph.json");
    let result = format::write_atomic(&path, |out| std::io::Write::write_all(out, b"data"));
    // Restore the ACL before any assertions, even if the save failed early.
    let restore = Command::new("icacls").arg(&dir).arg("/remove:d").arg(user).output().unwrap();
    assert!(restore.status.success(), "icacls /remove:d failed: {restore:?}");
    let content = std::fs::read(&path);
    let entries = std::fs::read_dir(&dir).unwrap().count();
    std::fs::remove_dir_all(&dir).unwrap();

    let err = result.unwrap_err();
    assert!(err.to_string().contains("syncing the directory"), "{err}");
    assert!(err.to_string().contains(&dir.display().to_string()), "{err}");
    assert_eq!(content.unwrap(), b"data");
    assert_eq!(entries, 1);
}

#[test]
fn equal_graphs_save_to_equal_bytes() {
    use format::{GraphWriter, RecordCodec};
    // Each HashMap has its own random seed, so equal maps usually iterate
    // in different orders
    let build = |reverse: bool| {
        let mut keys: Vec<usize> = (0..50).collect();
        if reverse {
            keys.reverse();
        }
        let dict: Attrs = keys.iter().map(|k| (format!("d{k}"), Value::from(*k as i64))).collect();
        let mut attr: Attrs = keys.iter().map(|k| (format!("k{k}"), Value::from(*k as i64))).collect();
        attr.insert("dict".into(), Value::Dict(dict));
        let mut g = G::new();
        let a = g.add_node("a", Record { attr: attr.clone(), meta: attr.clone() }).unwrap();
        g.add_edge(a, a, Record { attr, meta: Attrs::new() }).unwrap();
        g
    };
    let (g1, g2) = (build(false), build(true));
    let meta: Attrs = (0..20).map(|k| (format!("m{k}"), Value::from(k as i64))).collect();
    let meta2: Attrs = meta.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let save = |g: &G, meta: &Attrs| {
        let codec = RecordCodec { meta, half: false };
        let mut bin = Vec::new();
        GraphWriter::new(g, &codec).with_timestamp(None).write_binary(&mut bin).unwrap();
        let json = GraphWriter::new(g, &codec).with_timestamp(Some("fixed".into())).to_json(false).unwrap();
        (bin, json)
    };
    let (bin1, json1) = save(&g1, &meta);
    let (bin2, json2) = save(&g2, &meta2);
    assert_eq!(bin1, bin2);
    assert_eq!(json1, json2);
    let text = String::from_utf8(json1).unwrap();
    assert!(text.contains(r#""timestamp":{"String":"fixed"}"#), "{text}");
    assert!(text.find(r#""k0""#).unwrap() < text.find(r#""k1""#).unwrap());
    format::from_binary(&bin1).unwrap();
    // Record and Value serde (ops in a log) sort keys too
    let r1 = sonic_rs::to_string(&g1.node_by_id("a").unwrap().data).unwrap();
    let r2 = sonic_rs::to_string(&g2.node_by_id("a").unwrap().data).unwrap();
    assert_eq!(r1, r2);
}

/// `depth - 1` nested lists (or dicts) around an empty one: `depth` levels.
fn empty_nested(depth: usize, dict: bool) -> Value {
    let wrap = |v: Value| if dict { Value::Dict([("k".to_string(), v)].into()) } else { Value::List(vec![v]) };
    let innermost = if dict { Value::Dict(Default::default()) } else { Value::List(vec![]) };
    (1..depth).fold(innermost, |v, _| wrap(v))
}

/// The file format and `Value`'s serde (JSON, postcard, and through it
/// `Op`) agree on the depth limit, also when the innermost container is
/// empty: whatever loads can be logged and saved again.
#[test]
fn value_serde_depth_limit_matches_the_file_format() {
    type O = ironweaver_core::Op<Record, Record>;
    let op = |v: &Value| O::SetNodeAttr { id: "a".into(), key: "k".into(), value: Some(v.clone()) };
    for dict in [false, true] {
        for (depth, ok) in [(MAX_DEPTH, true), (MAX_DEPTH + 1, false)] {
            let v = empty_nested(depth, dict);
            let mut g = G::new();
            g.add_node("a", Record::with_attr([("k", v.clone())])).unwrap();
            let meta = Attrs::new();
            assert_eq!(format::to_binary(&g, &meta, false).is_ok(), ok, "file, depth {depth}, dict {dict}");
            assert_eq!(format::to_json(&g, &meta, false).is_ok(), ok, "file, depth {depth}, dict {dict}");

            let json = sonic_rs::to_string(&v);
            let bytes = postcard::to_stdvec(&v);
            let op_json = sonic_rs::to_string(&op(&v));
            let op_bytes = postcard::to_stdvec(&op(&v));
            if ok {
                assert_eq!(sonic_rs::from_str::<Value>(&json.unwrap()).unwrap(), v);
                assert_eq!(postcard::from_bytes::<Value>(&bytes.unwrap()).unwrap(), v);
                assert_eq!(sonic_rs::from_str::<O>(&op_json.unwrap()).unwrap(), op(&v));
                assert_eq!(postcard::from_bytes::<O>(&op_bytes.unwrap()).unwrap(), op(&v));
            } else {
                let err = json.unwrap_err().to_string();
                assert!(err.contains("nested more than"), "{err}");
                assert!(op_json.unwrap_err().to_string().contains("nested more than"));
                assert!(bytes.is_err() && op_bytes.is_err());
            }
        }
        // Encoded input one level too deep is rejected when decoding
        let ok = sonic_rs::to_string(&empty_nested(MAX_DEPTH, dict)).unwrap();
        let deeper = if dict { format!(r#"{{"Dict":{{"k":{ok}}}}}"#) } else { format!(r#"{{"List":[{ok}]}}"#) };
        let err = sonic_rs::from_str::<Value>(&deeper).unwrap_err().to_string();
        assert!(err.contains("nested more than"), "{err}");
        let bytes = postcard::to_stdvec(&empty_nested(MAX_DEPTH, dict)).unwrap();
        let mut deeper = if dict { vec![7u8, 1, 1, b'k'] } else { vec![6u8, 1] };
        deeper.extend(&bytes);
        assert!(postcard::from_bytes::<Value>(&deeper).is_err());
        // A list one level shallower decodes
        assert!(postcard::from_bytes::<Value>(&deeper[if dict { 4 } else { 2 }..]).is_ok());
    }
}

/// `Value`'s serde writes NaN and the infinities like the file format, so a
/// custom codec that writes attribute maps with `value::serialize_sorted`
/// saves JSON that loads again, and JSON `Op`s carry them too.
#[test]
fn value_serde_non_finite_floats_match_the_file_format() {
    use format::{Codec, GraphWriter};
    use ironweaver_core::value::serialize_sorted;
    use serde::Serializer;

    struct Plain;
    impl Codec<Record, Record> for Plain {
        fn node_attr<S: Serializer>(&self, n: &Record, s: S) -> Result<S::Ok, S::Error> {
            serialize_sorted(&n.attr, s)
        }
        fn node_meta<S: Serializer>(&self, n: &Record, s: S) -> Result<S::Ok, S::Error> {
            serialize_sorted(&n.meta, s)
        }
        fn edge_attr<S: Serializer>(&self, e: &Record, s: S) -> Result<S::Ok, S::Error> {
            serialize_sorted(&e.attr, s)
        }
        fn edge_meta<S: Serializer>(&self, e: &Record, s: S) -> Result<S::Ok, S::Error> {
            serialize_sorted(&e.meta, s)
        }
        fn graph_meta<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            serialize_sorted(&Attrs::new(), s)
        }
    }

    let floats = [("nan", f64::NAN), ("inf", f64::INFINITY), ("ninf", f64::NEG_INFINITY), ("one", 1.5)];
    let mut g = G::new();
    g.add_node("a", Record::with_attr(floats.map(|(k, f)| (k, Value::Float(f))))).unwrap();
    let json = GraphWriter::new(&g, &Plain).with_timestamp(None).to_json(false).unwrap();
    let (loaded, _) = format::from_json(&json).unwrap();
    let attr = &loaded.node_by_id("a").unwrap().data.attr;
    for (k, f) in floats {
        match attr[k] {
            Value::Float(v) => assert_eq!(v.to_bits(), f.to_bits(), "{k}"),
            ref other => panic!("{k}: {other:?}"),
        }
    }

    type O = ironweaver_core::Op<Record, Record>;
    let op = O::SetNodeAttr { id: "a".into(), key: "k".into(), value: Some(Value::Float(f64::NEG_INFINITY)) };
    let text = sonic_rs::to_string(&op).unwrap();
    assert!(text.contains(r#"{"Float":"-Infinity"}"#), "{text}");
    assert_eq!(sonic_rs::from_str::<O>(&text).unwrap(), op);
}
