use std::cell::{Cell, RefCell, UnsafeCell};
use std::rc::Rc;
use std::sync::Arc;
use std::{io, mem};

use faser_executor::park::{Park, ParkMode};
use io_uring::squeue::{Flags, PushError};
use io_uring::types::{self, CancelBuilder, SubmitArgs, Timespec};
use io_uring::{cqueue, opcode, IoUring};
use log::{debug, error, trace, warn};

use crate::fd;
use crate::operation::{complete_operation, ConfiguredEntry, Op, Operation};
use crate::util::notify::Notify;
pub(crate) use futures::PushFuture;

mod context;
mod futures;
mod unpark;

const LOG: &str = "faser_uring::driver";

/// True if the needs_park check should check the submission and completion queues.
///
/// This will have a perf impact on each poll, but may ensure better overall performance.
const NEEDS_PARK_CHECK_RINGS: bool = false;

/// [`Driver`] provies a [`Park`] implementation which will drive
/// a [`IoUring`] instance, submitting new requests and waiting
/// for completions.
///
/// Interaction with the driver is done via [`Handle`]. The handle
/// can be used to submit new requests to the driver.
pub struct Driver {
    shared: Rc<Shared>,
    unparker: Arc<unpark::Unparker>,
    unparker_buf: mem::ManuallyDrop<Box<UnsafeCell<[u8; 8]>>>,
}

/// [`Handle`] is used to interact with the [`Driver`] and
/// the backing [`IoUring`] instance.
#[derive(Clone)]
pub struct Handle {
    shared: Rc<Shared>,
}

struct Shared {
    ring: RefCell<IoUring>,
    backpressure: Notify,
    status: Cell<Status>,
}

/// The status of the driver.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum Status {
    /// The driver is running and accepting new requests.
    Running,
    /// The driver is draining and will not accept new requests.
    Draining,
    /// The driver has shutdown and will not accept new requests.
    Shutdown,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle").finish()
    }
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver").finish()
    }
}

impl Handle {
    /// Returns a handle to the current driver.
    ///
    /// If the current thread is not in a driver context, this will panic.
    #[track_caller]
    pub fn current() -> Self {
        context::DriverContext::handle().expect("not in driver context")
    }

    pub(crate) fn submit<T>(&self, op: T) -> Op<T>
    where
        T: Operation + 'static,
    {
        Op::new(op, self.clone())
    }

    /// Issue a cancellation request.
    ///
    /// Setting `sync` to true will cause the cancellation to
    /// be performed synchronously. If `sync` is false, async
    /// cancellation will be attempted first followed by sync
    /// cancellation if the async cancellation fails.
    pub(crate) fn cancel(&self, criteria: CancelBuilder, sync: bool) -> io::Result<()> {
        self.shared.cancel(criteria, sync)
    }

    /// Attempt to push a new entry into the submission queue.
    ///
    /// If the submission queue is full, this will block until there
    /// is space or the driver has shutdown.
    pub(crate) fn push(&self, entry: ConfiguredEntry) -> PushFuture {
        PushFuture::new(Rc::clone(&self.shared), entry)
    }

    pub(crate) fn close_fd(&self, kind: &fd::FdKind) -> io::Result<()> {
        self.shared.close_fd(kind)
    }
}

impl Driver {
    /// [Driver::DRAIN_TOKEN] is a special token which is used to signal the driver has drained all requests.
    const DRAIN_TOKEN: usize = 0x01;

    /// [Driver::UNPARKER_WAKE_TOKEN] is a special token which is used to signal unparker wake events.
    const UNPARKER_WAKE_TOKEN: usize = 0x02;

    /// [Driver::CANCELLATION_TOKEN] is a special token which is used to signal cancellation events.
    const CANCELLATION_TOKEN: usize = 0x03;

    /// [Driver::CLOSE_FD_TOKEN] is a special token which is used to signal close fd events.
    const CLOSE_FD_TOKEN: usize = 0x04;

