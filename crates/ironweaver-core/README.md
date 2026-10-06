# ironweaver-core

The graph engine behind the [ironweaver](https://pypi.org/project/ironweaver/) Python package, in pure Rust (no Python dependency).

- **A directed property multigraph**, `Graph<N, E>`.
  - Nodes have unique string ids, sorted label sets (interned, with a label index) and a payload `N`.
  - Edges have persistent ids (never reused), an optional type and a payload `E`.
  - Handles (`NodeIx` / `EdgeIx`) never alias a removed node or edge.
- **Changes as data.** `Op` values apply to a graph and return the ops that undo them, and `Graph::apply_all` applies a batch all-or-nothing. That's the base for a write-ahead log, replication or rollback.
- **Filter expressions** (`Expr`): comparisons, membership and existence on attribute paths, labels and edge types, and `and` / `or` / `not`.
- **Analytics on a `Projection`**, a compact read-only CSR snapshot of (part of) a graph that is `Send + Sync` and can be shared across threads. Many of the algorithms run in parallel:
  - components, topological order and cycle detection;
  - PageRank, betweenness, closeness, harmonic and degree centrality;
  - triangles, clustering and k-core numbers;
  - label propagation, Leiden communities and modularity;
  - node similarity, spanning forests, k shortest paths, BFS levels and batch shortest paths;
  - FastRP embeddings and node2vec walks.
- **Queries:** Cypher-like pattern matching and variable-length path expansion. Patterns and filter expressions implement serde, and patterns print back as text that parses to the same pattern.
- **Budgets:** traversals, path expansion and random walks can run under a `Budget` (most nodes visited, most results), failing or returning the first results with a `truncated` flag.
- **Pathfinding:** BFS, Dijkstra and A* with pluggable heuristics.
- **A file format** (JSON, or binary with a header and a CRC32), with atomic, deterministic saves, and binary loading from any reader without holding the whole file in memory.

Payloads are read only through the `Attributes` trait, and user callbacks return `Result<_, X>` for your own error type `X`. `Record` (maps of `Value`s) is the ready-made payload.

## Example

```rust
use ironweaver_core::algo::{leiden, pagerank, Leiden, PageRank};
use ironweaver_core::pathfinding::{find_path, EdgeCost, PathQuery};
use ironweaver_core::query::{find_matches, Pattern};
use ironweaver_core::{Direction, Graph, GraphError, Op, Projection, Record, Value};

fn main() -> Result<(), GraphError> {
    let mut g: Graph<Record, Record> = Graph::new();
    for id in ["ann", "bob", "cat", "dan"] {
        let ix = g.add_node(id, Record::default())?;
        g.add_label(ix, "Person")?;
    }
    let node = |g: &Graph<Record, Record>, id: &str| g.node_ix(id).unwrap();
    for (a, b, w) in [("ann", "bob", 1.0), ("bob", "cat", 2.0), ("ann", "cat", 5.0), ("cat", "dan", 1.0)] {
        let (a, b) = (node(&g, a), node(&g, b));
        g.insert_edge(a, b, None, Some("knows"), Record::with_attr([("weight", Value::from(w))]))?;
    }

    // Cheapest path
    let path = find_path::<_, _, GraphError>(&g, node(&g, "ann"), node(&g, "dan"), &mut PathQuery::dijkstra())?.unwrap();
    assert_eq!(path.cost, 4.0);

    // Analytics on a projection: dense node indices in, Vecs out
    let p = Projection::build::<_, _, GraphError>(&g, Direction::Both, &EdgeCost::Unit)?;
    let ranks = pagerank(&p, &PageRank::default())?;
    assert!((ranks.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    let communities = leiden(&p, &Leiden::default())?;
    assert_eq!(communities.iter().map(Vec::len).sum::<usize>(), 4);

    // Pattern matching
    let pattern = Pattern::parse("(a:Person)-[:knows]->(b)-[:knows]->(c)")?;
    let found = find_matches::<_, _, GraphError>(&g, &pattern, None)?;
    assert_eq!(found.len(), 3); // ann-bob-cat, ann-cat-dan, bob-cat-dan

    // Changes as data, with undo
    let undo = g.apply(Op::RemoveNode { id: "dan".into() })?;
    assert!(!g.contains_node("dan"));
    g.apply_all(undo).map_err(|(_, e)| e)?;
    assert!(g.contains_node("dan"));
    Ok(())
}
```

## Cancellation

Long computations (the `algo` functions, batch queries, pattern matching,
path expansion, searches, random walks) check a cancellation token. Run one
under `cancel::run(&token, || ...)` and call `token.cancel()` from another
thread: it stops soon after and `run` returns `Err(GraphError::Interrupted)`.
Without a token the checks cost nothing measurable.

To watch how far an algorithm has got, run it with
`cancel::run_with_progress(&token, &progress, || ...)` and read
`progress.snapshot()` from another thread: its phase (`"pagerank"`,
`"leiden"`, ...), the units done (iterations, rounds, runs, nodes, sources)
and the phase's total.

```rust
use ironweaver_core::algo::{pagerank, PageRank};
use ironweaver_core::cancel::{run_with_progress, Progress, Token};
use ironweaver_core::pathfinding::EdgeCost;
use ironweaver_core::{Direction, Graph, GraphError, Projection, Record};

fn main() -> Result<(), GraphError> {
    let mut g: Graph<Record, Record> = Graph::new();
    let a = g.add_node("a", Record::default())?;
    let b = g.add_node("b", Record::default())?;
    g.add_edge(a, b, Record::default())?;
    let p = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::Unit)?;

    let progress = Progress::new(); // a clone goes to the thread that watches
    let opts = PageRank { tol: 0.0, max_iter: 50, ..Default::default() };
    run_with_progress(&Token::new(), &progress, || pagerank(&p, &opts))??;
    let s = progress.snapshot();
    assert_eq!((s.phase, s.done, s.total), ("pagerank", 50, Some(50)));
    Ok(())
}
```

## Budgets

Depth limits don't bound work: a depth-2 traversal from a node with a
million neighbours visits a million nodes. The `*_limited` traversals,
shortest paths (`pathfinding::find_path_limited`), path expansion and
pattern matching (`query::expand_paths_limited`,
`for_each_match_limited`) and random walks (`random_walks::plan_limited`,
`WalkPlan::run_limited`) take a `Budget`
(`max_visited` nodes entered, `max_edges` edges examined, `max_results`)
and stop at its limits, with `GraphError::BudgetExceeded` or, with
`truncate()`, the first results and `truncated` set. Only `max_edges`
bounds the work at a single node with many edges:

```rust
use ironweaver_core::traversal::bfs_limited;
use ironweaver_core::{Budget, Direction, Graph, GraphError, Record};

let mut g: Graph<Record, Record> = Graph::new();
let hub = g.add_node("hub", Record::default())?;
for i in 0..1000 {
    let leaf = g.add_node(format!("leaf{i}"), Record::default())?;
    g.add_edge(hub, leaf, Record::default())?;
}
let all = |_, _: &_| Ok::<_, GraphError>(true);
let first = bfs_limited(&g, hub, Some(2), Direction::Out, Budget::default().max_results(10).truncate(), all)?;
assert_eq!((first.value.len(), first.truncated), (10, true));
assert!(bfs_limited(&g, hub, Some(2), Direction::Out, Budget::default().max_results(10), all).is_err());
let some = bfs_limited(&g, hub, None, Direction::Out, Budget::default().max_edges(100).truncate(), all)?;
assert_eq!((some.value.len(), some.edges, some.truncated), (101, 100, true));
# Ok::<(), GraphError>(())
```

For wall-clock limits, run the search under a cancellation token.

## File formats

Graphs are saved in format version 2 (JSON, or a checksummed binary file).
Version 1 JSON files (ironweaver 0.1) still load. Version 1 binary files do
not: loading one is a `GraphError::Format` that says so. To convert one,
load it with ironweaver 0.1, save it with `save_to_json`, and load that JSON
file.

## Minimum supported Rust version

Rust 1.99. It follows recent stable releases; raising it is a minor-version change.

## License

MIT
