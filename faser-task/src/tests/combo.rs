//! Test various combinations of task behavior.
//!
//! These test cases were partially lifted from the Tokio ones, as our
//! goal is to be compatible API wise with Tokio.
//! <https://github.com/tokio-rs/tokio/blob/master/tokio/src/runtime/tests/task_combinations.rs>
//!
//! Many of the test variants do not apply to this crate, for example we only have a single
//! thread and reference counting and task cleanup is done a bit differently. We're similiar
//! enough to Tokio in our external APIs however that we can use their test suite as a smoke test
//! for our task implementation.
use std::future::Future;
use std::panic;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::channel::oneshot;
use futures::FutureExt;

use super::{yield_now, TestSpawner};

struct Output {
    on_drop: Option<oneshot::Sender<()>>,
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.on_drop.take().unwrap().send(());
    }
}

struct FutWrapper<F> {
    inner: F,
    on_drop: Option<oneshot::Sender<()>>,
}

impl<F> Future for FutWrapper<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let me = Pin::into_inner_unchecked(self);
            let inner = Pin::new_unchecked(&mut me.inner);
            inner.poll(cx)
        }
    }
}

impl<F> Drop for FutWrapper<F> {
    fn drop(&mut self) {
        let _ = self.on_drop.take().unwrap().send(());
    }
}

struct Signals {
    /// Used to signal that the task has been polled at least once.
    on_first_poll: Option<oneshot::Sender<()>>,
    /// Used to signal when the output of the future has been dropped.
    on_output_drop: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskPanicType {
    OnRun,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinInterest {
    Polled,
    NotPolled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinHandleDrop {
    Immediately,
    FirstPoll,
    AfterNoConsume,
    AfterConsume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbortStrategy {
    Immediate,
    FirstPoll,
    LastPoll,
    AfterJoin,
}

async fn test_task(mut signals: Signals, task_panic: TaskPanicType) -> Output {
    // Signal that the task was polled.
    let _ = signals.on_first_poll.take().unwrap().send(());
    // Yield the task once.
    yield_now().await;

    // Check if we're supposed to panic.
    if let TaskPanicType::OnRun = task_panic {
        panic!("panicking on running task")
    }

    Output {
        on_drop: signals.on_output_drop.take(),
    }
}

fn test_task_combo(
    task_panic: TaskPanicType,
    join_interest: JoinInterest,
    join_handle_op: JoinHandleDrop,
    abort_task: AbortStrategy,
) {
    println!("{task_panic:?} - {join_interest:?} - {join_handle_op:?} - {abort_task:?}");

    let (on_first_poll, mut wait_first_poll) = oneshot::channel();
    let (on_output_drop, mut wait_output_drop) = oneshot::channel();
    let (on_future_drop, mut wait_future_drop) = oneshot::channel();

    let signals = Signals {
        on_first_poll: Some(on_first_poll),
        on_output_drop: Some(on_output_drop),
    };

    let spawner = TestSpawner::new();

    let future = test_task(signals, task_panic);
    let future = FutWrapper {
        inner: future,
        on_drop: Some(on_future_drop),
    };
    let handle = spawner.spawn(future);

    spawner.next().unwrap().run();

    let mut handle = Some(handle);

    let mut aborted_early = false;

    if matches!(join_interest, JoinInterest::Polled) {
        assert!(
            handle.as_mut().unwrap().now_or_never().is_none(),
            "polling handle should have failed"
        );
    }

    if abort_task == AbortStrategy::Immediate {
        handle.as_mut().unwrap().abort();
        aborted_early = true;
    }

    if join_handle_op == JoinHandleDrop::Immediately {
        drop(handle.take().unwrap());
    }

    // Check that the task was polled.
    assert!(
        wait_first_poll.try_recv().is_ok(),
        "Task should have been polled"
    );

    // Abort the task now that we know the first poll happened.
    if abort_task == AbortStrategy::FirstPoll && join_handle_op != JoinHandleDrop::Immediately {
        handle.as_mut().unwrap().abort();
        aborted_early = true;
    }

    if join_handle_op == JoinHandleDrop::FirstPoll {
        drop(handle.take().unwrap());
    }

    // Run the task again, it should complete.
    spawner.next().unwrap().run();
    assert!(spawner.next().is_none());

    // The future should be cleaned up now.
    assert!(wait_future_drop.try_recv().is_ok());

    if abort_task == AbortStrategy::LastPoll {
        if let Some(handle) = handle.as_mut() {
            handle.abort();
        }
    }

    if join_handle_op == JoinHandleDrop::AfterNoConsume {
        let panic = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            drop(handle.take().unwrap());
        }));
        if panic.is_err() {
            assert!(!matches!(task_panic, TaskPanicType::OnRun) && !aborted_early);
        }
    }
    if join_handle_op == JoinHandleDrop::AfterConsume {
        let result = handle
            .as_mut()
            .unwrap()
            .now_or_never()
            .expect("expected completion");
        match result {
            Ok(_) => {
                assert!(!aborted_early, "task was aborted but still returned output");
            }
            Err(err) if err.is_cancelled() => {
                assert!(
                    aborted_early,
                    "the task result was cancelled but the task was not aborted"
                );
            }
            Err(err) if err.is_panic() => {
                let is_panic_expected = matches!(task_panic, TaskPanicType::OnRun);
                assert!(is_panic_expected, "panic not expected");
            }
            _ => unreachable!(),
        }
        let handle = handle.take().unwrap();
        if abort_task == AbortStrategy::AfterJoin {
            handle.abort();
        }
        drop(handle);
    }

    // Output should have been dropped now.
    let output_created = wait_output_drop.try_recv().is_ok();
    assert_eq!(
        output_created,
        (!matches!(task_panic, TaskPanicType::OnRun)) && !aborted_early
    );
}

#[test]
fn combinations() {
    for &task_panic in &[TaskPanicType::None, TaskPanicType::OnRun] {
        for &join_interest in &[JoinInterest::NotPolled, JoinInterest::Polled] {
            for &join_handle_op in &[
                JoinHandleDrop::AfterConsume,
                JoinHandleDrop::AfterNoConsume,
                JoinHandleDrop::FirstPoll,
                JoinHandleDrop::Immediately,
            ] {
                for &abort_task in &[
                    AbortStrategy::Immediate,
                    AbortStrategy::AfterJoin,
                    AbortStrategy::LastPoll,
                    AbortStrategy::FirstPoll,
                ] {
                    println!(
                        "{task_panic:?} - {join_interest:?} - {join_handle_op:?} - {abort_task:?}"
                    );

                    test_task_combo(task_panic, join_interest, join_handle_op, abort_task);
                }
            }
        }
    }
}
