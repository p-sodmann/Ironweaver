use ironweaver_core::{Expr, Graph, Record, Value};

fn graph(seed: Option<u64>, reverse: bool) -> Graph<Record, Record> {
    let mut g = Graph::new();
    let mut ids: Vec<_> = (0..32).map(|i| format!("n{i}")).collect();
    if reverse {
        ids.reverse();
    }
    for id in ids {
        let mut data = Record::default();
        data.attr.insert("value".into(), Value::Int(1));
        let ix = g.add_node(id, data).unwrap();
        g.add_label(ix, "Item").unwrap();
    }
    g.set_shuffle_seed(seed);
    g
}

fn ids(g: &Graph<Record, Record>) -> Vec<String> {
    g.nodes().map(|(_, n)| n.id().to_owned()).collect()
}

#[test]
fn seeded_order_is_reproducible_and_default_is_unchanged() {
    let default = graph(None, false);
    let mut g = graph(Some(1234), false);
    let order = ids(&g);
    assert_ne!(order, ids(&default));
    assert_eq!(order, ids(&graph(Some(1234), true)));
    assert_ne!(order, ids(&graph(Some(4321), false)));
    assert_eq!(order, ids(&g.clone()));
    let mutable: Vec<_> = g.nodes_mut().map(|(_, n)| n.id().to_owned()).collect();
    assert_eq!(mutable, order);
    g.set_shuffle_seed(None);
    assert_eq!(ids(&g), ids(&default));
    g.set_shuffle_seed(Some(1234));
    assert_eq!(order, ids(&g));
    let keep: Vec<_> = g.node_indices().take(10).collect();
    let sub = g.induced_subgraph(keep.clone(), |n| Ok::<_, ()>(n.data.clone()), |e| Ok(e.data.clone())).unwrap();
    assert_eq!(sub.shuffle_seed(), Some(1234));
    assert_eq!(ids(&sub), order[..10]);
    g.remove_node(keep[0]).unwrap();
    g.add_node("replacement", Record::default()).unwrap();
    let mut sorted = ids(&g);
    sorted.sort();
    assert_eq!(sorted.len(), 32);
    assert!(sorted.contains(&"replacement".into()));
}

#[test]
fn indexed_candidates_follow_node_order() {
    let mut g = graph(Some(1234), false);
    let want: Vec<_> = g.node_indices().collect();
    assert_eq!(g.nodes_with_label("Item"), want);
    let path = vec!["value".into()];
    g.create_index::<ironweaver_core::GraphError>(&path).unwrap();
    assert_eq!(g.find_nodes(&path, &Value::Int(1)).unwrap().unwrap(), want);
    let expr = Expr::Or(vec![
        Expr::Label("Item".into()),
        Expr::Compare { path, op: ironweaver_core::CmpOp::Eq, value: Value::Int(1) },
    ]);
    assert_eq!(g.index_candidates(&expr).unwrap().unwrap(), want);
}
