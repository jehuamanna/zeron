//! Application use cases shared by Zeron desktop composition roots.

mod cache;
mod disk;
mod downloads;
mod library;
mod local_import;
mod progress;
mod sharing;
mod streaming;
mod torrent_import;

pub use cache::{CacheError, CourseCache, RemovalReport};
pub use disk::{DISK_RESERVE_BYTES, DiskGuard, DiskState, available_bytes, fits};
pub use downloads::{Availability, DownloadScope};
pub use learning_domain::{CourseProgress, LessonStatus};
pub use library::{
    CourseImportReceipt, CourseLibrary, LibraryError, LocalResource, TorrentResource,
};
pub use local_import::{
    CancellationFlag, ImportError, ImportLimits, ImportedCourse, VerifiedResource,
    import_local_course,
};
pub use progress::CourseProgressView;
pub use sharing::{CourseSharing, CourseSharingStatus, SharingError, TransferRouter};
pub use streaming::{StreamWait, open_lesson_stream};
pub use torrent_import::{
    TorrentImportBusy, TorrentImportFailure, TorrentImportSnapshot, TorrentImportStage,
    TorrentImportTimeouts, TorrentImports,
};
pub use torrent_port::{
    EngineError, InfoHash, NetworkProfile, PeerReachability, RateLimits, TorrentEngine,
    TorrentSource, TransferState, TransferStats,
};
