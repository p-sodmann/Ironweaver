// Cancelling long computations: each would run (practically) forever, and
// must stop soon after its token is cancelled from another thread, or after
// the poll hook asks it to.

use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ironweaver_core::algo::{self, Betweenness, PageRank};
use ironweaver_core::cancel::{self, Token};
use ironweaver_core::pathfinding::EdgeCost;
use ironweaver_core::query::{expand_paths, find_matches, Hops, Pattern, Uniqueness};
use ironweaver_core::{Direction, Graph, GraphError, NodeIx, Projection, Record};

type G = Graph<Record, Record>;

/// Every node linked to every other: path and pattern counts explode.
fn complete(n: usize) -> (G, Vec<NodeIx>) {
    let mut g = G::new();
    let ix: Vec<NodeIx> = (0..n).map(|i| g.add_node(format!("n{i}"), Record::default()).unwrap()).collect();
    for &a in &ix {
        for &b in &ix {
            if a != b {
                g.add_edge(a, b, Record::default()).unwrap();
            }
        }
    }
    (g, ix)
}

/// Run `f` under a token cancelled after `after`; the result, and how long
/// it took after the cancel.
fn cancelled_after<T: Send>(after: Duration, f: impl FnOnce() -> T + Send) -> (Result<T, GraphError>, Duration) {
    let token = Token::new();
    let (tx, rx) = mpsc::channel();
    let canceller = token.clone();
    thread::scope(|s| {
        s.spawn(move || {
            thread::sleep(after);
            canceller.cancel();
            tx.send(Instant::now()).unwrap();
        });
        let out = cancel::run(&token, f);
        let cancelled_at = rx.recv().unwrap();
        (out, cancelled_at.elapsed())
    })
}

#[test]
fn parallel_algorithms_stop() {
    let (g, _) = complete(300);
    let p = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::Unit).unwrap();
    // PageRank that never converges (tol 0, endless iterations)
    let endless = PageRank { tol: 0.0, max_iter: usize::MAX, ..Default::default() };
    let (out, lag) = cancelled_after(Duration::from_millis(30), || algo::pagerank(&p, &endless));
    assert_eq!(out.err(), Some(GraphError::Interrupted));
    assert!(lag < Duration::from_secs(2), "{lag:?}");
    // Exact betweenness on a bigger graph
    let (big, _) = complete(1200);
    let pb = Projection::build::<_, _, GraphError>(&big, Direction::Out, &EdgeCost::Unit).unwrap();
    let start = Instant::now();
    let (out, lag) =
        cancelled_after(Duration::from_millis(30), || algo::betweenness_centrality(&pb, &Betweenness::default()));
    assert_eq!(out.err(), Some(GraphError::Interrupted));
    assert!(lag < Duration::from_secs(2), "{lag:?} (ran {:?})", start.elapsed());
}

#[test]
fn sequential_search_stops() {
    let (g, ix) = complete(30);
    // Every walk of up to 30 edges: astronomically many
    let hops = Hops { min: 1, max: Some(30) };
    let (out, lag) = cancelled_after(Duration::from_millis(30), || {
        let mut n = 0u64;
        expand_paths::<_, _, GraphError>(
            &g,
            ix[0],
            Direction::Out,
            hops,
            Uniqueness::Walk,
            |_, _| Ok(true),
            |_, _| {
                n += 1;
                Ok(true)
            },
        )
        .map(|_| n)
    });
    assert_eq!(out.err(), Some(GraphError::Interrupted));
    assert!(lag < Duration::from_secs(2), "{lag:?}");
    // A pattern with ~30^8 matches
    let pattern = Pattern::parse("(a)-->(b)-->(c)-->(d)-->(e)-->(f)-->(h)-->(i)").unwrap();
    let (out, lag) = cancelled_after(Duration::from_millis(30), || {
        find_matches::<_, _, GraphError>(&g, &pattern, None).map(|m| m.len())
    });
    assert_eq!(out.err(), Some(GraphError::Interrupted));
    assert!(lag < Duration::from_secs(2), "{lag:?}");
}

#[test]
fn the_poll_hook_can_stop_sequential_code() {
    let (g, ix) = complete(20);
    let polls = Rc::new(std::cell::Cell::new(0));
    let counter = polls.clone();
    // Stop at the 5th poll, as the Python bindings do when a signal arrives
    let hook = Rc::new(move || {
        counter.set(counter.get() + 1);
        counter.get() >= 5
    });
    let out = cancel::run_polling(&Token::new(), hook, || {
        expand_paths::<_, _, GraphError>(
            &g,
            ix[0],
            Direction::Out,
            Hops { min: 1, max: Some(20) },
            Uniqueness::Walk,
            |_, _| Ok(true),
            |_, _| Ok(true),
        )
    });
    assert_eq!(out, Err(GraphError::Interrupted));
    assert_eq!(polls.get(), 5);
}

#[test]
fn uncancelled_runs_are_unaffected() {
    let (g, _) = complete(40);
    let p = Projection::build::<_, _, GraphError>(&g, Direction::Out, &EdgeCost::Unit).unwrap();
    let plain = algo::betweenness_centrality(&p, &Betweenness::default()).unwrap();
    let token = Token::new();
    let under = cancel::run(&token, || algo::betweenness_centrality(&p, &Betweenness::default())).unwrap().unwrap();
    assert_eq!(plain, under);
    assert_eq!(GraphError::Interrupted.to_string(), "interrupted");
}
