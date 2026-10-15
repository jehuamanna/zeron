//! Application use cases shared by Zeron desktop composition roots.

mod downloads;
mod library;
mod local_import;
mod sharing;
mod streaming;
mod torrent_import;

pub use downloads::{Availability, DownloadScope};
pub use library::{
    CourseImportReceipt, CourseLibrary, LibraryError, LocalResource, TorrentResource,
};
pub use local_import::{
    CancellationFlag, ImportError, ImportLimits, ImportedCourse, VerifiedResource,
    import_local_course,
};
pub use sharing::{CourseSharing, CourseSharingStatus, SharingError, TransferRouter};
pub use streaming::{StreamWait, open_lesson_stream};
pub use torrent_import::{
    TorrentImportBusy, TorrentImportFailure, TorrentImportSnapshot, TorrentImportStage,
    TorrentImportTimeouts, TorrentImports,
};
pub use torrent_port::{
    EngineError, InfoHash, NetworkProfile, PeerReachability, TorrentEngine, TorrentSource,
    TransferState, TransferStats,
};
