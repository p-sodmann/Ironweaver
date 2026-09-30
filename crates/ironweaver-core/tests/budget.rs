// Budgets on traversals, path expansion and random walks: limited runs
// return a prefix of the unlimited result, flag truncation, or fail.

use std::convert::Infallible;

use ironweaver_core::query::{expand_paths, expand_paths_limited, Hops, Uniqueness};
use ironweaver_core::random_walks::{plan, WalkOptions};
use ironweaver_core::traversal::{bfs, bfs_limited, dfs, dfs_limited, expand, expand_limited};
use ironweaver_core::{Budget, Direction, Edge, EdgeIx, Graph, GraphError, NodeIx, Record};

type G = Graph<Record, Record>;

/// A hub with `fan` children, each with one child of its own, plus a few
/// cross edges.
fn star(fan: usize) -> (G, NodeIx) {
    let mut g = G::new();
    let hub = g.add_node("hub", Record::default()).unwrap();
    let mut prev = None;
    for i in 0..fan {
        let c = g.add_node(format!("c{i}"), Record::default()).unwrap();
        let gc = g.add_node(format!("g{i}"), Record::default()).unwrap();
        g.add_edge(hub, c, Record::default()).unwrap();
        g.add_edge(c, gc, Record::default()).unwrap();
        if let Some(p) = prev {
            g.add_edge(gc, p, Record::default()).unwrap();
        }
        prev = Some(c);
    }
    (g, hub)
}

fn all(_: EdgeIx, _: &Edge<Record>) -> Result<bool, GraphError> {
    Ok(true)
}

#[test]
fn traversals_return_a_prefix() {
    let (g, hub) = star(50);
    for depth in [None, Some(1), Some(2)] {
        let full_bfs = bfs(&g, hub, depth, all).unwrap();
        let full_dfs = dfs(&g, hub, depth, all).unwrap();
        let full_expand = expand(&g, [hub], depth.unwrap_or(10), Direction::Out);
        for k in [0, 1, 7, 51, full_bfs.len() - 1, full_bfs.len(), full_bfs.len() + 5] {
            let budget = Budget::default().max_results(k).truncate();
            let b = bfs_limited(&g, hub, depth, budget, all).unwrap();
            assert_eq!(b.value, full_bfs[..k.min(full_bfs.len())]);
            assert_eq!(b.truncated, k < full_bfs.len());
            let d = dfs_limited(&g, hub, depth, budget, all).unwrap();
            assert_eq!(d.value, full_dfs[..k.min(full_dfs.len())]);
            assert_eq!(d.truncated, k < full_dfs.len());
            let x = expand_limited(&g, [hub], depth.unwrap_or(10), Direction::Out, budget).unwrap();
            assert_eq!(x.value, full_expand[..k.min(full_expand.len())]);
            assert_eq!(x.truncated, k < full_expand.len());
        }
        let unlimited = bfs_limited(&g, hub, depth, Budget::UNLIMITED, all).unwrap();
        assert_eq!((unlimited.value, unlimited.truncated), (full_bfs, false));
    }
}

#[test]
fn visits_are_bounded() {
    let (g, hub) = star(1000);
    // Only the hub is expanded: it and its children come back
    let b = bfs_limited(&g, hub, None, Budget::default().max_visited(1).truncate(), all).unwrap();
    assert_eq!((b.value.len(), b.truncated, b.visited), (1001, true, 1));
    let b = bfs_limited(&g, hub, Some(1), Budget::default().max_visited(1).truncate(), all).unwrap();
    assert_eq!((b.value.len(), b.truncated), (1001, false));
    let d = dfs_limited(&g, hub, None, Budget::default().max_visited(10).truncate(), all).unwrap();
    assert!(d.truncated && d.visited == 10);
    let x = expand_limited(&g, [hub], 3, Direction::Both, Budget::default().max_visited(5).truncate()).unwrap();
    assert!(x.truncated && x.visited == 5);

    // Error mode reports where it stopped
    let err = bfs_limited(&g, hub, None, Budget::default().max_visited(3), all).unwrap_err();
    assert!(matches!(err, GraphError::BudgetExceeded { visited: 3, .. }), "{err:?}");
    let err = dfs_limited(&g, hub, None, Budget::default().max_results(5), all).unwrap_err();
    assert_eq!(err, GraphError::BudgetExceeded { visited: 5, results: 5 });
    assert!(err.to_string().contains("budget exceeded"));
    let err = expand_limited(&g, [hub], 2, Direction::Out, Budget::default().max_results(2)).unwrap_err();
    assert!(matches!(err, GraphError::BudgetExceeded { results: 2, .. }));
    // Filter errors still come first
    let failing = |_: EdgeIx, _: &Edge<Record>| Err::<bool, _>(GraphError::InvalidArgument("boom".into()));
    assert!(matches!(bfs_limited(&g, hub, None, Budget::default(), failing), Err(GraphError::InvalidArgument(_))));
}

