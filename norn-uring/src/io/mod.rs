//! Contains traits and utilities for working with asynchronous I/O.
//!
//! TODO: Move these into their own crate.
use std::future::Future;
use std::io;

use crate::buf::{StableBuf, StableBufMut};
use crate::bufring;

/// [`AsyncReadOwned`] is like [`futures::io::AsyncRead`] but
/// it takes ownership of the buffer.
pub trait AsyncReadOwned {
    /// The future returned by [`AsyncReadOwned::read`].
    type ReadFuture<'a, B>: Future<Output = (io::Result<usize>, B)> + 'a
    where
        B: StableBufMut,
        Self: 'a;

    /// Read into the given buffer.
    fn read<B: StableBufMut>(&mut self, buf: B) -> Self::ReadFuture<'_, B>;
}

/// [`AsyncWriteOwned`] is like [`futures::io::AsyncWrite`] but
/// it takes ownership of the buffer.
pub trait AsyncWriteOwned {
    /// The future returned by [`AsyncWriteOwned::write`].
    type WriteFuture<'a, B>: Future<Output = (io::Result<usize>, B)> + 'a
    where
        B: StableBuf,
        Self: 'a;

    /// Write the given buffer.
    fn write<B: StableBuf>(&mut self, buf: B) -> Self::WriteFuture<'_, B>;
}

trait AsyncReadBufRing {
    type ReadRingFuture<'a>: Future<Output = io::Result<bufring::BufRingBuf>> + 'a
    where
        Self: 'a;

    fn read_with_ring(&mut self, ring: &bufring::BufRingBuf) -> Self::ReadRingFuture<'_>;
}

/// [`AsyncReadOwnedExt`] provides additional methods for [`AsyncReadOwned`].
pub trait AsyncWriteOwnedExt: AsyncWriteOwned {
    /// The future returned by [`AsyncWriteOwned::write_all`].
    type WriteAllFuture<'a, B>: Future<Output = (io::Result<usize>, B)> + 'a
    where
        B: StableBuf,
        Self: 'a;
    /// Write the entire contents of the given buffer.
    fn write_all<B: StableBuf>(&mut self, buf: B) -> Self::WriteAllFuture<'_, B>;
}

/// [`AsyncReadOwnedExt`] provides additional methods for [`AsyncReadOwned`].
pub trait AsyncReadOwnedExt: AsyncReadOwned {}

impl<T> AsyncReadOwnedExt for T where T: AsyncReadOwned {}

impl<T> AsyncWriteOwnedExt for T
where
    T: AsyncWriteOwned,
{
    type WriteAllFuture<'a, B> = impl Future<Output = (io::Result<usize>, B)> + 'a
    where
        B: StableBuf,
        Self: 'a;

    fn write_all<B: StableBuf>(&mut self, buf: B) -> Self::WriteAllFuture<'_, B> {
        async {
            let mut written = 0;
            let mut cursor = buf.into_cursor();
            while cursor.bytes_init() > 0 {
                let (res, b) = self.write(cursor).await;
                match res {
                    Ok(n) => {
                        written += n;
                        cursor = b;
                        cursor.consume(n);
                        if n == 0 {
                            break;
                        }
                    }
                    Err(e) => return (Err(e), b.into_inner()),
                }
            }
            (Ok(written), cursor.into_inner())
        }
    }
}

// impl AsyncReadOwned for &[u8] {
//     type ReadFuture<'a, B> = impl Future<Output = (io::Result<usize>, B)> + 'a
//     where
//         B: StableBufMut,
//         Self: 'a;

//     fn read<B: StableBufMut>(&mut self, mut buf: B) -> Self::ReadFuture<'_, B> {
//         async move {
//             let n = std::cmp::min(buf.bytes_remaining(), self.len());
//             unsafe {
//                 std::ptr::copy_nonoverlapping(self.as_ptr(), buf.stable_ptr_mut(), n);
//             }
//             (Ok(n), buf)
//         }
//     }
// }

// impl AsyncWriteOwned for &mut [u8] {
//     type WriteFuture<'a, B> = impl Future<Output = (io::Result<usize>, B)> + 'a
//     where
//         B: StableBuf,
//         Self: 'a;

//     fn write<B: StableBuf>(&mut self, buf: B) -> Self::WriteFuture<'_, B> {
//         async move {
//             let n = std::cmp::min(buf.bytes_init(), self.len());
//             unsafe {
//                 std::ptr::copy_nonoverlapping(buf.stable_ptr(), self.as_mut_ptr(), n);
//             }
//             (Ok(n), buf)
//         }
//     }
// }
