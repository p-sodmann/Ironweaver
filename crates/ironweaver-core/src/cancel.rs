// cancel.rs
//
// Cooperative cancellation for long computations.
//
// A caller runs a computation under a `Token` with `run`; another thread (a
// signal watcher, a timer, a request handler) calls `Token::cancel`, and the
// computation stops at its next check and `run` returns
// `GraphError::Interrupted`. Algorithms don't take the token as a parameter:
// at entry they fetch the current one with `stop()` (a thread-local set by
// `run`) and check the returned `Stop` in their loops, including inside
// rayon workers (the `Stop` is captured by the closures).
//
// Two kinds of checks:
// - `Stop::requested`: an atomic load, cheap enough for inner loops of
//   parallel code;
// - `Stop::poll`: also runs the thread's poll hook (`run_polling`), for
//   sequential code on the calling thread. The Python bindings use the hook
//   to run Python's signal handlers while they hold the GIL, so Ctrl+C
//   stops pattern matching, path search and traversals too. Call it every
//   few hundred iterations (it counts calls and polls every 256th).
//
// An algorithm that sees a stop request returns early with whatever it has;
// `run` then discards the result and returns `Interrupted`, so partial
// results never escape. Code that is not run under a token pays one
// thread-local read at entry and a branch per check.
//
// Progress travels the same way, the other direction: a caller that wants
// to watch a computation creates a `Progress`, runs it with
// `run_with_progress`, and reads `Progress::snapshot` from another thread.
// Algorithms fetch `progress()` at entry (a `Report`, no-op when nobody
// watches) and report a phase with its total (`Report::start`) and the work
// done (`Report::add`, or `Report::tick` for an indexed parallel loop), at
// the points where they check the stop flag.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::GraphError;

/// A cancellation flag shared between a computation and whoever may cancel it.
#[derive(Clone, Debug, Default)]
pub struct Token(Arc<AtomicBool>);

impl Token {
    pub fn new() -> Self {
        Token::default()
    }

    /// Ask the computation running under this token to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Token>> = const { RefCell::new(None) };
    // The calling thread's poll hook, and a call counter for `Stop::poll`
    static POLL: RefCell<Option<Rc<dyn Fn() -> bool>>> = const { RefCell::new(None) };
    static POLLS: Cell<u32> = const { Cell::new(0) };
    static PROGRESS: RefCell<Option<Progress>> = const { RefCell::new(None) };
}

/// What loops check: the current token's flag (or nothing).
#[derive(Clone, Debug, Default)]
pub struct Stop(Option<Arc<AtomicBool>>);

impl Stop {
    /// Whether the computation should stop. An atomic load: fine in inner
    /// loops and on any thread.
    #[inline]
    pub fn requested(&self) -> bool {
        self.0.as_ref().is_some_and(|f| f.load(Ordering::Relaxed))
    }

    /// Like [`requested`](Self::requested), and on the thread that started
    /// the computation, every 256th call also runs the poll hook of
    /// [`run_polling`] (which may cancel). For sequential loops.
    #[inline]
    pub fn poll(&self) -> bool {
        let Some(flag) = &self.0 else { return false };
        let n = POLLS.with(|c| {
            let n = c.get().wrapping_add(1);
            c.set(n);
            n
        });
        if n.is_multiple_of(256) {
            let hook = POLL.with(|p| p.borrow().clone());
            if hook.is_some_and(|hook| hook()) {
                flag.store(true, Ordering::Relaxed);
            }
        }
        flag.load(Ordering::Relaxed)
    }
}

/// The stop flag of the computation running on this thread (none if it
/// isn't running under [`run`]). Fetch it at the start of an algorithm and
/// move it into parallel closures.
pub fn stop() -> Stop {
    CURRENT.with(|c| Stop(c.borrow().as_ref().map(|t| t.0.clone())))
}

/// Progress of a computation, shared between it and whoever watches it:
/// the current phase, the work done in it and the phase's total. Create one,
/// run the computation with [`run_with_progress`] and read
/// [`snapshot`](Self::snapshot) from any thread while it runs. Cheap to clone
/// (the clones share the counters).
#[derive(Clone, Debug, Default)]
pub struct Progress(Arc<Shared>);

