//! Filesystem operations.

mod dir;
mod file;
mod opts;

pub use dir::{create_dir, remove_dir, remove_file};
pub use file::File;
pub use opts::OpenOptions;
