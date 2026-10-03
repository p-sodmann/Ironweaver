// Budgets on traversals, path expansion and random walks: limited runs
// return a prefix of the unlimited result, flag truncation, or fail.

use std::cell::Cell;
use std::convert::Infallible;
use std::rc::Rc;

use ironweaver_core::cancel::{self, Token};
use ironweaver_core::query::{expand_paths, expand_paths_limited, Hops, Uniqueness};
use ironweaver_core::random_walks::{plan, WalkOptions};
use ironweaver_core::traversal::{bfs, bfs_limited, bidirectional_bfs, dfs, dfs_limited, expand, expand_limited};
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
    assert_eq!(err, GraphError::BudgetExceeded { visited: 5, edges: 6, results: 5 });
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
    assert_eq!(limited.unwrap().unwrap_err(), GraphError::BudgetExceeded { visited: 30, edges: 56, results: 29 });

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

/// A hub with `n` parallel edges to one leaf, and the leaf.
fn parallel_hub(n: usize) -> (G, NodeIx, NodeIx) {
    let mut g = G::new();
    let hub = g.add_node("hub", Record::default()).unwrap();
    let leaf = g.add_node("leaf", Record::default()).unwrap();
    for _ in 0..n {
        g.add_edge(hub, leaf, Record::default()).unwrap();
    }
    (g, hub, leaf)
}

#[test]
fn edges_are_bounded() {
    let (g, hub, _) = parallel_hub(100_000);
    let hops = Hops { min: 1, max: Some(1) };
    for k in [0, 1, 10, 1000] {
        let budget = Budget::default().max_visited(1).max_edges(k).truncate();
        let calls = Cell::new(0);
        let counting = |_: EdgeIx, _: &Edge<Record>| {
            calls.set(calls.get() + 1);
            Ok::<_, GraphError>(true)
        };

        let b = bfs_limited(&g, hub, None, budget, counting).unwrap();
        assert_eq!((calls.replace(0), b.edges, b.truncated, b.value.len()), (k, k, true, 1 + usize::from(k > 0)));
        let d = dfs_limited(&g, hub, None, Budget::default().max_edges(k).truncate(), counting).unwrap();
        assert_eq!((calls.replace(0), d.edges, d.truncated), (k, k, true));
        let x = expand_limited(&g, [hub], 1, Direction::Out, budget).unwrap();
        assert_eq!((x.edges, x.truncated, x.value.len()), (k, true, 1 + usize::from(k > 0)));
        let budget = Budget::default().max_edges(k).truncate();
        let p = expand_paths_limited(&g, hub, Direction::Out, hops, Uniqueness::Trail, budget, counting, |_, _| {
            Ok::<_, GraphError>(true)
        })
        .unwrap();
        assert_eq!((calls.replace(0), p.edges, p.truncated), (k, k, true));

        // Error mode reports the edges examined
        let err = bfs_limited(&g, hub, None, Budget::default().max_edges(k), counting).unwrap_err();
        assert_eq!(err, GraphError::BudgetExceeded { visited: 1, edges: k, results: 1 + usize::from(k > 0) });
        assert!(err.to_string().contains(&format!("examining {k} edges")), "{err}");
        let err = expand_limited(&g, [hub], 1, Direction::Both, Budget::default().max_edges(k)).unwrap_err();
        assert!(matches!(err, GraphError::BudgetExceeded { edges, .. } if edges == k));
    }

    // Exactly enough edges: not truncated
    let (g, hub, leaf) = parallel_hub(100);
    let b = bfs_limited(&g, hub, None, Budget::default().max_edges(100).truncate(), all).unwrap();
    assert_eq!((b.value, b.truncated, b.edges), (vec![hub, leaf], false, 100));
    let x = expand_limited(&g, [hub], 3, Direction::Both, Budget::default().max_edges(200).truncate()).unwrap();
    assert_eq!((x.truncated, x.edges), (false, 200)); // each edge from both ends
    let x = expand_limited(&g, [hub], 3, Direction::Both, Budget::default().max_edges(199).truncate()).unwrap();
    assert_eq!((x.truncated, x.edges), (true, 199));
}