    /// Create a new [`Driver`] with the provided size from the provided [`io_uring::Builder`].
    pub fn new(mut builder: io_uring::Builder, size: u32) -> io::Result<Self> {
        let ring = builder.dontfork().build(size)?;
        Ok(Self {
            shared: Rc::new(Shared {
                ring: RefCell::new(ring),
                backpressure: Notify::default(),
                status: Cell::new(Status::Running),
            }),
            unparker: Arc::new(unpark::Unparker::new()?),
            unparker_buf: mem::ManuallyDrop::new(Box::new(UnsafeCell::new([0; 8]))),
        })
    }

    /// Returns a handle to the driver.
    ///
    /// The handle can be used to submit new requests to the driver.
    pub fn handle(&self) -> Handle {
        Handle {
            shared: Rc::clone(&self.shared),
        }
    }

    /// Prepare the ring for parking.
    ///
    /// Returns true if the ring is ready for parking.
    fn prepare_park(&self) -> bool {
        if self.shared.status() != Status::Running {
            return true;
        }
        let state = self.unparker.park();
        if !state.is_parked() {
            let fd = self.unparker.raw_fd();
            let fd = io_uring::types::Fd(fd);
            // Safety: We use the unparker to track the outstanding requests which use the unparker_buf, preventing
            //         any two requests from running at the same time.
            let unparker_ptr = self.unparker_buf.get();
            let opcode = io_uring::opcode::Read::new(fd, unparker_ptr as _, 8)
                .build()
                .user_data(Self::UNPARKER_WAKE_TOKEN as u64);
            // Safety: The request relies on some shared state which is marked as ManuallyDrop. The shared state
            //         is only ever dropped once the reactor has shutdown. Additionally, we're leaning on the safety
            //         requirements from prepare_unparker to ensure another [io_uring::SubmissionQueue] does not exist.

            if unsafe { self.shared.try_push_raw(&opcode) }.is_err() {
                return false;
            }
        }
        !state.woken()
    }

    /// Submits all pending entries to the ring.
    ///
    /// This will block the calling thread based on the provided `ParkMode`. It can return
    /// EBUSY, in which case the caller should retry.
    ///
    /// Returns the number of entries which were submitted.
    fn submit(&self, mut mode: ParkMode) -> io::Result<usize> {
        // If we're going to park, then prepare the unparker.
        if matches!(mode, ParkMode::Timeout(_) | ParkMode::NextCompletion) && !self.prepare_park() {
            // Preparing the unparker failed, don't park!
            mode = ParkMode::NoPark;
        }
        trace!(target: LOG, "submit.mode {:?}", mode);
        let submitted = self.shared.submit(mode)?;
        log::trace!(target: LOG, "submit.submitted {}", submitted);
        Ok(submitted)
    }

    /// Drain up to `max` entries from the ring.
    ///
    /// This will continuously drain entries from the ring until there are either
    /// no more entries left, or `max` entries have been drained. Once either of
    /// these conditions is true, this method will return the number of entries
    /// drained.
    ///
    /// `N` is the number of entries to drain at a time. This is used to allocate
    /// storage for copying the entries out of the ring. This should be a small value.
    fn drain<const N: usize>(&self, max: usize) -> usize {
        let mut entries: [mem::MaybeUninit<cqueue::Entry>; N] =
            unsafe { mem::MaybeUninit::uninit().assume_init() };
        let mut total_drained = 0;
        loop {
            let (entries, has_more) = self.shared.drain_fill(&mut entries);
            let nr_drained = entries.len();
            for cqe in entries {
                let user_data = cqe.user_data() as usize;
                if user_data == Self::DRAIN_TOKEN {
                    trace!(target: LOG, "drain.token");
                    self.shared.set_status(Status::Shutdown);
                    continue;
                }
                if user_data == Self::UNPARKER_WAKE_TOKEN {
                    trace!(target: LOG, "drain.token");
                    self.unparker.reset();
                    continue;
                }

                if user_data == Self::CANCELLATION_TOKEN {
                    trace!(target: LOG, "cancellation.token");
                    continue;
                }

                if user_data == Self::CLOSE_FD_TOKEN {
                    trace!(target: LOG, "close_fd.token");
                    continue;
                }

                if user_data <= 1024 {
                    let result = cqe.result();
                    let result = if result >= 0 {
                        Ok(result as u32)
                    } else {
                        Err(io::Error::from_raw_os_error(-result))
                    };
                    warn!(target: LOG, "drain.invalid_user_data {result:?}");
                    // Surely nothing in our heap is going to be allocated at < 1024!
                    // We are keeping this space reserved for additional operations.
                    continue;
                }
                // Safety: This is being called on a completion queue entry which has been generated
                // by a prior submission.
                unsafe { complete_operation(cqe) }
            }
            total_drained += nr_drained;
            if !has_more || total_drained >= max {
                break;
            }
        }
        total_drained
    }
}

