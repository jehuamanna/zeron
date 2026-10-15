use std::{future::Future, pin::Pin, sync::Arc};

use learning_application::{InfoHash, TorrentEngine as _};
use media_gateway::{MediaReader, MediaSource};

use crate::Router;

/// Streams one torrent file through the media gateway. Every response opens a
/// fresh engine reader that returns only verified bytes and waits for missing
/// pieces; dropping it with the response releases the engine's priority.
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
            let file = router
                .open_file(&info_hash, file_index)
                .await
                .map_err(std::io::Error::other)?;
            Ok(Box::new(file) as Box<dyn MediaReader>)
        })
    }
}
