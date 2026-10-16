//! Seekable plaintext view over a sealed resource.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use futures_util::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, ReadBuf, SeekFrom};

use crate::{PLAINTEXT_CHUNK_BYTES, ResourceCipher};

type Load<R> = BoxFuture<'static, (R, io::Result<(u64, Vec<u8>)>)>;

/// `AsyncRead + AsyncSeek` plaintext view over a sealed reader.
///
/// It reads and authenticates one whole sealed chunk at a time, keeps only
/// that chunk, and fails the read with [`io::ErrorKind::InvalidData`] rather
/// than return bytes from a chunk that failed authentication. Waits on the
/// inner reader, such as a torrent stream waiting for verified pieces, pass
/// through unchanged.
pub struct DecryptingReader<R> {
    inner: Option<R>,
    cipher: ResourceCipher,
    position: u64,
    current: Option<(u64, Vec<u8>)>,
    loading: Option<Load<R>>,
}

impl<R> std::fmt::Debug for DecryptingReader<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecryptingReader")
            .field("cipher", &self.cipher)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl<R: AsyncRead + AsyncSeek + Send + Unpin + 'static> DecryptingReader<R> {
    /// Wrap a reader positioned anywhere over the sealed bytes.
    pub const fn new(inner: R, cipher: ResourceCipher) -> Self {
        Self {
            inner: Some(inner),
            cipher,
            position: 0,
            current: None,
            loading: None,
        }
    }

    fn start_load(&mut self, index: u64) -> io::Result<()> {
        let mut inner = self
            .inner
            .take()
            .ok_or_else(|| io::Error::other("a previous chunk read failed"))?;
        let cipher = self.cipher.clone();
        self.loading = Some(Box::pin(async move {
            let result = async {
                let (offset, length) = cipher.sealed_span(index);
                inner.seek(SeekFrom::Start(offset)).await?;
                let mut sealed = vec![0_u8; length];
                inner.read_exact(&mut sealed).await?;
                Ok((index, cipher.open_chunk(index, &sealed)?))
            }
            .await;
            (inner, result)
        }));
        Ok(())
    }
}

impl<R: AsyncRead + AsyncSeek + Send + Unpin + 'static> AsyncRead for DecryptingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        loop {
            if let Some(loading) = this.loading.as_mut() {
                let (inner, result) = ready!(loading.as_mut().poll(context));
                this.loading = None;
                this.inner = Some(inner);
                this.current = Some(result?);
            }
            if this.position >= this.cipher.plaintext_length() || out.remaining() == 0 {
                return Poll::Ready(Ok(()));
            }
            let chunk = u64::from(PLAINTEXT_CHUNK_BYTES);
            let index = this.position / chunk;
            match &this.current {
                Some((loaded, plaintext)) if *loaded == index => {
                    let at = usize::try_from(this.position - index * chunk)
                        .unwrap_or_else(|_| unreachable!("an offset inside a chunk fits"));
                    let count = (plaintext.len() - at).min(out.remaining());
                    out.put_slice(&plaintext[at..at + count]);
                    this.position += count as u64;
                    return Poll::Ready(Ok(()));
                }
                _ => this.start_load(index)?,
            }
        }
    }
}

impl<R: AsyncRead + AsyncSeek + Send + Unpin + 'static> AsyncSeek for DecryptingReader<R> {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let target = match position {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.cipher.plaintext_length().checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        self.position = target.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "seek outside the resource")
        })?;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.position))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

    use super::DecryptingReader;
    use crate::{PLAINTEXT_CHUNK_BYTES, SEALED_CHUNK_BYTES, tests::fixture, tests::sealed};

    #[tokio::test]
    async fn arbitrary_ranges_round_trip() {
        let chunk = PLAINTEXT_CHUNK_BYTES as usize;
        let plaintext = fixture(3 * chunk + 1000);
        let (cipher, bytes) = sealed(&plaintext, "r1");
        for (start, length) in [
            (0, plaintext.len()),
            (0, 1),
            (chunk - 1, 2),
            (chunk, chunk),
            (plaintext.len() - 1, 1),
            (chunk / 2, 2 * chunk),
        ] {
            let mut reader = DecryptingReader::new(Cursor::new(bytes.clone()), cipher.clone());
            reader.seek(SeekFrom::Start(start as u64)).await.unwrap();
            let mut out = vec![0; length];
            reader.read_exact(&mut out).await.unwrap();
            assert_eq!(out, plaintext[start..start + length]);
        }
        let mut reader = DecryptingReader::new(Cursor::new(bytes), cipher);
        reader.seek(SeekFrom::End(-10)).await.unwrap();
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).await.unwrap();
        assert_eq!(tail, plaintext[plaintext.len() - 10..]);
    }

    #[tokio::test]
    async fn a_tampered_chunk_fails_the_read_without_returning_its_bytes() {
        let plaintext = fixture(2 * PLAINTEXT_CHUNK_BYTES as usize);
        let (cipher, mut bytes) = sealed(&plaintext, "r1");
        bytes[SEALED_CHUNK_BYTES as usize + 3] ^= 1;
        let mut reader = DecryptingReader::new(Cursor::new(bytes), cipher);
        let mut first = vec![0; PLAINTEXT_CHUNK_BYTES as usize];
        reader.read_exact(&mut first).await.unwrap();
        let mut next = [0_u8; 1];
        let error = reader.read_exact(&mut next).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(next, [0]);
    }

    #[tokio::test]
    async fn seeking_before_the_start_is_refused() {
        let (cipher, bytes) = sealed(&fixture(10), "r1");
        let mut reader = DecryptingReader::new(Cursor::new(bytes), cipher);
        assert!(reader.seek(SeekFrom::Current(-1)).await.is_err());
    }
}
