//! A [`Park`] implementation providing a driver for
//! [io_uring].
//!
//! [`Park`]: faser_executor::park::Park
//! [io_uring](https://kernel.dk/io_uring.pdf)
#![deny(missing_debug_implementations, rust_2018_idioms)]

pub(crate) mod driver;
pub(crate) mod error;
pub(crate) mod fd;
pub(crate) mod operation;
pub(crate) mod util;

pub mod bufring;
pub mod io;
pub mod net;

pub use driver::{Driver, Handle};
pub use util::noop;
