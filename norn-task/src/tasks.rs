use std::cell::{Cell, RefCell};
use std::future::Future;
use std::marker::PhantomData;

use crate::header::Header;
use crate::{JoinHandle, RegisteredTask, Runnable, Schedule};

/// [`TaskSet`] tracks a set of registered tasks.
///
/// It can be used to cancel all tasks, such as during shutdown.
pub struct TaskSet {
    inner: RefCell<Inner>,
    size: Cell<usize>,
    // !Send
    _m: PhantomData<*const ()>,
}

impl std::fmt::Debug for TaskSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskSet").finish()
    }
}

struct Inner {
    list: cordyceps::List<Header>,
    closed: bool,
}

impl TaskSet {
    /// Construct a new [`TaskSet`].
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(Inner {
                closed: false,
                list: cordyceps::List::new(),
            }),
            size: Cell::new(0),
            _m: PhantomData,
        }
    }

    /// Bind a future to this [`TaskSet`], returning a [`Runnable`] and a [`JoinHandle`].
    ///
    /// If the [`TaskSet`] is closed, the [`Runnable`] will be `None` and the [`JoinHandle`]
    /// will immediately return `Err(())`.
    ///
    /// # Safety
    /// Callers must ensure that the provided [`Future`] outlives it's captures. The future cannot
    /// reference **anything** which may drop before the future itself.
    ///
    /// An easy way to guarantee this is to require that the future is `'static` along with it's output,
    /// however shorter lifetimes are also valid. For example, if you can prove that the [`TaskSet`]
    /// outlives all captures of a future, then you can safely bind that future to the [`TaskSet`].
    ///
    /// Once the [`TaskSet`] is closed or dropped, all futures (or their output) associated with it will be
    /// dropped, even if they are not yet complete or still in the scheduler queue.
    pub unsafe fn bind<T, S>(
        &self,
        future: T,
        scheduler: S,
    ) -> (Option<Runnable>, JoinHandle<T::Output>)
    where
        S: Schedule,
        T: Future,
    {
        let (task, handle) = crate::task_cell::TaskCell::allocate(future, scheduler);
        if self.is_closed() {
            task.shutdown();
            return (None, JoinHandle::from(handle));
        }
        let mut this = self.inner.borrow_mut();
        let bound = RegisteredTask::from(task.clone());
        this.list.push_back(bound);
        self.size.set(self.size.get() + 1);
        (Some(Runnable::from(task)), JoinHandle::from(handle))
    }

    fn is_closed(&self) -> bool {
        self.inner.borrow_mut().closed
    }

    /// Shutdown all tasks in the [`TaskSet`].
    ///
    /// This will prevent any new tasks from being added to the [`TaskSet`].
    pub fn shutdown(&self) {
        let mut this = self.inner.borrow_mut();
        this.closed = true;

        while let Some(task) = this.list.pop_front() {
            task.shutdown();
            self.size.set(self.size.get() - 1);
        }
    }

    /// Remove a task from the [`TaskSet`].
    ///
    /// # Safety
    /// The provided [`RegisteredTask`] must have been returned from a previous call to [`TaskSet::bind`]
    /// on this [`TaskSet`]. Do not use [`RegisteredTask`]s from other [`TaskSet`]s.
    pub unsafe fn remove(&self, task: &RegisteredTask) -> Option<RegisteredTask> {
        let mut inner = self.inner.borrow_mut();
        let task = unsafe { inner.list.remove(task.as_ptr()) };
        if task.is_some() {
            self.size.set(self.size.get() - 1);
        }
        task
    }
}

impl Default for TaskSet {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TaskSet {
    fn drop(&mut self) {
        self.shutdown();
    }
}
