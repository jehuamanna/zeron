//! Application use cases shared by Zeron desktop composition roots.

mod annotation_migration;
mod cache;
mod catalogs;
mod course_removal;
mod disk;
mod documents;
mod downloads;
mod drawings;
mod library;
mod local_import;
mod notes;
mod progress;
mod search;
mod sharing;
mod streaming;
mod torrent_import;
mod trust;
mod youtube;

pub use annotation_migration::{AnnotationMove, AnnotationMoveReport};
pub use cache::{CacheError, CourseCache, RemovalReport};
pub use catalogs::{CatalogEntryView, CatalogView, EntryStatus, MAX_CATALOG_URL_BYTES};
pub use course_removal::{
    DELETING_SUFFIX, SetAsideFiles, finish_interrupted_removals, set_aside_course_files,
};
pub use disk::{DISK_RESERVE_BYTES, DiskGuard, DiskState, available_bytes, fits};
pub use documents::{
    DocumentError, DocumentSource, MAX_DOCUMENT_BYTES, MAX_TEXT_BYTES, read_local_document,
    read_torrent_document,
};
pub use downloads::{Availability, DownloadScope};
pub use drawings::{Drawing, DrawingSave};
pub use learning_domain::{
    Anchor, CourseProgress, Cue, DrawingError, DrawingFile, HighlightColor, HighlightStyle,
    LessonStatus, MAX_LINK_BYTES as MAX_YOUTUBE_LINK_BYTES, MAX_TRANSCRIPT_BYTES, MatchReason,
    NoteError, PlaylistId, Region, TextHighlight, TranscriptError, VideoId, YouTubeLink,
    YouTubeLinkError, parse_transcript,
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
pub use search::{SearchKind, SearchResult};
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
pub use trust::CoursePublisher;
pub use youtube::{
    MAX_PLAYLIST_VIDEOS, MAX_YOUTUBE_TEXT_CHARS, VideoDetails, VideoLookupError,
    VideoMetadataSource, YOUTUBE_MEDIA_TYPE, YouTubeAddError, YouTubePlaylistCourse,
    YouTubeVideoCourse, look_up_youtube_playlist, look_up_youtube_video,
    youtube_playlist_course_id, youtube_playlist_manifest, youtube_resource_id,
    youtube_video_course_id, youtube_video_manifest,
};