#[test]
fn path_expansion_is_bounded() {
    let (g, hub) = star(20);
    let hops = Hops { min: 1, max: Some(4) };
    let collect = |budget: Option<Budget>| {
        let mut paths: Vec<Vec<NodeIx>> = Vec::new();
        let limited = {
            let visit = |_: &[EdgeIx], nodes: &[NodeIx]| {
                paths.push(nodes.to_vec());
                Ok::<_, GraphError>(true)
            };
            match budget {
                Some(b) => Some(expand_paths_limited(&g, hub, Direction::Both, hops, Uniqueness::Path, b, all, visit)),
                None => {
                    expand_paths(&g, hub, Direction::Both, hops, Uniqueness::Path, all, visit).unwrap();
                    None
                }
            }
        };
        (paths, limited)
    };
    let (full, _) = collect(None);
    assert!(full.len() > 100);
    for k in [0, 1, 10, full.len(), full.len() + 1] {
        let (paths, limited) = collect(Some(Budget::default().max_results(k).truncate()));
        assert_eq!(paths, full[..k.min(full.len())]);
        assert_eq!(limited.unwrap().unwrap().truncated, k < full.len());
    }
    let (paths, limited) = collect(Some(Budget::default().max_visited(30).truncate()));
    let limited = limited.unwrap().unwrap();
    assert!(limited.truncated && limited.visited == 30 && !paths.is_empty());
    assert_eq!(paths, full[..paths.len()]);
    let (_, limited) = collect(Some(Budget::default().max_visited(30)));
    assert_eq!(limited.unwrap().unwrap_err(), GraphError::BudgetExceeded { visited: 30, results: 29 });

    // A visitor stopping the search is not truncation
    let limited = expand_paths_limited(
        &g,
        hub,
        Direction::Out,
        hops,
        Uniqueness::Path,
        Budget::default().max_results(100),
        all,
        |_, _| Ok::<_, GraphError>(false),
    )
    .unwrap();
    assert!(!limited.truncated);
}

#[test]
fn random_walks_are_bounded() {
    let (g, _) = star(30);
    let mut opts = WalkOptions::new(4, 500);
    opts.seed = Some(3);
    opts.allow_revisit = true;
    let plan = plan::<_, _, GraphError>(&g, Some("hub"), opts).unwrap();
    let full = plan.run();
    let ids = |walks: &[ironweaver_core::random_walks::Walk]| -> Vec<Vec<String>> {
        walks.iter().map(|w| plan.items(w).map(str::to_owned).collect()).collect()
    };
    let limited = plan.run_limited(Budget::default().max_results(5).truncate()).unwrap();
    assert_eq!(ids(&limited.value), ids(&full[..5]));
    assert!(limited.truncated);
    let limited = plan.run_limited(Budget::UNLIMITED).unwrap();
    assert_eq!((ids(&limited.value), limited.truncated), (ids(&full), false));
    assert!(limited.visited >= full.len() && limited.visited <= 500 * 4);

    // 40 nodes of walking at 4 per walk: 10 attempts, a prefix of the full run
    let limited = plan.run_limited(Budget::default().max_visited(40).truncate()).unwrap();
    assert!(limited.truncated && limited.visited <= 40);
    let got = ids(&limited.value);
    let mut first = WalkOptions::new(4, 10);
    first.seed = Some(3);
    first.allow_revisit = true;
    let ten = plan_walks(&g, first);
    assert_eq!(got, ten);
    assert!(matches!(plan.run_limited(Budget::default().max_visited(40)), Err(GraphError::BudgetExceeded { .. })));
}

fn plan_walks(g: &G, opts: WalkOptions) -> Vec<Vec<String>> {
    ironweaver_core::random_walks::random_walks::<_, _, GraphError>(g, Some("hub"), opts).unwrap()
}

#[test]
fn unlimited_budget_changes_nothing() {
    let (g, hub) = star(10);
    let a = dfs(&g, hub, None, |_, _| Ok::<_, Infallible>(true)).unwrap();
    let b = dfs_limited(&g, hub, None, Budget::UNLIMITED, all).unwrap();
    assert_eq!(a, b.value);
    assert_eq!(b.visited, a.len());
}
