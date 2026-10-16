//! Spike: `zeron.chunked-xchacha20poly1305.v1` (ADR 0017 decision 2).
//!
//! A resource is split into fixed plaintext chunks. Chunk `i` is sealed with
//! XChaCha20-Poly1305 under a per-resource key derived from the release's
//! content key, nonce = `i` (unique per key), and associated data binding the
//! resource ID, the chunk index, and whether it is the final chunk. The
//! ciphertext is the concatenation of sealed chunks, each `chunk + 16` bytes
//! except the last. There is no header: the manifest carries `chunkSize` and
//! `plaintextLength`.

use std::{
    io::{self, Read, Write},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use futures_util::future::BoxFuture;
use hkdf::Hkdf;
use sha2::Sha256;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, ReadBuf, SeekFrom};

pub const TAG: usize = 16;

/// Per-resource cipher state shared by every reader of one resource.
#[derive(Clone)]
pub struct ResourceCipher {
    cipher: Arc<XChaCha20Poly1305>,
    resource_id: Arc<str>,
    pub chunk: usize,
    pub plain_len: u64,
}

impl ResourceCipher {
    pub fn new(content_key: &[u8; 32], resource_id: &str, chunk: usize, plain_len: u64) -> Self {
        let mut file_key = [0_u8; 32];
        Hkdf::<Sha256>::new(Some(b"zeron.content.v1"), content_key)
            .expand(resource_id.as_bytes(), &mut file_key)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        Self {
            cipher: Arc::new(XChaCha20Poly1305::new(&file_key.into())),
            resource_id: resource_id.into(),
            chunk,
            plain_len,
        }
    }

    pub fn chunk_count(&self) -> u64 {
        self.plain_len.div_ceil(self.chunk as u64).max(1)
    }

    pub fn cipher_len(&self) -> u64 {
        self.plain_len + self.chunk_count() * TAG as u64
    }

    fn plain_chunk_len(&self, index: u64) -> usize {
        let start = index * self.chunk as u64;
        (self.plain_len - start).min(self.chunk as u64) as usize
    }

    fn nonce(index: u64) -> XNonce {
        let mut nonce = [0_u8; 24];
        nonce[..8].copy_from_slice(&index.to_le_bytes());
        nonce.into()
    }

    fn aad(&self, index: u64) -> Vec<u8> {
        let mut aad = Vec::with_capacity(32 + self.resource_id.len());
        aad.extend_from_slice(b"zeron.chunk.v1\n");
        aad.extend_from_slice(&(self.resource_id.len() as u32).to_be_bytes());
        aad.extend_from_slice(self.resource_id.as_bytes());
        aad.extend_from_slice(&index.to_be_bytes());
        aad.push(u8::from(index + 1 == self.chunk_count()));
        aad
    }

    pub fn seal(&self, index: u64, plain: &[u8]) -> Vec<u8> {
        let aad = self.aad(index);
        self.cipher
            .encrypt(&Self::nonce(index), Payload { msg: plain, aad: &aad })
            .expect("sealing cannot fail for in-range lengths")
    }

    pub fn open(&self, index: u64, sealed: &[u8]) -> io::Result<Vec<u8>> {
        let aad = self.aad(index);
        self.cipher
            .decrypt(&Self::nonce(index), Payload { msg: sealed, aad: &aad })
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "chunk failed authentication"))
    }
}

/// Encrypt `src` into `dst`; returns the ciphertext length.
pub fn encrypt(src: &mut impl Read, dst: &mut impl Write, cipher: &ResourceCipher) -> io::Result<u64> {
    let mut written = 0;
    let mut buf = vec![0_u8; cipher.chunk];
    for index in 0..cipher.chunk_count() {
        let len = cipher.plain_chunk_len(index);
        src.read_exact(&mut buf[..len])?;
        let sealed = cipher.seal(index, &buf[..len]);
        dst.write_all(&sealed)?;
        written += sealed.len() as u64;
    }
    Ok(written)
}

type Load<R> = BoxFuture<'static, (R, io::Result<(u64, Vec<u8>)>)>;

/// `AsyncRead + AsyncSeek` plaintext view over a ciphertext reader. It reads
/// and authenticates one whole chunk at a time and never returns bytes from a
/// chunk that failed authentication.
pub struct DecryptingReader<R> {
    inner: Option<R>,
    cipher: ResourceCipher,
    pos: u64,
    current: Option<(u64, Vec<u8>)>,
    loading: Option<Load<R>>,
}

impl<R: AsyncRead + AsyncSeek + Send + Unpin + 'static> DecryptingReader<R> {
    pub fn new(inner: R, cipher: ResourceCipher) -> Self {
        Self { inner: Some(inner), cipher, pos: 0, current: None, loading: None }
    }

    fn load(&mut self, index: u64) {
        let mut inner = self.inner.take().expect("no load in flight");
        let cipher = self.cipher.clone();
        self.loading = Some(Box::pin(async move {
            let result = async {
                let offset = index * (cipher.chunk + TAG) as u64;
                inner.seek(SeekFrom::Start(offset)).await?;
                let mut sealed = vec![0_u8; cipher.plain_chunk_len(index) + TAG];
                inner.read_exact(&mut sealed).await?;
                Ok((index, cipher.open(index, &sealed)?))
            }
            .await;
            (inner, result)
        }));
    }
}

