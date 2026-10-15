use std::{future::Future, pin::Pin, sync::Arc};

use learning_application::{InfoHash, StreamWait, open_lesson_stream};
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
}

impl MediaSource for TorrentMedia {
    fn open(
        &self,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Box<dyn MediaReader>>> + Send + 'static>> {
        let router = Arc::clone(&self.router);
        let info_hash = self.info_hash.clone();
        let file_index = self.file_index;
        Box::pin(async move {
            let file = open_lesson_stream(&*router, &info_hash, file_index, StreamWait::default())
                .await
                .map_err(std::io::Error::other)?;
            Ok(Box::new(file) as Box<dyn MediaReader>)
        })
    }
}
