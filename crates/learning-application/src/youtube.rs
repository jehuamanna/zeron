//! Online-only YouTube courses (ADR 0018): a pasted link becomes a course
//! Zeron writes itself, so notes, progress, and search work as for any course.

use std::future::Future;

use futures_util::{StreamExt as _, stream};

use course_format::{
    COURSE_SCHEMA_VERSION, CourseManifest, ExternalReference, Lesson, Module, Publisher, Resource,
    ResourceKind,
};
use learning_domain::{PlaylistId, VideoId, YouTubeLink, clean_youtube_text};
use learning_storage::YouTubeVideo;
use thiserror::Error;

use crate::{CancellationFlag, CourseImportReceipt, CourseLibrary, LibraryError};

/// Longest title or channel name kept from YouTube, in characters.
pub const MAX_YOUTUBE_TEXT_CHARS: usize = 300;
/// Media type of a resource played by YouTube's own player.
pub const YOUTUBE_MEDIA_TYPE: &str = "video/x-youtube";
/// Most videos a playlist course holds; YouTube's keyless player lists no more.
pub const MAX_PLAYLIST_VIDEOS: usize = 200;
/// Video lookups in flight at once while adding a playlist.
const PLAYLIST_LOOKUPS_AT_ONCE: usize = 8;
/// SHA-256 of empty content: a YouTube resource has no package bytes.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// What YouTube says about a video, still untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoDetails {
    /// Video title.
    pub title: String,
    /// Channel name, when YouTube gives one.
    pub channel: Option<String>,
}

/// Why YouTube could not describe a video.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VideoLookupError {
    /// The owner does not allow the video to be played in other apps.
    #[error("the owner of this video does not allow it to be played outside YouTube")]
    NotEmbeddable,
    /// The video does not exist, is private, or was removed.
    #[error("this video is unavailable; it may be private or removed")]
    Unavailable,
    /// YouTube could not be reached or answered unexpectedly.
    #[error("YouTube could not be reached; check the internet connection")]
    Unreachable,
}

