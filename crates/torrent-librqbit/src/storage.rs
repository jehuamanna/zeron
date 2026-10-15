use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, bail};
use librqbit::{
    ManagedTorrentShared, TorrentMetadata,
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
};

/// Marker error for content that is missing or has the wrong length.
#[derive(Debug)]
pub(crate) struct ContentMismatch(pub(crate) String);

impl std::fmt::Display for ContentMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ContentMismatch {}

/// Serves an immutable content directory without ever opening a file for writing.
#[derive(Clone)]
pub(crate) struct ReadOnlyContent {
    pub(crate) root: PathBuf,
}

pub(crate) struct ReadOnlyStorage {
    files: Arc<Vec<File>>,
}

impl StorageFactory for ReadOnlyContent {
    type Storage = ReadOnlyStorage;

    fn create(
        &self,
        _shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<ReadOnlyStorage> {
        let files = metadata
            .file_infos
            .iter()
            .map(|info| {
                let relative = info.relative_filename.display().to_string();
                let mismatch = || {
                    anyhow::Error::new(ContentMismatch(format!(
                        "`{relative}` is missing or not a regular file"
                    )))
                };
                let path = self.root.join(&info.relative_filename);
                let metadata = std::fs::symlink_metadata(&path).map_err(|_| mismatch())?;
                if !metadata.file_type().is_file() {
                    return Err(mismatch());
                }
                if metadata.len() != info.len {
                    return Err(anyhow::Error::new(ContentMismatch(format!(
                        "`{relative}` has a different length than the torrent declares"
                    ))));
                }
                File::open(&path).map_err(|_| mismatch())
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(ReadOnlyStorage {
            files: Arc::new(files),
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

impl TorrentStorage for ReadOnlyStorage {
    fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> anyhow::Result<()> {
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        let file = self.files.get(file_id).context("unknown torrent file")?;
        read_exact_at(file, buf, offset)?;
        Ok(())
    }

    fn pwrite_all(&self, _: usize, _: u64, _: &[u8]) -> anyhow::Result<()> {
        bail!("seeded content is immutable")
    }

    fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
        bail!("seeded content is immutable")
    }

    fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
        bail!("seeded content is immutable")
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        let actual = self
            .files
            .get(file_id)
            .context("unknown torrent file")?
            .metadata()?
            .len();
        if actual == length {
            Ok(())
        } else {
            bail!("seeded content length differs from the torrent")
        }
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Self {
            files: Arc::clone(&self.files),
        }))
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset)? {
            0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            read => {
                buf = &mut buf[read..];
                offset += read as u64;
            }
        }
    }
    Ok(())
}