#[derive(Debug)]
struct Shared {
    phase: Mutex<&'static str>,
    done: AtomicU64,
    total: AtomicU64,
}

impl Default for Shared {
    fn default() -> Self {
        Shared { phase: Mutex::new(""), done: AtomicU64::new(0), total: AtomicU64::new(UNKNOWN) }
    }
}

// `total` when the phase doesn't know its size
const UNKNOWN: u64 = u64::MAX;

/// What [`Progress::snapshot`] reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressSnapshot {
    /// The phase the computation is in (`""` before it reports one), e.g.
    /// `"pagerank"`, `"leiden"`.
    pub phase: &'static str,
    /// Units of work done in the phase (iterations, rounds, nodes, sources;
    /// the algorithm's documentation says which).
    pub done: u64,
    /// The phase's units, if known. An algorithm that stops early (PageRank
    /// converging, label propagation with no change) ends below its total.
    pub total: Option<u64>,
}

impl Progress {
    pub fn new() -> Self {
        Progress::default()
    }

    /// The current phase and its counters. Read without stopping the
    /// computation; the three fields are read one after the other, so a
    /// snapshot taken just as a phase starts may mix the two phases.
    pub fn snapshot(&self) -> ProgressSnapshot {
        let phase = *self.0.phase.lock().unwrap_or_else(|e| e.into_inner());
        let total = self.0.total.load(Ordering::Relaxed);
        ProgressSnapshot {
            phase,
            done: self.0.done.load(Ordering::Relaxed),
            total: (total != UNKNOWN).then_some(total),
        }
    }
}

/// What algorithms report progress to: the [`Progress`] of the computation
/// running on this thread, or nothing (then every call is a branch). Fetch
/// it with [`progress`] at the start of an algorithm and move it into
/// parallel closures, like [`Stop`].
#[derive(Clone, Debug, Default)]
pub struct Report(Option<Arc<Shared>>);

/// Units [`Report::tick`] counts at once.
pub const TICK: usize = 1024;

impl Report {
    /// Whether anybody watches.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    /// Start a phase of `total` units (`None`: unknown) with nothing done.
    pub fn start(&self, phase: &'static str, total: Option<u64>) {
        if let Some(s) = &self.0 {
            *s.phase.lock().unwrap_or_else(|e| e.into_inner()) = phase;
            s.done.store(0, Ordering::Relaxed);
            s.total.store(total.unwrap_or(UNKNOWN), Ordering::Relaxed);
        }
    }

