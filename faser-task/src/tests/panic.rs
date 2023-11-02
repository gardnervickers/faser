use futures::FutureExt;

use super::{TestFuture, TestState};

#[test]
fn panic_during_poll() {
    let _e = TestState::enter();
    TestState::with(|v| {
        v.panic_on_run = true;
    });

    let spawner = super::TestSpawner::new();
    let handle = spawner.spawn(TestFuture);
    spawner.next().unwrap().run();

    TestState::with(|v| {
        assert_eq!(v.num_polls, 1);
        assert!(v.task_dropped);
        assert!(!v.output_dropped);
    });
    assert!(handle.now_or_never().unwrap().is_err());
}

#[test]
fn panic_during_poll_abort() {
    let _e = TestState::enter();
    TestState::with(|v| {
        v.panic_on_run = true;
    });

    let spawner = super::TestSpawner::new();
    let handle = spawner.spawn(TestFuture);
    spawner.next().unwrap().run();
    handle.abort();
    TestState::with(|v| {
        assert_eq!(v.num_polls, 1);
        assert!(v.task_dropped);
        assert!(!v.output_dropped);
    });
    assert!(handle.now_or_never().unwrap().is_err());
}
