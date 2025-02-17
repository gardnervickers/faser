use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{ready, Context, Poll};

use crate::driver::{Shared, Status};
use crate::error::SubmitError;
use crate::operation::ConfiguredEntry;
use crate::util::notify::Notified;

use super::LOG;

pin_project_lite::pin_project! {
    struct PushFutureInner<'a> {
        shared: &'a Shared,
        #[pin]
        notify: Option<Notified<'a>>,
        entry: Option<ConfiguredEntry>
    }
}

impl PushFuture {
    pub(super) fn new(shared: Rc<Shared>, entry: ConfiguredEntry) -> Self {
        let shared = Rc::into_raw(shared);
        // Safety: We leak the Rc via into_raw, so we know that the reference will
        // be alive for 'static.
        let shared: &'static Shared = unsafe { &*shared };
        let inner = PushFutureInner {
            shared,
            notify: None,
            entry: Some(entry),
        };
        PushFuture {
            shared: Some(shared),
            fut: Some(inner),
        }
    }
}

impl Future for PushFutureInner<'_> {
    type Output = Result<(), SubmitError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            if this.shared.status() != Status::Running {
                log::trace!(target: LOG, "ring.push.sutting_down");
                return Poll::Ready(Err(SubmitError::shutting_down()));
            }
            if let Some(notify) = this.notify.as_mut().as_pin_mut() {
                ready!(notify.poll(cx));
                Pin::set(&mut this.notify, None);
            }

            if let Err(entry) = this
                .shared
                .try_push(this.entry.take().expect("entry already submitted"))
            {
                // Put the entry back
                *this.entry = Some(entry);
                // Wait for the submission queue to have space
                log::trace!(target: LOG, "ring.push.full");
                Pin::set(&mut this.notify, Some(this.shared.backpressure.wait()));
                continue;
            }
            log::trace!(target: LOG, "ring.push.ok");
            return Poll::Ready(Ok(()));
        }
    }
}

pin_project_lite::pin_project! {
    /// A future which guarantees that the reactor will not be dropped
    pub(crate) struct PushFuture {
        shared: Option<&'static Shared>,
        #[pin]
        fut: Option<PushFutureInner<'static>>,
    }

    impl PinnedDrop for PushFuture {
        fn drop(this: Pin<&mut Self>) {
            // First we need to drop the future, which will drop the shared reference.
            let mut me = this.project();
            me.fut.set(None);
            // Now we can reconstruct the Rc and drop it.
            let shared = me.shared.take().unwrap();
            // Safety: We previously leaked the Rc via into_raw, so we know that the reference will
            // be valid.
            let shared = unsafe { Rc::from_raw(shared) };
            drop(shared)
        }
    }
}

impl Future for PushFuture {
    type Output = Result<(), SubmitError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().project();
        let fut = this
            .fut
            .as_pin_mut()
            .expect("cannot poll future after completion");
        fut.poll(cx)
    }
}
