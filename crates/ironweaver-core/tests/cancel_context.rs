use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use ironweaver_core::cancel::{self, Progress, Token};
use ironweaver_core::GraphError;

fn counting_hook(calls: &Rc<Cell<usize>>) -> Rc<dyn Fn() -> bool> {
    let calls = calls.clone();
    Rc::new(move || {
        calls.set(calls.get() + 1);
        false
    })
}

fn poll_once() {
    // Exactly one hook call, whatever the thread's previous poll count.
    let stop = cancel::stop();
    for _ in 0..256 {
        assert!(!stop.poll());
    }
}

fn run_context<T>(
    token: &Token,
    progress: &Progress,
    hook: Rc<dyn Fn() -> bool>,
    f: impl FnOnce() -> T,
) -> Result<T, GraphError> {
    cancel::run_with_progress(token, progress, || cancel::run_polling(token, hook, f))?
}

#[test]
fn nested_contexts_restore_after_success_errors_and_cancellation() {
    for mode in 0..4 {
        let outer_token = Token::new();
        let outer_progress = Progress::new();
        let outer_calls = Rc::new(Cell::new(0));
        let inner_token = Token::new();
        let inner_progress = Progress::new();
        let inner_calls = Rc::new(Cell::new(0));
        if mode == 3 {
            inner_token.cancel();
        }

        run_context(&outer_token, &outer_progress, counting_hook(&outer_calls), || {
            cancel::progress().start("outer", Some(10));
            let out = run_context(&inner_token, &inner_progress, counting_hook(&inner_calls), || {
                assert_ne!(mode, 3, "a pre-cancelled token must not run the closure");
                cancel::progress().start("inner", Some(5));
                cancel::progress().add(2);
                poll_once();
                if mode == 2 {
                    inner_token.cancel();
                }
                if mode == 1 {
                    Err("closure error")
                } else {
                    Ok(7)
                }
            });
            match mode {
                0 => assert_eq!(out, Ok(Ok(7))),
                1 => assert_eq!(out, Ok(Err("closure error"))),
                _ => assert_eq!(out, Err(GraphError::Interrupted)),
            }
            if mode == 3 {
                let out = cancel::run_polling(&inner_token, counting_hook(&inner_calls), || unreachable!());
                assert_eq!(out, Err::<(), _>(GraphError::Interrupted));
            }
            // Cancelling the inner token must not affect the restored outer token.
            assert!(!cancel::stop().requested());
            poll_once();
            cancel::progress().add(1);
            assert_eq!(outer_progress.snapshot().done, 1);
            assert_eq!(inner_progress.snapshot().done, if mode == 3 { 0 } else { 2 });
        })
        .unwrap();
        assert_eq!(outer_calls.get(), 1);
        assert_eq!(inner_calls.get(), if mode == 3 { 0 } else { 1 });
        assert!(!cancel::progress().is_active());
        // No hook leaks into a later token-only computation.
        cancel::run(&Token::new(), poll_once).unwrap();
        assert_eq!(outer_calls.get(), 1);
        outer_token.cancel();
        assert!(!cancel::stop().requested());
    }
}

#[test]
fn nested_contexts_restore_after_unwinding() {
    let outer_token = Token::new();
    let outer_progress = Progress::new();
    let outer_calls = Rc::new(Cell::new(0));
    let inner_token = Token::new();
    let inner_progress = Progress::new();
    let inner_calls = Rc::new(Cell::new(0));

    run_context(&outer_token, &outer_progress, counting_hook(&outer_calls), || {
        cancel::progress().start("outer", None);
        let panic = catch_unwind(AssertUnwindSafe(|| {
            run_context(&inner_token, &inner_progress, counting_hook(&inner_calls), || {
                cancel::progress().start("inner", None);
                poll_once();
                inner_token.cancel();
                panic!("unwind through all three context guards");
            })
        }));
        assert!(panic.is_err());
        assert!(!cancel::stop().requested());
        poll_once();
        cancel::progress().add(3);
        assert_eq!(outer_progress.snapshot().done, 3);
        assert_eq!(inner_progress.snapshot().done, 0);
    })
    .unwrap();
    assert_eq!(outer_calls.get(), 1);
    assert_eq!(inner_calls.get(), 1);
    assert!(!cancel::progress().is_active());
    cancel::run(&Token::new(), poll_once).unwrap();
    assert_eq!(outer_calls.get(), 1);
    outer_token.cancel();
    assert!(!cancel::stop().requested());
}

#[test]
fn contexts_are_local_but_captured_reports_and_stops_cross_threads() {
    let token = Token::new();
    let watched = Progress::new();
    let calls = Rc::new(Cell::new(0));
    let out = run_context(&token, &watched, counting_hook(&calls), || {
        cancel::progress().start("shared", Some(2));
        let stop = cancel::stop();
        let report = cancel::progress();
        let canceller = token.clone();
        std::thread::spawn(move || {
            assert!(!cancel::progress().is_active());
            assert!(!cancel::stop().requested());
            // This thread's own token, hook and progress remain independent.
            let local_calls = Rc::new(Cell::new(0));
            let local_progress = Progress::new();
            run_context(&Token::new(), &local_progress, counting_hook(&local_calls), || {
                poll_once();
                cancel::progress().start("local", Some(1));
                cancel::progress().add(1);
                report.add(2);
                canceller.cancel();
                assert!(stop.requested());
                assert!(!cancel::stop().requested());
            })
            .unwrap();
            assert_eq!(local_calls.get(), 1);
            assert_eq!(local_progress.snapshot().done, 1);
            assert!(!cancel::progress().is_active());
            assert!(!cancel::stop().requested());
        })
        .join()
        .unwrap();
        assert!(cancel::stop().requested());
    });
    assert_eq!(out, Err(GraphError::Interrupted));
    assert_eq!(calls.get(), 0);
    assert_eq!(watched.snapshot().done, 2);
    assert!(!cancel::progress().is_active());
    assert!(!cancel::stop().requested());
}
