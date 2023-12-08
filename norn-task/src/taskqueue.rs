use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;

use crate::{JoinHandle, Runnable, Schedule, TaskSet};

/// [`TaskQueue`] provides a way to spawn and run tasks.
///
/// ```rust
/// let tq = norn_task::TaskQueue::new();
///
/// tq.spawn(async { println!("Hello world") }).detach();
/// while let Some(runnable) = tq.next() {
///     runnable.run();
/// }
/// ```
#[derive(Clone)]
pub struct TaskQueue {
    shared: Rc<Shared>,
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TaskQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskQueue").finish()
    }
}

struct Shared {
    runqueue: RefCell<VecDeque<Runnable>>,
    taskset: TaskSet,
}

impl TaskQueue {
    /// Construct a new [`TaskQueue`].
    pub fn new() -> Self {
        let shared = Shared {
            runqueue: RefCell::new(VecDeque::with_capacity(1024)),
            taskset: TaskSet::default(),
        };
        Self {
            shared: Rc::new(shared),
        }
    }

    /// Spawn a [`Future`] onto the [`TaskQueue`].
    ///
    /// The future will immediately be queued for execution. Returns a [`JoinHandle`]
    /// which can be used to await the result of the future.
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let sched = Rc::clone(&self.shared);
        // Safety: The 'static bound on the future is required to ensure that the future does not reference
        //         data which can be dropped before the future. 'static guarantees that the future outlives
        //         all references it captures.
        let (runnable, handle) = unsafe { self.shared.taskset.bind(future, sched) };
        if let Some(runnable) = runnable {
            self.shared.schedule(runnable);
        }
        handle
    }

    /// Returns the next [`Runnable`] to be executed.
    pub fn next(&self) -> Option<Runnable> {
        let next = self.shared.runqueue.borrow_mut().pop_front();
        next
    }

    /// Returns the number of [`Runnable`]s in the queue.
    pub fn runnable(&self) -> usize {
        self.shared.runqueue.borrow().len()
    }

    /// Shutdown the [`TaskQueue`].
    ///
    /// Cancels all tasks and drops their [`Future`]s.
    pub fn shutdown(&self) {
        self.shared.taskset.shutdown();
        drop(self.shared.runqueue.take());
    }
}

impl Schedule for Rc<Shared> {
    fn schedule(&self, runnable: Runnable) {
        self.runqueue.borrow_mut().push_back(runnable);
    }

    fn unbind(&self, registered: &crate::RegisteredTask) {
        unsafe { self.taskset.remove(registered) };
    }
}