    /// Add `n` units done. An atomic add: fine on any thread, but for work
    /// items of a few microseconds use [`tick`](Self::tick).
    #[inline]
    pub fn add(&self, n: u64) {
        if let Some(s) = &self.0 {
            s.done.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// For a loop over the items `0..n`, in any order and on any threads:
    /// call it for every item `i`; it adds the units of [`TICK`] items at
    /// once (at the first of them), so all `n` are counted when every item
    /// has been, with an atomic add per [`TICK`] items.
    #[inline]
    pub fn tick(&self, i: usize, n: usize) {
        if self.0.is_some() && i.is_multiple_of(TICK) {
            self.add((n - i).min(TICK) as u64);
        }
    }
}

/// Where the computation running on this thread reports progress (nowhere
/// if it isn't running under [`run_with_progress`]).
pub fn progress() -> Report {
    PROGRESS.with(|p| Report(p.borrow().as_ref().map(|p| p.0.clone())))
}

/// Run `f` under `token`: `Err(GraphError::Interrupted)` if the token was
/// cancelled (before or while `f` ran; whatever `f` returned is dropped),
/// else `f`'s result.
pub fn run<T>(token: &Token, f: impl FnOnce() -> T) -> Result<T, GraphError> {
    struct Restore(Option<Token>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
    }
    if token.is_cancelled() {
        return Err(GraphError::Interrupted);
    }
    let _restore = Restore(CURRENT.with(|c| c.borrow_mut().replace(token.clone())));
    let out = f();
    if token.is_cancelled() {
        Err(GraphError::Interrupted)
    } else {
        Ok(out)
    }
}

/// [`run`] with a poll hook on this thread: sequential code calling
/// [`Stop::poll`] runs `hook` now and then, and stops if it returns true.
pub fn run_polling<T>(token: &Token, hook: Rc<dyn Fn() -> bool>, f: impl FnOnce() -> T) -> Result<T, GraphError> {
    struct Restore(Option<Rc<dyn Fn() -> bool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            POLL.with(|p| *p.borrow_mut() = previous);
        }
    }
    let _restore = Restore(POLL.with(|p| p.borrow_mut().replace(hook)));
    run(token, f)
}

/// [`run`] reporting progress to `progress`: algorithms that report it
/// (see [`progress`]) update it while `f` runs.
pub fn run_with_progress<T>(token: &Token, progress: &Progress, f: impl FnOnce() -> T) -> Result<T, GraphError> {
    struct Restore(Option<Progress>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            PROGRESS.with(|p| *p.borrow_mut() = previous);
        }
    }
    let _restore = Restore(PROGRESS.with(|p| p.borrow_mut().replace(progress.clone())));
    run(token, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_and_cancel() {
        let token = Token::new();
        assert_eq!(run(&token, || 5), Ok(5));
        assert!(!stop().requested()); // restored after run
                                      // Cancelled from another thread while running
        let t = token.clone();
        let out = run(&token, || {
            let s = stop();
            std::thread::spawn(move || t.cancel()).join().unwrap();
            let mut spins = 0;
            while !s.requested() {
                spins += 1;
            }
            spins
        });
        assert_eq!(out, Err(GraphError::Interrupted));
        // Already cancelled: f doesn't run
        assert_eq!(run(&token, || unreachable!()), Err::<(), _>(GraphError::Interrupted));
    }

    #[test]
    fn poll_hook_runs_on_the_calling_thread() {
        let token = Token::new();
        let calls = Rc::new(Cell::new(0));
        let c = calls.clone();
        let hook = Rc::new(move || {
            c.set(c.get() + 1);
            c.get() >= 3
        });
        let out = run_polling(&token, hook, || {
            let s = stop();
            let mut i = 0u32;
            while !s.poll() {
                i += 1;
            }
            i
        });
        assert_eq!(out, Err(GraphError::Interrupted));
        assert_eq!(calls.get(), 3);
        // Hook removed afterwards; no token, no stop
        assert!(!stop().poll());
        let fresh = Token::new();
        assert_eq!(run(&fresh, || (0..10_000).all(|_| !stop().poll())), Ok(true));
    }

    #[test]
    fn progress_is_reported_and_read_from_another_thread() {
        // Nobody watching: reports are no-ops
        let r = progress();
        assert!(!r.is_active());
        r.start("x", Some(1));
        r.add(1);

        let token = Token::new();
        let watched = Progress::new();
        assert_eq!(watched.snapshot(), ProgressSnapshot { phase: "", done: 0, total: None });
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let w = watched.clone();
        let watcher = std::thread::spawn(move || {
            go_rx.recv().unwrap();
            seen_tx.send(w.snapshot()).unwrap();
        });
        let out = run_with_progress(&token, &watched, || {
            let r = progress();
            assert!(r.is_active());
            r.start("count", Some(10));
            r.add(4);
            go_tx.send(()).unwrap();
            seen_rx.recv().unwrap()
        });
        watcher.join().unwrap();
        assert_eq!(out, Ok(ProgressSnapshot { phase: "count", done: 4, total: Some(10) }));
        assert!(!progress().is_active()); // restored after the run

        // tick counts every item once, whatever the order and the threads
        let n = 3 * TICK + 7;
        run_with_progress(&token, &watched, || {
            use rayon::prelude::*;
            let r = progress();
            r.start("items", Some(n as u64));
            (0..n).into_par_iter().rev().for_each(|i| r.tick(i, n));
        })
        .unwrap();
        assert_eq!(watched.snapshot(), ProgressSnapshot { phase: "items", done: n as u64, total: Some(n as u64) });
    }
}
