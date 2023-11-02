//! A [`Park`] implementation providing a driver for
//! [io_uring].
//!
//! [`Park`]: faser_executor::park::Park
//! [io_uring](https://kernel.dk/io_uring.pdf)
#![deny(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    clippy::missing_safety_doc
)]

pub(crate) mod driver;
pub(crate) mod error;
pub(crate) mod operation;
pub(crate) mod util;

pub use driver::{Driver, Handle};

pub use util::noop;
