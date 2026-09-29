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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
        if n % 256 == 0 {
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
}