#[test]
fn walks_count_steps_as_edges() {
    let (g, _) = star(30);
    let mut opts = WalkOptions::new(4, 100);
    opts.seed = Some(3);
    opts.allow_revisit = true;
    let walks = plan::<_, _, GraphError>(&g, Some("hub"), opts).unwrap();
    let full = walks.run_limited(Budget::UNLIMITED).unwrap();
    assert_eq!(full.edges, full.visited - 100);
    // 3 steps per walk: 30 edges fit 10 attempts
    let limited = walks.run_limited(Budget::default().max_edges(30).truncate()).unwrap();
    assert!(limited.truncated && limited.edges <= 30 && limited.visited <= 40);
    assert!(matches!(walks.run_limited(Budget::default().max_edges(30)), Err(GraphError::BudgetExceeded { .. })));
    // One-node walks take no steps
    let single = plan::<_, _, GraphError>(&g, Some("hub"), WalkOptions::new(1, 10)).unwrap();
    let limited = single.run_limited(Budget::default().max_edges(0)).unwrap();
    assert_eq!((limited.edges, limited.truncated), (0, false));
}

#[test]
fn cancel_stops_inside_a_nodes_edges() {
    let (g, hub, leaf) = parallel_hub(100_000);
    // A filter that cancels on its first call: no further edge is examined
    // Runs `search` under a token with that filter; the filter's calls
    type Filter<'a> = &'a dyn Fn(EdgeIx, &Edge<Record>) -> Result<bool, GraphError>;
    let calls_until_stop = |search: &dyn Fn(Filter) -> Result<(), GraphError>| {
        let token = Token::new();
        let calls = Cell::new(0);
        let filter = |_: EdgeIx, _: &Edge<Record>| {
            calls.set(calls.get() + 1);
            token.cancel();
            Ok(true)
        };
        assert_eq!(cancel::run(&token, || search(&filter)), Err(GraphError::Interrupted));
        calls.get()
    };
    let hops = Hops { min: 1, max: Some(1) };
    assert_eq!(calls_until_stop(&|f| bfs(&g, hub, None, f).map(drop)), 1);
    assert_eq!(calls_until_stop(&|f| dfs(&g, hub, None, f).map(drop)), 1);
    assert_eq!(calls_until_stop(&|f| bidirectional_bfs(&g, hub, leaf, None, Direction::Out, f).map(drop)), 1);
    assert_eq!(
        calls_until_stop(&|f| expand_paths(&g, hub, Direction::Out, hops, Uniqueness::Trail, f, |_, _| Ok(true))),
        1
    );

    // expand has no filter: a poll hook that cancels stops it within one
    // poll interval of the hub's edges
    let hooks = Rc::new(Cell::new(0));
    let h = hooks.clone();
    let hook = Rc::new(move || {
        h.set(h.get() + 1);
        true
    });
    let out = cancel::run_polling(&Token::new(), hook, || expand(&g, [hub], 1, Direction::Out));
    assert_eq!(out, Err(GraphError::Interrupted));
    assert_eq!(hooks.get(), 1);
}

#[test]
fn path_searches_count_against_the_budget() {
    use ironweaver_core::pathfinding::{find_path, find_path_limited, Heuristic, PathQuery};
    use ironweaver_core::traversal::bidirectional_bfs_limited;
    // hub -> c_i -> g_i (with cross edges); find a path to the last grandchild
    let (g, hub) = star(200);
    let end = g.node_ix("g199").unwrap();
    let queries: [fn() -> PathQuery<'static, Record, GraphError>; 3] =
        [PathQuery::bfs, PathQuery::dijkstra, || PathQuery::astar(Heuristic::Zero)];
    for query in queries {
        let full = find_path::<_, _, GraphError>(&g, hub, end, &mut query()).unwrap().unwrap();
        let limited = find_path_limited::<_, _, GraphError>(&g, hub, end, &mut query(), Budget::UNLIMITED).unwrap();
        assert_eq!(limited.value.as_ref(), Some(&full));
        assert!(!limited.truncated && limited.visited > 0 && limited.edges > 0);
        let (visited, edges) = (limited.visited, limited.edges);
        // Exactly enough is enough; one less stops it
        let exact = Budget::default().max_visited(visited).max_edges(edges).max_results(1);
        let r = find_path_limited::<_, _, GraphError>(&g, hub, end, &mut query(), exact).unwrap();
        assert_eq!(r.value.as_ref(), Some(&full));
        for budget in [
            Budget::default().max_visited(visited - 1),
            Budget::default().max_edges(edges - 1),
            Budget::default().max_results(0),
        ] {
            let r = find_path_limited::<_, _, GraphError>(&g, hub, end, &mut query(), budget.truncate()).unwrap();
            assert!(r.value.is_none() && r.truncated, "{budget:?}");
            assert!(r.visited <= visited && r.edges <= edges);
            let err = find_path_limited::<_, _, GraphError>(&g, hub, end, &mut query(), budget).unwrap_err();
            assert!(matches!(err, GraphError::BudgetExceeded { .. }), "{err:?}");
        }
        // Unreachable within the budget's room: not truncated
        let back =
            find_path_limited::<_, _, GraphError>(&g, end, hub, &mut query(), Budget::default().max_edges(10_000));
        assert_eq!(back.unwrap().value, None);
    }

    // Bidirectional BFS: both frontiers count, edges before the filter
    let (g, hub, leaf) = parallel_hub(1000);
    let r = bidirectional_bfs_limited(&g, hub, leaf, None, Direction::Out, Budget::UNLIMITED, all).unwrap();
    assert_eq!((r.value, r.visited, r.truncated), (Some(vec![hub, leaf]), 1, false));
    let budget = Budget::default().max_edges(0).truncate();
    let r = bidirectional_bfs_limited(&g, leaf, hub, None, Direction::In, budget, all).unwrap();
    assert_eq!((r.value, r.edges, r.truncated), (None, 0, true));
    let r = bidirectional_bfs_limited(&g, hub, hub, None, Direction::Out, Budget::default().max_results(0), all);
    assert!(r.is_err());
}

#[test]
fn path_search_polls_cancellation_per_edge() {
    use ironweaver_core::pathfinding::{find_path, Heuristic, PathQuery};
    // A* from a hub with 2000 leaves: the heuristic cancels on its second
    // call, and no further neighbour is estimated
    let (g, hub, _) = parallel_hub(0);
    let mut g = g;
    for i in 0..2000 {
        let leaf = g.add_node(format!("x{i}"), Record::default()).unwrap();
        g.add_edge(hub, leaf, Record::default()).unwrap();
    }
    let end = g.add_node("end", Record::default()).unwrap();
    let token = Token::new();
    let calls = Cell::new(0);
    let estimate = Box::new(|_: &ironweaver_core::Node<Record>| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            token.cancel();
        }
        Ok(0.0)
    });
    let mut q = PathQuery::astar(Heuristic::Custom(estimate));
    let r = cancel::run(&token, || find_path::<_, _, GraphError>(&g, hub, end, &mut q));
    assert_eq!(r, Err(GraphError::Interrupted));
    assert_eq!(calls.get(), 2);
}