impl Park for Driver {
    type Unparker = Arc<unpark::Unparker>;

    type Guard = context::DriverContextGuard;

    fn park(&mut self, mut mode: ParkMode) -> Result<(), io::Error> {
        let drained = self.drain::<32>(usize::MAX);
        if drained > 0 {
            trace!(target: LOG, "park.drained {}", drained);
            mode = ParkMode::NoPark;
        }

        loop {
            match self.submit(mode) {
                Ok(_) => return Ok(()),
                Err(err) if err.raw_os_error() == Some(libc::EBUSY) => {
                    trace!(target: LOG, "park.ebusy");
                    let drained = self.drain::<32>(usize::MAX);
                    trace!(target: LOG, "park.drained {}", drained);
                    mode = ParkMode::NoPark;
                    continue;
                }
                Err(err) if err.raw_os_error() == Some(libc::EINTR) => {
                    error!(target: LOG, "park.eintr");
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn enter(&self) -> Self::Guard {
        context::DriverContext::enter(self.handle())
    }

    fn unparker(&self) -> Self::Unparker {
        Arc::clone(&self.unparker)
    }

    fn needs_park(&self) -> bool {
        self.shared.needs_park()
    }

    fn shutdown(&mut self) {
        if self.shared.status() == Status::Shutdown {
            return;
        };
        loop {
            if self.shared.status() == Status::Shutdown {
                return;
            }
            if self.shared.status() == Status::Running {
                self.unparker.wake();
                self.shared.submit(ParkMode::NoPark).unwrap();
                self.shared.cancel_all().unwrap();
                let opcode = io_uring::opcode::Nop::new()
                    .build()
                    .flags(io_uring::squeue::Flags::IO_DRAIN)
                    .user_data(Self::DRAIN_TOKEN as u64);
                if unsafe { self.shared.try_push_raw(&opcode) }.is_ok() {
                    self.shared.set_status(Status::Draining);
                }
            }
            if self.shared.status() == Status::Draining {
                self.park(ParkMode::NextCompletion).unwrap();
            }
        }
    }
}

impl Shared {
    /// Get the current status of the driver.
    fn status(&self) -> Status {
        self.status.get()
    }

    /// Set the status of the driver.
    ///
    /// All waiters will be notified of the status change.
    fn set_status(&self, status: Status) {
        debug!(target: LOG, "status.change {:?} => {:?}", self.status.get(), status);
        if status != self.status.get() {
            // On status change, notify all waiters.
            self.backpressure.notify(usize::MAX);
        }
        self.status.set(status);
    }

    /// Attempt to push a new entry into the submission queue.
    ///
    /// If the submission queue is full, this will return the entry.
    fn try_push(&self, entry: ConfiguredEntry) -> Result<(), ConfiguredEntry> {
        let mut ring = self.ring.borrow_mut();
        let mut sq = ring.submission();
        if sq.is_full() {
            Err(entry)
        } else {
            let entry = entry.into_entry();
            unsafe { sq.push(&entry) }.unwrap();
            Ok(())
        }
    }

    /// Attempt to push a new raw entry into the submission queue.
    ///
    /// If the submission queue is full, this will return an error.
    unsafe fn try_push_raw(&self, entry: &io_uring::squeue::Entry) -> Result<(), PushError> {
        let mut ring = self.ring.borrow_mut();
        let mut sq = ring.submission();
        sq.push(entry)
    }

    /// Attempt to push a new raw entry into the submission queue.
    ///
    /// If the submission queue is full, this will attempt to submit
    /// once and then try again. If the submission fails, this will
    /// return an error.
    unsafe fn try_push_raw_submit(&self, entry: &io_uring::squeue::Entry) -> io::Result<()> {
        if self.try_push_raw(entry).is_err() {
            // Try to make space.
            self.submit(ParkMode::NoPark)?;
        }
        // Try again.
        self.try_push_raw(entry).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("failed to push entry: {:?}", err),
            )
        })?;
        Ok(())
    }

    /// Submit all entries in the submission queue.
    ///
    /// The provided `ParkMode` is used to determine if the
    /// submission should block on new completions or not.
    ///
    /// Returns the number of entries which were submitted.
    fn submit(&self, mode: ParkMode) -> io::Result<usize> {
        let ring = self.ring.borrow();
        let submitter = ring.submitter();
        let submitted = match mode {
            ParkMode::Timeout(duration) => {
                let ts = Timespec::new()
                    .sec(duration.as_secs())
                    .nsec(duration.subsec_nanos());
                let args = SubmitArgs::new().timespec(&ts);
                submitter.submit_with_args(1, &args)?
            }
            ParkMode::NextCompletion => {
                let args = SubmitArgs::new();
                submitter.submit_with_args(1, &args)?
            }
            ParkMode::NoPark => submitter.submit()?,
        };
        self.backpressure.notify(submitted);
        Ok(submitted)
    }

    /// Cancel a specific request synchronously.
    ///
    /// Returns an error if the request could not be cancelled.
    fn cancel(&self, criteria: CancelBuilder, sync: bool) -> io::Result<()> {
        // Submit all unsubmitted entries to the ring so that we can cancel them.
        self.submit(ParkMode::NoPark)?;

        // First try to submit an async cancel request, this avoids a syscall.
        let mut ring = self.ring.borrow_mut();
        if !sync {
            let mut sq = ring.submission();
            if !sq.is_full() {
                let cancel = opcode::AsyncCancel2::new(criteria)
                    .build()
                    .flags(Flags::SKIP_SUCCESS)
                    .user_data(Driver::CANCELLATION_TOKEN as u64);
                unsafe { sq.push(&cancel) }.unwrap();
                return Ok(());
            }
        }
        let submitter = ring.submitter();
        submitter.register_sync_cancel(None, criteria)?;
        Ok(())
    }

    fn close_fd(&self, kind: &fd::FdKind) -> io::Result<()> {
        let entry = match kind {
            fd::FdKind::Fd(fd) => opcode::Close::new(types::Fd(fd.0)).build(),
            fd::FdKind::Fixed(fd) => opcode::Close::new(types::Fixed(fd.0)).build(),
        }
        .flags(Flags::SKIP_SUCCESS)
        .user_data(Driver::CLOSE_FD_TOKEN as u64);
        unsafe { self.try_push_raw_submit(&entry) }?;
        Ok(())
    }

    /// Cancel all outstanding requests synchronously.
    pub(crate) fn cancel_all(&self) -> io::Result<()> {
        let ring = self.ring.borrow();
        let criteria = CancelBuilder::any();
        ring.submitter().register_sync_cancel(None, criteria)?;
        Ok(())
    }

    fn needs_park(&self) -> bool {
        // First check if there are any waiters, this is a cheap check
        // compared to checking the ring.
        if self.backpressure.waiters() > 0 {
            return true;
        }
        if NEEDS_PARK_CHECK_RINGS {
            let mut ring = self.ring.borrow_mut();
            let (_, sq, cq) = ring.split();
            sq.is_full() || cq.is_full()
        } else {
            false
        }
    }

    /// Drain the completion queue into the provided buffer.
    ///
    /// Returns the filled buffer, and a flag indicating if there are more entries after
    /// this buffer.
    fn drain_fill<'a, const N: usize>(
        &'a self,
        entries: &'a mut [mem::MaybeUninit<cqueue::Entry>; N],
    ) -> (&'a mut [cqueue::Entry], bool) {
        let mut ring = self.ring.borrow_mut();
        let mut cq = ring.completion();
        let has_more = cq.len() > entries.len();
        (cq.fill(entries), has_more)
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        Park::shutdown(self);
    }
}
