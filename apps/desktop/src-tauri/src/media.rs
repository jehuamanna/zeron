use std::{future::Future, pin::Pin, sync::Arc};

use learning_application::{
    DecryptingReader, InfoHash, ResourceCipher, StreamWait, open_lesson_stream,
};
use media_gateway::{MediaReader, MediaSource};

use crate::Router;

/// Streams one torrent file through the media gateway. Every response opens a
/// fresh engine reader that returns only verified bytes and waits for missing
/// pieces; dropping it with the response releases the engine's priority.
/// A lesson opened while its course is still being restored at startup waits
/// for the torrent instead of failing.
pub(crate) struct TorrentMedia {
    pub(crate) router: Arc<Router>,
    pub(crate) info_hash: InfoHash,
    pub(crate) file_index: usize,
    /// Opens a paid lesson's sealed bytes on top of the verified stream.
    pub(crate) cipher: Option<ResourceCipher>,
}

impl MediaSource for TorrentMedia {
    fn open(
        &self,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Box<dyn MediaReader>>> + Send + 'static>> {
        let router = Arc::clone(&self.router);
        let info_hash = self.info_hash.clone();
        let file_index = self.file_index;
        let cipher = self.cipher.clone();
        Box::pin(async move {
            let file = open_lesson_stream(&*router, &info_hash, file_index, StreamWait::default())
                .await
                .map_err(std::io::Error::other)?;
            Ok(match cipher {
                Some(cipher) => {
                    Box::new(DecryptingReader::new(file, cipher)) as Box<dyn MediaReader>
                }
                None => Box::new(file) as Box<dyn MediaReader>,
            })
        })
    }
}

/// A paid lesson of a local course: the package file stays sealed on disk and
/// every response decrypts the requested range, failing on altered bytes.
pub(crate) struct SealedFileMedia {
    pub(crate) path: std::path::PathBuf,
    pub(crate) cipher: ResourceCipher,
}

impl MediaSource for SealedFileMedia {
    fn open(
        &self,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Box<dyn MediaReader>>> + Send + 'static>> {
        let path = self.path.clone();
        let cipher = self.cipher.clone();
        Box::pin(async move {
            let file = tokio::fs::File::open(path).await?;
            if file.metadata().await?.len() != cipher.sealed_length() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the sealed lesson changed on disk",
                ));
            }
            Ok(Box::new(DecryptingReader::new(file, cipher)) as Box<dyn MediaReader>)
        })
    }
}