#[test]
fn pattern_matching_counts_against_the_budget() {
    use ironweaver_core::query::{find_matches, find_matches_limited, for_each_match_limited, Pattern};
    let (g, _) = star(30);
    for text in ["(a)-->(b)", "(h)-->(c)-->(d)", "(a)-[*1..3]->(b)", "(a)-[*0..2]-(b)", "(a), (b)"] {
        let p = Pattern::parse(text).unwrap();
        let full = find_matches::<_, _, GraphError>(&g, &p, None).unwrap();
        let limited = find_matches_limited::<_, _, GraphError>(&g, &p, Budget::UNLIMITED).unwrap();
        assert_eq!(limited.value, full, "{text}");
        let (visited, edges) = (limited.visited, limited.edges);
        assert!(visited > 0, "{text}");
        // A prefix of the matches, whichever limit stops it
        for budget in [
            Budget::default().max_results(full.len() / 2),
            Budget::default().max_visited(visited / 2),
            Budget::default().max_edges(edges / 2),
        ] {
            let r = find_matches_limited::<_, _, GraphError>(&g, &p, budget.truncate()).unwrap();
            assert!(r.truncated || edges == 0, "{text} {budget:?}");
            assert_eq!(r.value, full[..r.value.len()], "{text} {budget:?}");
            assert!(r.visited <= visited && r.edges <= edges);
        }
        let exact = Budget::default().max_visited(visited).max_edges(edges).max_results(full.len());
        let r = find_matches_limited::<_, _, GraphError>(&g, &p, exact).unwrap();
        assert_eq!((r.value.len(), r.truncated), (full.len(), false), "{text}");
        // The visitor stopping is not a truncation
        let mut seen = 0;
        let r = for_each_match_limited::<_, _, GraphError>(&g, &p, Budget::default().max_results(1), |_| {
            seen += 1;
            Ok(false)
        })
        .unwrap();
        assert_eq!((seen, r.truncated), (1, false));
    }

    // A variable-length edge through a hub is bounded by edges, not paths
    let (g, _, _) = parallel_hub(100_000);
    let p = Pattern::parse("(a)-[*1..5]-(b)").unwrap();
    let budget = Budget::default().max_edges(1000).truncate();
    let r = find_matches_limited::<_, _, GraphError>(&g, &p, budget).unwrap();
    assert!(r.truncated && r.edges == 1000 && r.value.len() <= 1000);
    let err = find_matches_limited::<_, _, GraphError>(&g, &p, Budget::default().max_visited(50)).unwrap_err();
    assert!(matches!(err, GraphError::BudgetExceeded { visited: 50, .. }), "{err:?}");
}