/// Describes YouTube videos. The desktop app implements it over HTTPS;
/// tests use a fake.
pub trait VideoMetadataSource {
    /// Look up one video's title and channel.
    fn video(
        &self,
        id: &VideoId,
    ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send;

    /// Look up a playlist's title and channel.
    fn playlist(
        &self,
        id: &PlaylistId,
    ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send;
}

/// Why a link could not become a course.
#[derive(Debug, Error)]
pub enum YouTubeAddError {
    /// The text is not a YouTube video or playlist link.
    #[error(transparent)]
    Link(#[from] learning_domain::YouTubeLinkError),
    /// A playlist link was given where one video was expected.
    #[error("this is a link to a playlist, not to one video")]
    Playlist,
    /// None of the playlist's videos can be played in Zeron.
    #[error("this playlist has no videos that can be played outside YouTube")]
    EmptyPlaylist,
    /// The learner cancelled adding the playlist.
    #[error("adding the playlist was cancelled")]
    Cancelled,
    /// YouTube could not describe the video.
    #[error(transparent)]
    Lookup(#[from] VideoLookupError),
    /// A course from a folder or torrent already uses this video's course ID.
    #[error("another course in the library already uses this video's ID")]
    CourseIdTaken,
    /// The library could not store the course.
    #[error(transparent)]
    Library(LibraryError),
}

/// A video ready to be added, with its cleaned details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubeVideoCourse {
    /// The video.
    pub video: VideoId,
    /// Cleaned title and channel.
    pub details: VideoDetails,
}

/// Parse a pasted link and look the video up. Nothing is fetched for a link
/// that is not a YouTube video.
///
/// # Errors
///
/// Returns [`YouTubeAddError`] for links that are not one video, and for
/// videos YouTube cannot describe.
pub async fn look_up_youtube_video(
    source: &impl VideoMetadataSource,
    link: &str,
) -> Result<YouTubeVideoCourse, YouTubeAddError> {
    let video = match YouTubeLink::parse(link)? {
        YouTubeLink::Video(video) => video,
        YouTubeLink::Playlist(_) => return Err(YouTubeAddError::Playlist),
    };
    let details = clean_details(
        source.video(&video).await?,
        &format!("YouTube video {}", video.as_str()),
    );
    Ok(YouTubeVideoCourse { video, details })
}

/// Clean untrusted details, naming an untitled item `fallback`.
fn clean_details(details: VideoDetails, fallback: &str) -> VideoDetails {
    let title = clean_youtube_text(&details.title, MAX_YOUTUBE_TEXT_CHARS);
    VideoDetails {
        title: if title.is_empty() {
            fallback.to_owned()
        } else {
            title
        },
        channel: details
            .channel
            .map(|channel| clean_youtube_text(&channel, MAX_YOUTUBE_TEXT_CHARS))
            .filter(|channel| !channel.is_empty()),
    }
}

/// A playlist ready to be added: its cleaned details and the videos that
/// can be played, in playlist order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubePlaylistCourse {
    /// The playlist.
    pub playlist: PlaylistId,
    /// Cleaned playlist title and channel.
    pub details: VideoDetails,
    /// Playable videos with their cleaned titles, in playlist order.
    pub videos: Vec<(VideoId, String)>,
    /// Videos left out because they are private, removed, or not embeddable.
    pub skipped: usize,
}

/// Look up a playlist whose video IDs YouTube's player listed. IDs are
/// untrusted: malformed ones are dropped, repeats keep their first place, and
/// at most [`MAX_PLAYLIST_VIDEOS`] are kept. Each video is then looked up,
/// a few at a time, and the ones YouTube will not play are skipped.
/// `progress` hears `(looked up, total)` after each video.
///
/// # Errors
///
/// Returns [`YouTubeAddError::EmptyPlaylist`] when nothing can be played,
/// [`YouTubeAddError::Cancelled`] when `cancel` is set, and
/// [`YouTubeAddError::Lookup`] when the playlist is unavailable or YouTube
/// cannot be reached.
pub async fn look_up_youtube_playlist(
    source: &impl VideoMetadataSource,
    playlist: &PlaylistId,
    video_ids: &[String],
    cancel: &CancellationFlag,
    mut progress: impl FnMut(usize, usize),
) -> Result<YouTubePlaylistCourse, YouTubeAddError> {
    let mut ids: Vec<VideoId> = Vec::new();
    for id in video_ids.iter().filter_map(|id| VideoId::parse(id)) {
        if ids.len() == MAX_PLAYLIST_VIDEOS {
            break;
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(YouTubeAddError::EmptyPlaylist);
    }
    let details = clean_details(
        source.playlist(playlist).await?,
        &format!("YouTube playlist {}", playlist.as_str()),
    );
    let total = ids.len();
    // Owned IDs keep the stream's futures free of borrowed iterator items,
    // so the whole lookup stays `Send` for the desktop command.
    let mut lookups = stream::iter(ids.into_iter().map(|id| async move {
        let found = source.video(&id).await;
        (id, found)
    }))
    .buffered(PLAYLIST_LOOKUPS_AT_ONCE);
    let mut videos = Vec::with_capacity(total);
    let mut skipped = 0;
    let mut done = 0;
    while let Some((id, found)) = lookups.next().await {
        if cancel.is_cancelled() {
            return Err(YouTubeAddError::Cancelled);
        }
        match found {
            Ok(found) => {
                let found = clean_details(found, &format!("YouTube video {}", id.as_str()));
                videos.push((id, found.title));
            }
            Err(VideoLookupError::NotEmbeddable | VideoLookupError::Unavailable) => skipped += 1,
            Err(error @ VideoLookupError::Unreachable) => return Err(error.into()),
        }
        done += 1;
        progress(done, total);
    }
    if videos.is_empty() {
        return Err(YouTubeAddError::EmptyPlaylist);
    }
    Ok(YouTubePlaylistCourse {
        playlist: playlist.clone(),
        details,
        videos,
        skipped,
    })
}

/// The course ID of a YouTube playlist course.
#[must_use]
pub fn youtube_playlist_course_id(playlist: &PlaylistId) -> String {
    format!("youtube-playlist-{}", playlist.as_str())
}

/// The course ID of a one-video YouTube course.
#[must_use]
pub fn youtube_video_course_id(video: &VideoId) -> String {
    format!("youtube-video-{}", video.as_str())
}

/// The resource ID of a YouTube video in its course.
#[must_use]
pub fn youtube_resource_id(video: &VideoId) -> String {
    format!("video-{}", video.as_str())
}

/// Build the `course.json` of a one-video YouTube course.
#[must_use]
pub fn youtube_video_manifest(course: &YouTubeVideoCourse) -> CourseManifest {
    youtube_manifest(
        youtube_video_course_id(&course.video),
        "1.0.0",
        &course.details,
        course.video.watch_url(),
        "Video",
        &[(course.video.clone(), course.details.title.clone())],
    )
}

/// Build the `course.json` of a playlist course at `version`: one lesson
/// per video, in playlist order.
#[must_use]
pub fn youtube_playlist_manifest(course: &YouTubePlaylistCourse, version: &str) -> CourseManifest {
    youtube_manifest(
        youtube_playlist_course_id(&course.playlist),
        version,
        &course.details,
        course.playlist.playlist_url(),
        "Videos",
        &course.videos,
    )
}

fn youtube_manifest(
    course_id: String,
    version: &str,
    details: &VideoDetails,
    source_url: String,
    module_title: &str,
    videos: &[(VideoId, String)],
) -> CourseManifest {
    CourseManifest {
        schema_version: COURSE_SCHEMA_VERSION,
        course_id,
        version: version.to_owned(),
        title: details.title.clone(),
        description: None,
        publisher: Publisher {
            name: details
                .channel
                .clone()
                .unwrap_or_else(|| "YouTube".to_owned()),
            publisher_id: None,
        },
        language: "und".to_owned(),
        category: None,
        tags: vec!["youtube".to_owned()],
        license: None,
        prerequisites: Vec::new(),
        references: vec![ExternalReference {
            label: "Watch on YouTube".to_owned(),
            url: source_url,
        }],
        thumbnail_resource_id: None,
        estimated_duration_seconds: None,
        modules: vec![Module {
            id: "youtube".to_owned(),
            title: module_title.to_owned(),
            position: 0,
            lessons: videos
                .iter()
                .zip(0_u32..)
                .map(|((video, title), position)| Lesson {
                    id: format!("lesson-{}", video.as_str()),
                    title: title.clone(),
                    position,
                    estimated_duration_seconds: None,
                    resources: vec![Resource {
                        id: youtube_resource_id(video),
                        kind: ResourceKind::Video,
                        path: format!("youtube/{}", video.as_str()),
                        media_type: YOUTUBE_MEDIA_TYPE.to_owned(),
                        byte_length: 0,
                        sha256: EMPTY_SHA256.to_owned(),
                        duration_seconds: None,
                        language: None,
                        relations: Vec::new(),
                    }],
                })
                .collect(),
        }],
    }
}

impl CourseLibrary {
    /// Add a looked-up YouTube video as a course, or refresh it. A removed
    /// course comes back with the learner's kept notes and progress.
    ///
    /// # Errors
    ///
    /// Returns [`YouTubeAddError::CourseIdTaken`] when a folder or torrent
    /// course uses the same ID, or [`YouTubeAddError::Library`] when the
    /// manifest is invalid or storage fails.
    pub fn add_youtube_video(
        &mut self,
        course: &YouTubeVideoCourse,
    ) -> Result<CourseImportReceipt, YouTubeAddError> {
        self.save_youtube_manifest(
            &youtube_video_manifest(course),
            std::slice::from_ref(&course.video),
        )
    }

    /// Add a looked-up playlist as a course, or refresh it. When its videos
    /// changed since the last time, the course version goes up a patch, so
    /// the course page offers to move notes whose video left the playlist.
    ///
    /// # Errors
    ///
    /// As [`Self::add_youtube_video`].
    pub fn add_youtube_playlist(
        &mut self,
        course: &YouTubePlaylistCourse,
    ) -> Result<CourseImportReceipt, YouTubeAddError> {
        let course_id = youtube_playlist_course_id(&course.playlist);
        let previous = self
            .storage
            .kept_course_manifest(&course_id)
            .map_err(|error| YouTubeAddError::Library(error.into()))?;
        let mut manifest = youtube_playlist_manifest(course, "1.0.0");
        if let Some(previous) = previous {
            let lessons = |manifest: &CourseManifest| -> Vec<String> {
                manifest
                    .modules
                    .iter()
                    .flat_map(|module| &module.lessons)
                    .map(|lesson| lesson.id.clone())
                    .collect()
            };
            let mut version = semver::Version::parse(&previous.version)
                .unwrap_or_else(|_| semver::Version::new(1, 0, 0));
            if lessons(&previous) != lessons(&manifest) {
                version.patch += 1;
            }
            manifest.version = version.to_string();
        }
        let videos: Vec<VideoId> = course.videos.iter().map(|(id, _)| id.clone()).collect();
        self.save_youtube_manifest(&manifest, &videos)
    }

    fn save_youtube_manifest(
        &mut self,
        manifest: &CourseManifest,
        videos: &[VideoId],
    ) -> Result<CourseImportReceipt, YouTubeAddError> {
        manifest
            .validate()
            .map_err(|error| YouTubeAddError::Library(LibraryError::Storage(error.into())))?;
        let rows: Vec<YouTubeVideo> = videos
            .iter()
            .map(|video| YouTubeVideo {
                resource_id: youtube_resource_id(video),
                video_id: video.as_str().to_owned(),
            })
            .collect();
        let saved =
            self.storage
                .save_youtube_course(manifest, &rows)
                .map_err(|error| match error {
                    learning_storage::StorageError::CourseIdTaken => YouTubeAddError::CourseIdTaken,
                    error => YouTubeAddError::Library(error.into()),
                })?;
        Ok(CourseImportReceipt {
            course_id: saved.course_id,
            course_version: saved.course_version,
            title: saved.title,
            verified_resource_count: rows.len(),
            total_resource_bytes: 0,
        })
    }

    /// Whether a course, in the library or removed, plays from YouTube.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn is_youtube_course(&self, course_id: &str) -> Result<bool, LibraryError> {
        Ok(self.storage.course_is_youtube(course_id)?)
    }

    /// The YouTube video a resource of a YouTube course plays, or `None`
    /// when the course is not a YouTube course in the library.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn youtube_video(
        &self,
        course_id: &str,
        resource_id: &str,
    ) -> Result<Option<VideoId>, LibraryError> {
        Ok(self
            .storage
            .youtube_video(course_id, resource_id)?
            .and_then(|id| VideoId::parse(&id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, ImportLimits};

    struct Fake(Result<VideoDetails, VideoLookupError>);

    impl VideoMetadataSource for Fake {
        fn video(
            &self,
            _: &VideoId,
        ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send {
            std::future::ready(self.0.clone())
        }

        fn playlist(
            &self,
            _: &PlaylistId,
        ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send {
            std::future::ready(self.0.clone())
        }
    }

    /// A playlist "Rust talks" by "Ferris" whose videos are titled by ID;
    /// IDs starting with `p` are private and with `x` unreachable.
    struct Talks;

    impl VideoMetadataSource for Talks {
        fn video(
            &self,
            id: &VideoId,
        ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send {
            std::future::ready(match id.as_str().as_bytes()[0] {
                b'p' => Err(VideoLookupError::Unavailable),
                b'x' => Err(VideoLookupError::Unreachable),
                _ => Ok(VideoDetails {
                    title: format!("Talk {}", id.as_str()),
                    channel: None,
                }),
            })
        }

        fn playlist(
            &self,
            _: &PlaylistId,
        ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send {
            std::future::ready(Ok(VideoDetails {
                title: "Rust talks".to_owned(),
                channel: Some("Ferris".to_owned()),
            }))
        }
    }

    /// Fails the test if anything is fetched.
    struct Never;

    impl VideoMetadataSource for Never {
        fn video(
            &self,
            _: &VideoId,
        ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send {
            panic!("nothing is fetched for a link that is not one video");
            #[expect(unreachable_code, reason = "gives the never-called function its type")]
            std::future::pending()
        }

        fn playlist(
            &self,
            _: &PlaylistId,
        ) -> impl Future<Output = Result<VideoDetails, VideoLookupError>> + Send {
            panic!("nothing is fetched for an empty playlist");
            #[expect(unreachable_code, reason = "gives the never-called function its type")]
            std::future::pending()
        }
    }

    fn list() -> PlaylistId {
        PlaylistId::parse("PLrustTalks").unwrap()
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[tokio::test]
    async fn playlists_keep_order_drop_bad_ids_and_repeats_and_skip_private_videos() {
        let mut seen = Vec::new();
        let course = look_up_youtube_playlist(
            &Talks,
            &list(),
            &ids(&[
                "aaaaaaaaaaa",
                "not an id",
                "privateaaaa",
                "bbbbbbbbbbb",
                "aaaaaaaaaaa",
                "ccccccccccc",
            ]),
            &CancellationFlag::default(),
            |done, total| seen.push((done, total)),
        )
        .await
        .unwrap();
        let titles: Vec<&str> = course
            .videos
            .iter()
            .map(|(_, title)| title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Talk aaaaaaaaaaa", "Talk bbbbbbbbbbb", "Talk ccccccccccc"]
        );
        assert_eq!(course.skipped, 1);
        assert_eq!(course.details.title, "Rust talks");
        assert_eq!(seen, [(1, 4), (2, 4), (3, 4), (4, 4)]);

        let manifest = youtube_playlist_manifest(&course, "1.0.0");
        manifest.validate().unwrap();
        assert_eq!(manifest.course_id, "youtube-playlist-PLrustTalks");
        assert_eq!(manifest.publisher.name, "Ferris");
        assert_eq!(
            manifest.references[0].url,
            "https://www.youtube.com/playlist?list=PLrustTalks"
        );
        let positions: Vec<u32> = manifest.modules[0]
            .lessons
            .iter()
            .map(|lesson| lesson.position)
            .collect();
        assert_eq!(positions, [0, 1, 2]);
    }

    #[tokio::test]
    async fn playlists_are_capped_and_fail_cleanly() {
        let many: Vec<String> = (0..250).map(|n| format!("v{n:010}")).collect();
        let course = look_up_youtube_playlist(
            &Talks,
            &list(),
            &many,
            &CancellationFlag::default(),
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(course.videos.len(), MAX_PLAYLIST_VIDEOS);
        youtube_playlist_manifest(&course, "1.0.0")
            .validate()
            .unwrap();

        let none = CancellationFlag::default();
        assert!(matches!(
            look_up_youtube_playlist(&Never, &list(), &ids(&["bad"]), &none, |_, _| {}).await,
            Err(YouTubeAddError::EmptyPlaylist)
        ));
        assert!(matches!(
            look_up_youtube_playlist(&Talks, &list(), &ids(&["privateaaaa"]), &none, |_, _| {})
                .await,
            Err(YouTubeAddError::EmptyPlaylist)
        ));
        assert!(matches!(
            look_up_youtube_playlist(
                &Talks,
                &list(),
                &ids(&["aaaaaaaaaaa", "xxxxxxxxxxx"]),
                &none,
                |_, _| {}
            )
            .await,
            Err(YouTubeAddError::Lookup(VideoLookupError::Unreachable))
        ));
        let cancelled = CancellationFlag::default();
        cancelled.cancel();
        assert!(matches!(
            look_up_youtube_playlist(
                &Talks,
                &list(),
                &ids(&["aaaaaaaaaaa"]),
                &cancelled,
                |_, _| {}
            )
            .await,
            Err(YouTubeAddError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn refreshing_a_changed_playlist_raises_its_version_and_keeps_notes() {
        let mut library = CourseLibrary::open_in_memory(ImportLimits::default()).unwrap();
        let none = CancellationFlag::default();
        let first = look_up_youtube_playlist(
            &Talks,
            &list(),
            &ids(&["aaaaaaaaaaa", "bbbbbbbbbbb"]),
            &none,
            |_, _| {},
        )
        .await
        .unwrap();
        let receipt = library.add_youtube_playlist(&first).unwrap();
        assert_eq!(
            (
                receipt.course_version.as_str(),
                receipt.verified_resource_count
            ),
            ("1.0.0", 2)
        );
        let course_id = receipt.course_id;
        assert_eq!(
            library
                .youtube_video(&course_id, "video-bbbbbbbbbbb")
                .unwrap()
                .unwrap()
                .as_str(),
            "bbbbbbbbbbb"
        );
        library
            .create_note(
                &course_id,
                "lesson-bbbbbbbbbbb",
                &Anchor::VideoTime { resource_id: "video-bbbbbbbbbbb".to_owned(), seconds: 12.0 },
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"keep"}]}]}"#,
            )
            .unwrap();

        // The same videos again: nothing changes.
        assert_eq!(
            library.add_youtube_playlist(&first).unwrap().course_version,
            "1.0.0"
        );

        // A video left the playlist and another joined.
        let second = look_up_youtube_playlist(
            &Talks,
            &list(),
            &ids(&["aaaaaaaaaaa", "ccccccccccc"]),
            &none,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(
            library
                .add_youtube_playlist(&second)
                .unwrap()
                .course_version,
            "1.0.1"
        );
        assert_eq!(
            library
                .youtube_video(&course_id, "video-bbbbbbbbbbb")
                .unwrap(),
            None
        );
        assert_eq!(
            library.notes(&course_id, None).unwrap().len(),
            1,
            "the note is kept"
        );
        let moves = library.annotation_moves(&course_id).unwrap();
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from_lesson_id, "lesson-bbbbbbbbbbb");
        assert_eq!(moves[0].from_title.as_deref(), Some("Talk bbbbbbbbbbb"));
    }

    fn found(title: &str, channel: Option<&str>) -> Fake {
        Fake(Ok(VideoDetails {
            title: title.to_owned(),
            channel: channel.map(str::to_owned),
        }))
    }

    const LINK: &str = "https://youtu.be/M7lc1UVf-VE";

    #[tokio::test]
    async fn a_video_link_becomes_a_valid_one_lesson_course() {
        let course = look_up_youtube_video(&found("  Embedded\nplayer ", Some("Google")), LINK)
            .await
            .unwrap();
        let manifest = youtube_video_manifest(&course);
        manifest.validate().unwrap();
        // The manifest survives the same parser every course goes through.
        let json = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(course_format::parse_and_validate(&json).unwrap(), manifest);
        assert_eq!(manifest.course_id, "youtube-video-M7lc1UVf-VE");
        assert_eq!(manifest.title, "Embedded player");
        assert_eq!(manifest.publisher.name, "Google");
        assert_eq!(
            manifest.references[0].url,
            "https://www.youtube.com/watch?v=M7lc1UVf-VE"
        );
        let lesson = &manifest.modules[0].lessons[0];
        assert_eq!(lesson.resources[0].id, "video-M7lc1UVf-VE");
        assert_eq!(lesson.resources[0].kind, ResourceKind::Video);
        assert_eq!(lesson.resources[0].byte_length, 0);
    }

    #[tokio::test]
    async fn ids_starting_with_a_dash_still_make_valid_ids() {
        let course = look_up_youtube_video(&found("T", None), "youtu.be/-_abcDEF012")
            .await
            .unwrap();
        let manifest = youtube_video_manifest(&course);
        manifest.validate().unwrap();
        assert_eq!(manifest.publisher.name, "YouTube");
    }

    #[tokio::test]
    async fn empty_titles_get_a_name_and_bad_links_fetch_nothing() {
        let course = look_up_youtube_video(&found("\u{7}", Some(" ")), LINK)
            .await
            .unwrap();
        assert_eq!(course.details.title, "YouTube video M7lc1UVf-VE");
        assert_eq!(course.details.channel, None);

        assert!(matches!(
            look_up_youtube_video(&Never, "https://example.com/v").await,
            Err(YouTubeAddError::Link(_))
        ));
        assert!(matches!(
            look_up_youtube_video(&Never, "https://www.youtube.com/playlist?list=PL123").await,
            Err(YouTubeAddError::Playlist)
        ));
        assert!(matches!(
            look_up_youtube_video(&Fake(Err(VideoLookupError::NotEmbeddable)), LINK).await,
            Err(YouTubeAddError::Lookup(VideoLookupError::NotEmbeddable))
        ));
    }

    #[tokio::test]
    async fn added_videos_take_notes_and_positions_and_come_back_after_removal() {
        let mut library = CourseLibrary::open_in_memory(ImportLimits::default()).unwrap();
        let course = look_up_youtube_video(&found("Player", None), LINK)
            .await
            .unwrap();
        let receipt = library.add_youtube_video(&course).unwrap();
        let course_id = receipt.course_id;
        assert!(library.is_youtube_course(&course_id).unwrap());
        let video = library
            .youtube_video(&course_id, "video-M7lc1UVf-VE")
            .unwrap()
            .unwrap();
        assert_eq!(video.as_str(), "M7lc1UVf-VE");
        assert_eq!(
            library
                .local_resource(&course_id, "video-M7lc1UVf-VE")
                .unwrap(),
            None
        );

        library
            .create_note(
                &course_id,
                "lesson-M7lc1UVf-VE",
                &Anchor::VideoTime {
                    resource_id: "video-M7lc1UVf-VE".to_owned(),
                    seconds: 90.0,
                },
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"embed origin"}]}]}"#,
            )
            .unwrap();
        assert!(
            library
                .save_playback_position(&course_id, "video-M7lc1UVf-VE", 90.0, 1344.0)
                .unwrap()
        );

        library.storage.remove_course(&course_id).unwrap();
        assert_eq!(
            library
                .youtube_video(&course_id, "video-M7lc1UVf-VE")
                .unwrap(),
            None
        );
        library.add_youtube_video(&course).unwrap();
        assert_eq!(library.notes(&course_id, None).unwrap().len(), 1);
        assert_eq!(
            library
                .playback_position(&course_id, "video-M7lc1UVf-VE")
                .unwrap(),
            Some(90.0)
        );
    }
}
