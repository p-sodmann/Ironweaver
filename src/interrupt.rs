// interrupt.rs
//
// Ctrl+C (and other signals) during long computations.
//
// Python runs signal handlers only in the main thread, between bytecodes,
// with the GIL held. So:
// - `released` runs a computation that doesn't need the GIL on a thread of
//   rayon's pool (no thread to start: a call on a tiny graph stays cheap)
//   under a cancellation token, while the calling thread waits with the GIL
//   released and, every 20 ms, briefly takes the GIL back to run the signal
//   handlers. If one raises (KeyboardInterrupt), the token is
//   cancelled, the worker stops at its next check (core `cancel`), and the
//   exception is raised.
// - `polling` runs code that holds the GIL (it reads attributes or calls
//   Python filters): the core's sequential loops call `Stop::poll`, whose
//   hook runs the signal handlers.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use ironweaver_core::cancel::{self, Token};
use ironweaver_core::{GraphError, Projection};
use pyo3::exceptions::{PyKeyboardInterrupt, PyRuntimeError};
use pyo3::prelude::*;

/// How often a waiting call checks for signals.
const TICK: Duration = Duration::from_millis(20);

fn interrupted() -> PyErr {
    PyKeyboardInterrupt::new_err("interrupted")
}

/// Below this size (nodes + edges) computations run inline: they finish
/// quickly, and handing them to another thread would cost more (tens of
/// microseconds) than many of them take.
const INLINE: usize = 20_000;

/// The size `released` compares with `INLINE`.
pub fn size(p: &Projection) -> usize {
    p.node_count() + p.edge_count()
}

/// Run `f` with the GIL released, stoppable by Ctrl+C unless `size` (nodes +
/// edges, see `size`) says it is quick.
pub fn released<T: Send>(py: Python<'_>, size: usize, f: impl FnOnce() -> T + Send) -> PyResult<T> {
    if size < INLINE {
        return Ok(py.detach(f));
    }
    let out: PyResult<Result<T, GraphError>> = py.detach(|| {
        let token = Token::new();
        let (tx, rx) = mpsc::channel();
        let worker_token = token.clone();
        rayon::in_place_scope(|s| {
            s.spawn(move |_| {
                // The receiver may be gone if the waiter returned early
                let _ = tx.send(cancel::run(&worker_token, f));
            });
            loop {
                match rx.recv_timeout(TICK) {
                    Ok(result) => return Ok(result),
                    Err(RecvTimeoutError::Timeout) => {
                        if let Err(e) = Python::attach(|py| py.check_signals()) {
                            token.cancel();
                            // Wait for the job to notice (the scope would
                            // anyway), then raise
                            let _ = rx.recv();
                            return Err(e);
                        }
                    }
                    // The job panicked; the scope re-raises the panic
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(PyRuntimeError::new_err("computation failed"));
                    }
                }
            }
        })
    });
    match out? {
        Ok(value) => Ok(value),
        Err(GraphError::Interrupted) => Err(interrupted()),
        Err(e) => Err(crate::errors::graph_error(e)),
    }
}

/// Run `f` (holding the GIL) so that the core's sequential loops run
/// Python's signal handlers now and then, and stop if one raises.
pub fn polling<T>(py: Python<'_>, f: impl FnOnce() -> T) -> PyResult<T> {
    let _ = py;
    let pending: Rc<RefCell<Option<PyErr>>> = Rc::default();
    let seen = pending.clone();
    // The GIL is held while `f` runs, so attaching is only a check
    let hook = Rc::new(move || match Python::attach(|py| py.check_signals()) {
        Ok(()) => false,
        Err(e) => {
            *seen.borrow_mut() = Some(e);
            true
        }
    });
    match cancel::run_polling(&Token::new(), hook, f) {
        Ok(value) => Ok(value),
        Err(_) => Err(pending.borrow_mut().take().unwrap_or_else(interrupted)),
    }
}
