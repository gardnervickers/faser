use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use pin_list::PinList;

/// [`Notify`] is a notification mechanism for tasks.
///
/// Tasks can wait on a [`Notify`] instance, and then be woken by
/// a call to [`Notify::notify`].
///
/// If a [`Notified`] is dropped before its future completes, it
/// will pass the wakeup to the next [`Notified`] in the queue.
pub(crate) struct Notify {
    inner: RefCell<Inner>,
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notify")
            .field("waiters", &self.inner.borrow().num_waiters)
            .finish()
    }
}

impl Default for Notify {
    fn default() -> Self {
        Self {
            inner: RefCell::new(Inner {
                // Safety: We don't need any checking for the waiters list because
                //         we always use the same list for Notified instances.
                waiters: PinList::new(unsafe { pin_list::id::Unchecked::new() }),
                num_waiters: 0,
            }),
        }
    }
}

impl Notify {
    /// Returns a [`Notified`] which can be used to wait on this [`Notify`].
    pub(crate) fn wait(&self) -> Notified<'_> {
        Notified {
            notify: self,
            node: pin_list::Node::new(),
        }
    }

    /// Returns the number of waiters on this [`Notify`].
    pub(crate) fn waiters(&self) -> usize {
        self.inner.borrow().num_waiters
    }

    /// Notify the next `n` waiters.
    pub(crate) fn notify(&self, n: usize) -> usize {
        let mut removed = 0;

        while removed != n {
            let mut inner = self.inner.borrow_mut();
            if let Ok(waker) = inner.waiters.cursor_front_mut().remove_current(()) {
                inner.num_waiters -= 1;
                drop(inner);
                waker.wake();
                removed += 1;
            } else {
                break;
            }
        }
        removed
    }
}

struct Inner {
    waiters: PinList<PLTypes>,
    num_waiters: usize,
}

struct PLTypes;
impl pin_list::Types for PLTypes {
    type Id = pin_list::id::Unchecked;

    type Protected = Waker;

    type Removed = ();

    type Unprotected = ();
}

pin_project_lite::pin_project! {
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct Notified<'notify> {
        notify: &'notify Notify,
        #[pin]
        node: pin_list::Node<PLTypes>,
    }

    impl PinnedDrop for Notified<'_> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            // First, check if the node has been initialized. Initialization
            // happens only if the node has been linked into the list. Nodes
            // which are removed are also initialized, but they are not linked.
            //
            // If a node was never initialized, then there is nothing to do.
            let node = match this.node.initialized_mut() {
                Some(initialized) => initialized,
                None => return,
            };
            // The node has been initialized, so we need to move it back
            // to the uninitialized state, which may unlink.
            let mut inner = this.notify.inner.borrow_mut();
            match node.reset(&mut inner.waiters) {
                (pin_list::NodeData::Linked(_waker), ()) => {
                    // The node was linked, so there is nothing left to do.
                    inner.num_waiters -= 1;
                }
                (pin_list::NodeData::Removed(()), ()) => {
                    // The node was not linked, which means it was notified,
                    // so pass the notification on to the next waiter.
                    if let Ok(waker) = inner.waiters.cursor_front_mut().remove_current(()) {
                        inner.num_waiters -= 1;
                        drop(inner);
                        waker.wake();
                    }
                }
            }
        }
    }
}

impl<'notify> Future for Notified<'notify> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let mut inner = this.notify.inner.borrow_mut();
        if let Some(node) = this.node.as_mut().initialized_mut() {
            match node.take_removed(&inner.waiters) {
                Err(node) => {
                    // We have not been woken up, register the waker.
                    *node.protected_mut(&mut inner.waiters).unwrap() = cx.waker().clone();
                    Poll::Pending
                }
                Ok(((), ())) => {
                    // The node has been removed from the list, so we can return ready.
                    Poll::Ready(())
                }
            }
        } else {
            // The node has not been initialized, so we need to initialize it.
            inner.waiters.push_back(this.node, cx.waker().clone(), ());
            inner.num_waiters += 1;
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {

    use std::pin::pin;

    use super::*;

    #[test]
    fn test_notify() {
        let (waker, count) = futures_test::task::new_count_waker();
        let mut cx = Context::from_waker(&waker);
        let notify = Notify::default();
        let mut notified = pin!(notify.wait());

        assert_eq!(notify.waiters(), 0);
        // Poll the notified once to register it.
        assert!(notified.as_mut().poll(&mut cx).is_pending());
        // The notified should be registered now.
        assert_eq!(notify.waiters(), 1);

        // Notify the notified.
        assert_eq!(notify.notify(1), 1);
        // This should have triggered a wakeup.
        assert_eq!(count, 1);
        assert!(notified.as_mut().poll(&mut cx).is_ready());
        // The notified should be unregistered now.
        assert_eq!(notify.waiters(), 0);
    }

    #[test]
    fn test_drop_notified() {
        let (waker, count) = futures_test::task::new_count_waker();
        let mut cx = Context::from_waker(&waker);
        let notify = Notify::default();
        let mut notified1 = Box::pin(notify.wait());
        let mut notified2 = Box::pin(notify.wait());

        // Poll once to register the notified with the list.
        assert!(notified1.as_mut().poll(&mut cx).is_pending());
        assert!(notified2.as_mut().poll(&mut cx).is_pending());

        // Notify a single waiter.
        assert_eq!(notify.notify(1), 1);
        assert_eq!(count, 1);
        // Drop the notified before it is woken up.
        drop(notified1);

        // We should see the notification passed to the next waiter.
        assert!(notified2.as_mut().poll(&mut cx).is_ready());
    }

    #[test]
    fn test_drop_with_waiters() {
        let (waker, _) = futures_test::task::new_count_waker();
        let mut cx = Context::from_waker(&waker);
        let notify = Notify::default();
        let mut notified1 = Box::pin(notify.wait());
        let mut notified2 = Box::pin(notify.wait());

        assert!(notified1.as_mut().poll(&mut cx).is_pending());
        assert!(notified2.as_mut().poll(&mut cx).is_pending());
    }
}
