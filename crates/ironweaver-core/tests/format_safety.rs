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
fn loading_crafted_deep_binary_fails_without_overflowing_the_stack() {
    // Binary layout of a one-item list: variant tag 6 (u32), length 1 (u64)
    let mut level = 6u32.to_le_bytes().to_vec();
    level.extend(1u64.to_le_bytes());

    let bytes = format::to_binary(&nested(2), &Attrs::new(), false).unwrap();
    // The existing list header sits right before the string's tag and length
    let at = find(&bytes, b"MARK") - 12 - level.len();
    assert_eq!(bytes[at..at + level.len()], level[..]);
    for extra in [MAX_DEPTH - 1, 100_000] {
        let mut crafted = bytes[..at].to_vec();
        for _ in 0..extra {
            crafted.extend(&level);
        }
        crafted.extend(&bytes[at..]);
        let err = format::from_binary(&crafted).err().unwrap();
        assert!(err.to_string().contains("nested more than"), "{err}");
    }
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
