use std::mem;

/// Abort if the closure `f` panics.
///
/// Use this with care, this will cause the entire program to immediately
/// abort. Using this is only appropriate where unwinding from a panic would
/// result in corruption, or where an unrecoverable program error has been
/// detected.
#[inline]
pub(crate) fn abort_on_panic<T>(f: impl FnOnce() -> T) -> T {
    struct Bomb;

    impl Drop for Bomb {
        fn drop(&mut self) {
            std::process::abort();
        }
    }

    let bomb = Bomb;
    let t = f();
    mem::forget(bomb);
    t
}
