//! Application use cases shared by Zeron desktop composition roots.

mod cache;
mod disk;
mod documents;
mod downloads;
mod drawings;
mod library;
mod local_import;
mod notes;
mod progress;
mod sharing;
mod streaming;
mod torrent_import;

pub use cache::{CacheError, CourseCache, RemovalReport};
pub use disk::{DISK_RESERVE_BYTES, DiskGuard, DiskState, available_bytes, fits};
pub use documents::{
    DocumentError, DocumentSource, MAX_DOCUMENT_BYTES, read_local_document, read_torrent_document,
};
pub use downloads::{Availability, DownloadScope};
pub use drawings::{Drawing, DrawingSave};
pub use learning_domain::{
    Anchor, CourseProgress, DrawingError, DrawingFile, LessonStatus, NoteError, Region,
};
pub use library::{
    CourseImportReceipt, CourseLibrary, LibraryError, LocalResource, TorrentResource,
};
pub use local_import::{
    CancellationFlag, ImportError, ImportLimits, ImportedCourse, VerifiedResource,
    import_local_course,
};
pub use notes::{Note, NoteChange};
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
