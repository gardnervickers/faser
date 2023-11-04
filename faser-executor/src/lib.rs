use std::future::Future;
use std::pin::pin;

use faser_task::JoinHandle;

mod context;
pub mod park;
mod wakerfn;

pub struct LocalExecutor<P: park::Park> {
    /// Task queue contains tasks which are ready to be executed.
    taskqueue: faser_task::TaskQueue,
    park: P,
}

impl<P: park::Park> LocalExecutor<P> {
    pub fn new(park: P) -> Self {
        Self {
            taskqueue: faser_task::TaskQueue::new(),
            park,
        }
    }

    pub fn handle(&self) -> Handle {
        Handle {
            taskqueue: self.taskqueue.clone(),
        }
    }
    pub fn block_on<F>(&mut self, fut: F) -> F::Output
    where
        F: Future,
    {
        let _g1 = self.park.enter();
        let _g2 = context::Context::enter(self.handle());
        let fut = pin!(fut);
        let mut root = wakerfn::FutureHarness::new(fut);

        loop {
            if let Some(result) = root.try_poll() {
                return result;
            }
            while let Some(next) = self.taskqueue.next() {
                next.run();
                if self.park.needs_park() {
                    break;
                }
            }
            let mut mode = park::ParkMode::NextCompletion;
            if root.is_notified() {
                mode = park::ParkMode::NoPark;
            }
            self.park.park(mode).unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub struct Handle {
    taskqueue: faser_task::TaskQueue,
}

impl Handle {
    pub fn current() -> Self {
        context::Context::handle().expect("executor not set")
    }

    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
    {
        self.taskqueue.spawn(future)
    }
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + 'static,
{
    Handle::current().spawn(future)
}

impl<P: park::Park> Drop for LocalExecutor<P> {
    fn drop(&mut self) {
        self.taskqueue.shutdown();
        self.park.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::task::Poll;

    use crate::park::SpinPark;

    use super::*;

    #[test]
    fn block_on() {
        let mut executor = LocalExecutor::new(SpinPark);

        let res = executor.block_on(async { 1 + 1 });
        assert_eq!(res, 2);
    }

    #[test]
    fn spawn_in_blockon() {
        let mut executor = LocalExecutor::new(SpinPark);

        let handle = executor.handle();
        let res = executor
            .block_on(async move {
                let handle = handle.clone();
                handle.spawn(async move { 1 + 1 }).await
            })
            .unwrap();
        assert_eq!(res, 2);
    }

    #[test]
    fn spawn_before_blockon() {
        let mut executor = LocalExecutor::new(SpinPark);
        let handle = executor.handle();

        let f1 = handle.spawn(async { 1 + 1 });
        executor.block_on(async move {
            let res = f1.await.unwrap();
            assert_eq!(res, 2);
        });
    }

    #[test]
    fn spawn_after_shutdown() {
        let executor = LocalExecutor::new(SpinPark);
        let handle = executor.handle();

        drop(executor);
        let f1 = handle.spawn(async { 1 + 1 });
        let f1 = pin!(f1);

        let waker = futures_test::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        let Poll::Ready(res) = f1.poll(&mut cx) else {
            panic!("expected ready");
        };
        assert!(res.is_err());
        assert!(res.err().unwrap().is_cancelled());
    }

    #[test]
    fn spawn_from_context() {
        let mut executor = LocalExecutor::new(SpinPark);

        executor.block_on(async {
            let f1 = crate::spawn(async { 1 + 1 });
            let res = f1.await.unwrap();
            assert_eq!(res, 2);
        })
    }
}
