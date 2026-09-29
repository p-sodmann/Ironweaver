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

    // Version 1 (bincode): wrap the 3-item list at byte 171 of the legacy
    // file in many one-item lists (variant 6 as u32, length 1 as u64)
    let legacy = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/legacy_graph.bin")).unwrap();
    assert_eq!(legacy[171..175], 6u32.to_le_bytes());
    let mut v1_level = 6u32.to_le_bytes().to_vec();
    v1_level.extend(1u64.to_le_bytes());
    let mut crafted = legacy[..171].to_vec();
    for _ in 0..100_000 {
        crafted.extend(&v1_level);
    }
    crafted.extend(&legacy[171..]);
    let err = format::from_binary(&crafted).err().unwrap();
    assert!(err.to_string().contains("nested more than"), "{err}");
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