impl<R: AsyncRead + AsyncSeek + Send + Unpin + 'static> AsyncRead for DecryptingReader<R> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        loop {
            if let Some(loading) = this.loading.as_mut() {
                let (inner, result) = std::task::ready!(loading.as_mut().poll(cx));
                this.loading = None;
                this.inner = Some(inner);
                this.current = Some(result?);
            }
            if this.pos >= this.cipher.plain_len || out.remaining() == 0 {
                return Poll::Ready(Ok(()));
            }
            let index = this.pos / this.cipher.chunk as u64;
            match &this.current {
                Some((loaded, plain)) if *loaded == index => {
                    let at = (this.pos - index * this.cipher.chunk as u64) as usize;
                    let n = (plain.len() - at).min(out.remaining());
                    out.put_slice(&plain[at..at + n]);
                    this.pos += n as u64;
                    return Poll::Ready(Ok(()));
                }
                _ => this.load(index),
            }
        }
    }
}

impl<R: AsyncRead + AsyncSeek + Send + Unpin + 'static> AsyncSeek for DecryptingReader<R> {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let len = self.cipher.plain_len as i128;
        let target = match position {
            SeekFrom::Start(at) => at as i128,
            SeekFrom::End(delta) => len + delta as i128,
            SeekFrom::Current(delta) => self.pos as i128 + delta as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = target as u64;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.pos))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn fixture(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn sealed(plain: &[u8], id: &str, chunk: usize) -> (ResourceCipher, Vec<u8>) {
        let cipher = ResourceCipher::new(&[7; 32], id, chunk, plain.len() as u64);
        let mut out = Vec::new();
        encrypt(&mut Cursor::new(plain), &mut out, &cipher).unwrap();
        assert_eq!(out.len() as u64, cipher.cipher_len());
        (cipher, out)
    }

    async fn read_range(cipher: &ResourceCipher, ct: Vec<u8>, start: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut reader = DecryptingReader::new(Cursor::new(ct), cipher.clone());
        reader.seek(SeekFrom::Start(start)).await?;
        let mut out = vec![0; len];
        reader.read_exact(&mut out).await?;
        Ok(out)
    }

    #[tokio::test]
    async fn arbitrary_ranges_round_trip() {
        let plain = fixture(1000);
        let (cipher, ct) = sealed(&plain, "r1", 64);
        for (start, len) in [(0, 1000), (0, 1), (63, 2), (64, 64), (999, 1), (130, 500), (900, 100)] {
            let got = read_range(&cipher, ct.clone(), start, len).await.unwrap();
            assert_eq!(got, plain[start as usize..start as usize + len]);
        }
        let mut reader = DecryptingReader::new(Cursor::new(ct), cipher);
        let mut all = Vec::new();
        reader.read_to_end(&mut all).await.unwrap();
        assert_eq!(all, plain);
    }

    #[tokio::test]
    async fn empty_and_exact_multiple_lengths() {
        for len in [0, 64, 128] {
            let plain = fixture(len);
            let (cipher, ct) = sealed(&plain, "r", 64);
            let mut reader = DecryptingReader::new(Cursor::new(ct), cipher);
            let mut all = Vec::new();
            reader.read_to_end(&mut all).await.unwrap();
            assert_eq!(all, plain);
        }
    }

    #[tokio::test]
    async fn tampering_is_refused() {
        let plain = fixture(300);
        let (cipher, ct) = sealed(&plain, "r1", 64);

        let mut flipped = ct.clone();
        flipped[100] ^= 1;
        assert!(read_range(&cipher, flipped, 64, 10).await.is_err());

        // Swap chunks 0 and 1: the index in the AAD and nonce refuses it.
        let step = 64 + TAG;
        let mut swapped = ct.clone();
        swapped[..step].copy_from_slice(&ct[step..2 * step]);
        swapped[step..2 * step].copy_from_slice(&ct[..step]);
        assert!(read_range(&cipher, swapped, 0, 10).await.is_err());

        // A chunk spliced in from another resource with the same content key.
        let (_, other) = sealed(&plain, "r2", 64);
        let mut spliced = ct.clone();
        spliced[..step].copy_from_slice(&other[..step]);
        assert!(read_range(&cipher, spliced, 0, 10).await.is_err());

        // Truncation to a whole-chunk boundary: the new last chunk was sealed
        // as non-final, so a reader told the shorter length refuses it.
        let truncated = ct[..2 * step].to_vec();
        let short = ResourceCipher::new(&[7; 32], "r1", 64, 128);
        assert!(read_range(&short, truncated, 64, 10).await.is_err());

        // A wrong content key.
        let wrong = ResourceCipher::new(&[8; 32], "r1", 64, 300);
        assert!(read_range(&wrong, ct, 0, 10).await.is_err());
    }
}
