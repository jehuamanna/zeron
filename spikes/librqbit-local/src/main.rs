//! Throwaway Phase 0 spike: librqbit 9.0.1 against Zeron's own published release,
//! on 127.0.0.1 only, with DHT, trackers, LSD, UPnP, and persistence disabled.

use std::{
    collections::HashSet,
    io::SeekFrom,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerMode, ListenerOptions,
    ManagedTorrent, ManagedTorrentShared, Session, SessionOptions, TorrentMetadata,
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
};
use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, StructureEdit,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const VIDEO_BYTES: usize = 24 * 1024 * 1024;
const SMALL_BYTES: usize = 300 * 1024;
const LESSONS: usize = 6;
const VIDEO_LESSON: usize = 3;

struct Fixture {
    release_dir: PathBuf,
    torrent_bytes: Vec<u8>,
    info_hash: String,
    magnet: String,
    sources: Vec<(String, Vec<u8>)>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("zeron-librqbit-spike-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let result = run(&root).await;
    make_writable(&root);
    let _ = std::fs::remove_dir_all(&root);
    result
}

async fn run(root: &Path) -> anyhow::Result<()> {
    let fixture = publish_fixture(root)?;
    let package = fixture.release_dir.join("package");
    report(
        "fixture",
        format!("published release with info hash {}", fixture.info_hash),
    );

    // 1. librqbit parses Zeron's deterministic torrent and agrees on the info hash.
    let parsed = librqbit::torrent_from_bytes(&fixture.torrent_bytes)?;
    let parsed_hash = parsed.info_hash.as_string();
    report(
        "interop/parse",
        format!(
            "librqbit info hash {parsed_hash}, matches: {}",
            parsed_hash == fixture.info_hash
        ),
    );
    if parsed_hash != fixture.info_hash {
        bail!("info hash disagreement");
    }

    // 2. Seeding directly from the read-only release directory.
    let seed_ok = try_seed(root, "seed-a", &package, &fixture.torrent_bytes, false).await;
    report("seed/read-only/overwrite=false", describe(&seed_ok));
    let seed_ok = try_seed(root, "seed-b", &package, &fixture.torrent_bytes, true).await;
    report("seed/read-only/overwrite=true", describe(&seed_ok));

    // A read-only storage adapter seeds the immutable release in place.
    let before = snapshot_permissions(&package)?;
    let (seeder, seed_handle) = seed(root, "seed-c", &package, &fixture.torrent_bytes).await?;
    let seeder_addr = seeder.listen_addr().context("seeder has no listener")?;
    report(
        "seed/read-only-storage-adapter",
        format!(
            "seeding the read-only release in place on {seeder_addr}; {} of {} bytes verified by librqbit's initial check",
            seed_handle.stats().progress_bytes,
            seed_handle.stats().total_bytes
        ),
    );

    // 3. Learner: metadata-first via magnet with an explicit local peer.
    let leech_out = root.join("leech");
    let leecher = Session::new_with_opts(root.join("leech-default"), local_options()).await?;
    let started = Instant::now();
    let listed = leecher
        .add_torrent(
            AddTorrent::from_url(fixture.magnet.as_str()),
            Some(AddTorrentOptions {
                list_only: true,
                initial_peers: Some(vec![seeder_addr]),
                ..Default::default()
            }),
        )
        .await?;
    let AddTorrentResponse::ListOnly(list) = listed else {
        bail!("expected list-only response");
    };
    let files: Vec<(String, u64)> = list
        .info
        .iter_file_details()
        .map(|details| (details.filename.to_vec().join("/"), details.len))
        .collect();
    report(
        "learner/metadata",
        format!(
            "resolved {} files from magnet in {:?}; info hash matches: {}",
            files.len(),
            started.elapsed(),
            list.info_hash.as_string() == fixture.info_hash
        ),
    );
    let manifest_index = files
        .iter()
        .position(|(path, _)| path == "course.json")
        .context("course.json missing from torrent")?;
    let video_index = files
        .iter()
        .position(|(path, _)| path.ends_with("lesson-4.mp4"))
        .context("lesson 4 missing from torrent")?;

    // 4. Manifest-only download.
    let started = Instant::now();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(list.torrent_bytes.clone()),
            Some(AddTorrentOptions {
                only_files: Some(vec![manifest_index]),
                initial_peers: Some(vec![seeder_addr]),
                output_folder: Some(leech_out.to_string_lossy().into_owned()),
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("no handle for leech torrent")?;
    tokio::time::timeout(Duration::from_secs(60), handle.wait_until_completed()).await??;
    let manifest_bytes = std::fs::read(leech_out.join("course.json"))?;
    let manifest = course_format::parse_and_validate(&manifest_bytes)?;
    let stats = handle.stats();
    report(
        "learner/manifest-only",
        format!(
            "course.json verified in {:?}: '{}' with {} lessons; downloaded {} of {} bytes; per-file progress {:?}",
            started.elapsed(),
            manifest.title,
            manifest.modules[0].lessons.len(),
            stats.progress_bytes,
            stats.total_bytes,
            stats.file_progress
        ),
    );

    // 5. Select lesson four and read from the middle before the file completes.
    let mut selected: HashSet<usize> = HashSet::from([manifest_index, video_index]);
    leecher.update_only_files(&handle, &selected).await?;
    let (video_path, video_bytes) = fixture
        .sources
        .iter()
        .find(|(name, _)| name.ends_with("lesson-4.mp4"))
        .cloned()
        .context("video source missing")?;
    let offset = (VIDEO_BYTES / 2) as u64 + 12_345;
    let mut stream = Arc::clone(&handle).stream(video_index).await?;
    let started = Instant::now();
    stream.seek(SeekFrom::Start(offset)).await?;
    let mut buffer = vec![0_u8; 64 * 1024];
    tokio::time::timeout(Duration::from_secs(60), stream.read_exact(&mut buffer)).await??;
    let range = usize::try_from(offset)?..usize::try_from(offset)? + buffer.len();
    let stats = handle.stats();
    report(
        "learner/seek-into-absent-data",
        format!(
            "{video_path}: read 64 KiB at offset {offset} in {:?}; bytes match source: {}; torrent progress at that moment {} of {} bytes; per-file progress {:?}",
            started.elapsed(),
            buffer == video_bytes[range],
            stats.progress_bytes,
            stats.total_bytes,
            stats.file_progress
        ),
    );
    drop(stream);

    // 6. Bounded cancellation: pause the transfer, then time out a read of absent data.
    leecher.pause(&handle).await?;
    let mut stream = Arc::clone(&handle).stream(video_index).await?;
    stream
        .seek(SeekFrom::Start((VIDEO_BYTES - 128 * 1024) as u64))
        .await?;
    let started = Instant::now();
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut buffer)).await;
    report(
        "learner/cancel-while-paused",
        format!(
            "read of absent data while paused: {} after {:?}; dropping the stream returned immediately",
            if read.is_err() {
                "timed out (pending, not served)"
            } else {
                "completed unexpectedly"
            },
            started.elapsed()
        ),
    );
    drop(stream);
    leecher.unpause(&handle).await?;
    tokio::time::timeout(Duration::from_secs(120), wait_files(&handle, &selected)).await??;
    let seed_stats = seed_handle.stats();
    report(
        "seed/metrics",
        format!(
            "seeder uploaded {} bytes; live: {}",
            seed_stats.uploaded_bytes,
            seed_stats
                .live
                .as_ref()
                .map_or_else(|| "none".to_owned(), ToString::to_string)
        ),
    );
    let leech_stats = handle.stats();
    report(
        "learner/selected-files-complete",
        format!(
            "selected files complete; per-file progress {:?} (unselected files only touched through shared boundary pieces)",
            leech_stats.file_progress
        ),
    );

    report(
        "seed/release-untouched",
        format!(
            "release files still read-only and byte-identical after seeding: {}",
            snapshot_permissions(&package)? == before
        ),
    );

    // 7. Restart reuse without any peers: a new session over the same output folder.
    leecher.stop().await;
    seeder.stop().await;
    let restarted = Session::new_with_opts(root.join("leech-default-2"), local_options()).await?;
    selected.insert(manifest_index);
    let started = Instant::now();
    let reused = restarted
        .add_torrent(
            AddTorrent::from_bytes(list.torrent_bytes.clone()),
            Some(AddTorrentOptions {
                only_files: Some(selected.iter().copied().collect()),
                output_folder: Some(leech_out.to_string_lossy().into_owned()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("no handle after restart")?;
    tokio::time::timeout(Duration::from_secs(60), reused.wait_until_completed()).await??;
    report(
        "learner/restart-reuse",
        format!(
            "after restart with no peers, selected files verified complete from disk in {:?}: {} bytes",
            started.elapsed(),
            reused.stats().progress_bytes
        ),
    );
    restarted.stop().await;
    Ok(())
}

async fn wait_files(handle: &Arc<ManagedTorrent>, files: &HashSet<usize>) -> anyhow::Result<()> {
    loop {
        let stats = handle.stats();
        let lengths: Vec<u64> =
            handle.with_metadata(|metadata| metadata.info.iter_file_lengths().collect())?;
        if files
            .iter()
            .all(|index| stats.file_progress.get(*index) == lengths.get(*index))
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn try_seed(
    root: &Path,
    name: &str,
    package: &Path,
    torrent: &[u8],
    overwrite: bool,
) -> anyhow::Result<String> {
    let session = Session::new_with_opts(root.join(name), local_options()).await?;
    let outcome = async {
        let handle = session
            .add_torrent(
                AddTorrent::from_bytes(torrent.to_vec()),
                Some(AddTorrentOptions {
                    output_folder: Some(package.to_string_lossy().into_owned()),
                    overwrite,
                    ..Default::default()
                }),
            )
            .await?
            .into_handle()
            .context("no handle")?;
        tokio::time::timeout(Duration::from_secs(30), handle.wait_until_completed()).await??;
        Ok::<_, anyhow::Error>(format!(
            "seeding; {} of {} bytes verified",
            handle.stats().progress_bytes,
            handle.stats().total_bytes
        ))
    }
    .await;
    session.stop().await;
    outcome
}

async fn seed(
    root: &Path,
    name: &str,
    package: &Path,
    torrent: &[u8],
) -> anyhow::Result<(Arc<Session>, Arc<ManagedTorrent>)> {
    let session = Session::new_with_opts(root.join(name), local_options()).await?;
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(torrent.to_vec()),
            Some(AddTorrentOptions {
                output_folder: Some(package.to_string_lossy().into_owned()),
                storage_factory: Some(
                    ReadOnlyRelease {
                        root: package.to_path_buf(),
                    }
                    .boxed(),
                ),
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("no seed handle")?;
    tokio::time::timeout(Duration::from_secs(30), handle.wait_until_completed()).await??;
    Ok((session, handle))
}

fn local_options() -> SessionOptions {
    SessionOptions {
        bind_device_name: std::env::var("SPIKE_BIND_DEVICE").ok(),
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        persistence: None,
        fastresume: false,
        ipv4_only: true,
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpOnly,
            listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            enable_upnp_port_forwarding: false,
            ipv4_only: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn publish_fixture(root: &Path) -> anyhow::Result<Fixture> {
    let sources_dir = root.join("sources");
    std::fs::create_dir_all(&sources_dir)?;
    let mut drafts = PublisherDrafts::open_in_memory()?;
    let draft_id = drafts.create()?.draft.draft_id;
    drafts.update_metadata(
        &draft_id,
        0,
        MetadataEdit {
            version: "1.0.0".to_owned(),
            title: "Spike Course".to_owned(),
            publisher_name: "Zeron Spike".to_owned(),
            language: "en".to_owned(),
            ..MetadataEdit::default()
        },
    )?;
    let module_id = drafts
        .edit_structure(
            &draft_id,
            1,
            StructureEdit::AddModule {
                title: "Module".to_owned(),
            },
        )?
        .draft
        .modules[0]
        .id
        .clone();
    let mut revision = 2;
    let mut sources = Vec::new();
    for lesson in 0..LESSONS {
        let lesson_id = drafts
            .edit_structure(
                &draft_id,
                revision,
                StructureEdit::AddLesson {
                    module_id: module_id.clone(),
                    title: format!("Lesson {}", lesson + 1),
                },
            )?
            .draft
            .modules[0]
            .lessons[lesson]
            .id
            .clone();
        revision += 1;
        let (name, length) = if lesson == VIDEO_LESSON {
            (format!("lesson-{}.mp4", lesson + 1), VIDEO_BYTES)
        } else {
            (format!("lesson-{}.txt", lesson + 1), SMALL_BYTES)
        };
        let bytes: Vec<u8> = (0..length)
            .map(|index| (index.wrapping_mul(2_654_435_761) >> 13) as u8 ^ lesson as u8)
            .collect();
        let path = sources_dir.join(&name);
        std::fs::write(&path, &bytes)?;
        drafts.attach_source(&draft_id, revision, &lesson_id, &path)?;
        revision += 1;
        sources.push((name, bytes));
    }
    let draft = drafts.draft(&draft_id)?.context("draft missing")?.draft;
    let jobs = PublishJobs::new(&root.join("staging"));
    let job = jobs.begin(&draft)?;
    jobs.run(
        &job.job_id,
        &draft,
        &DistributionOptions::default(),
        &mut |_| {},
    )
    .context("job missing")?;
    let (record, _) = jobs.activate(&job.job_id, &mut drafts, &root.join("releases"))?;
    let release_dir = root.join("releases").join(&record.release.release_id);
    Ok(Fixture {
        torrent_bytes: std::fs::read(release_dir.join("release.torrent"))?,
        release_dir,
        info_hash: record.release.info_hash,
        magnet: record.release.magnet,
        sources,
    })
}

fn report(scenario: &str, finding: String) {
    println!("[{scenario}] {finding}");
}

fn describe(result: &anyhow::Result<String>) -> String {
    match result {
        Ok(message) => format!("OK: {message}"),
        Err(error) => format!("FAILED: {error:#}"),
    }
}

/// Seeds an immutable release without ever opening a file for writing.
#[derive(Clone)]
struct ReadOnlyRelease {
    root: PathBuf,
}

struct ReadOnlyStorage {
    files: Arc<Vec<std::fs::File>>,
}

impl StorageFactory for ReadOnlyRelease {
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
                let file = std::fs::File::open(self.root.join(&info.relative_filename))?;
                if file.metadata()?.len() != info.len {
                    bail!("release file length differs from the torrent");
                }
                Ok(file)
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
        use std::os::unix::fs::FileExt;
        self.files
            .get(file_id)
            .context("unknown file")?
            .read_exact_at(buf, offset)?;
        Ok(())
    }

    fn pwrite_all(&self, _: usize, _: u64, _: &[u8]) -> anyhow::Result<()> {
        bail!("release files are immutable")
    }

    fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
        bail!("release files are immutable")
    }

    fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
        bail!("release files are immutable")
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        let actual = self
            .files
            .get(file_id)
            .context("unknown file")?
            .metadata()?
            .len();
        if actual == length {
            Ok(())
        } else {
            bail!("release file length differs from the torrent")
        }
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(ReadOnlyStorage {
            files: Arc::clone(&self.files),
        }))
    }
}

fn snapshot_permissions(root: &Path) -> anyhow::Result<Vec<(PathBuf, bool, Vec<u8>)>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let readonly = std::fs::metadata(&path)?.permissions().readonly();
                entries.push((path.clone(), readonly, std::fs::read(&path)?));
            }
        }
    }
    entries.sort();
    Ok(entries)
}

fn make_writable(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            make_writable(&path);
        } else if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&path, permissions);
        }
    }
}
