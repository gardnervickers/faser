//! Test cases where the future needs to be woken up.
use futures::FutureExt;

use super::{yield_now, TestFuture, TestState};

#[test]
fn drop_handle_wake_task() {
    let _e = TestState::enter();
    TestState::with(|v| {
        v.return_pending = true;
        v.store_waker = true;
    });

    let spawner = super::TestSpawner::new();
    spawner.spawn(TestFuture).detach();
    spawner.next().unwrap().run();
    assert!(spawner.next().is_none());
    TestState::with(|v| {
        assert!(!v.task_dropped);
        assert!(!v.output_dropped);
        assert_eq!(v.num_polls, 1);

        v.return_pending = false;
        v.store_waker = false;
        v.stashed_waker.take().unwrap().wake();
    });

    spawner.next().unwrap().run();

    TestState::with(|v| {
        assert!(v.task_dropped);
        assert!(v.output_dropped);
        assert_eq!(v.num_polls, 2);
    });
}

#[test]
fn wake_task_drop_handle() {
    let _e = TestState::enter();
    TestState::with(|v| {
        v.return_pending = true;
        v.store_waker = true;
    });

    let spawner = super::TestSpawner::new();
    let handle = spawner.spawn(TestFuture);
    spawner.next().unwrap().run();
    assert!(spawner.next().is_none());
    TestState::with(|v| {
        assert!(!v.task_dropped);
        assert!(!v.output_dropped);
        v.return_pending = false;
        v.store_waker = false;
        v.stashed_waker.take().unwrap().wake();
    });

    spawner.next().unwrap().run();

    TestState::with(|v| {
        assert!(v.task_dropped);
        assert!(!v.output_dropped);
        assert_eq!(v.num_polls, 2);
    });
    assert!(handle.now_or_never().unwrap().is_ok());
    TestState::with(|v| {
        assert!(v.output_dropped);
    });
}

#[test]
fn running_task_wakes_itself() {
    let spawner = super::TestSpawner::new();
    let handle = spawner.spawn(async {
        yield_now().await;
    });

    spawner.next().unwrap().run();
    spawner.next().unwrap().run();
    assert!(handle.now_or_never().unwrap().is_ok());
}

#[test]
fn abort_waiting_task() {
    let _e = TestState::enter();
    TestState::with(|v| {
        v.return_pending = true;
        v.store_waker = true;
    });

    let spawner = super::TestSpawner::new();
    let handle = spawner.spawn(TestFuture);
    spawner.next().unwrap().run();
    TestState::with(|v| {
        assert_eq!(v.num_polls, 1);
        assert!(v.stashed_waker.is_some());
    });

    // Aborting the task should immediately schedule it to run even if
    // the waker does not fire.
    handle.abort();
    spawner.next().unwrap().run();

    TestState::with(|v| {
        assert_eq!(v.num_polls, 1, "task should not be polled again if aborted");
        assert!(v.task_dropped);
    });
}
