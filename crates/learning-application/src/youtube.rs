//! Online-only YouTube courses (ADR 0018): a pasted link becomes a course
//! Zeron writes itself, so notes, progress, and search work as for any course.

use std::future::Future;

use course_format::{
    COURSE_SCHEMA_VERSION, CourseManifest, ExternalReference, Lesson, Module, Publisher, Resource,
    ResourceKind,
};
use learning_domain::{VideoId, YouTubeLink, clean_youtube_text};
use learning_storage::YouTubeVideo;
use thiserror::Error;

use crate::{CourseImportReceipt, CourseLibrary, LibraryError};

/// Longest title or channel name kept from YouTube, in characters.
pub const MAX_YOUTUBE_TEXT_CHARS: usize = 300;
/// Media type of a resource played by YouTube's own player.
pub const YOUTUBE_MEDIA_TYPE: &str = "video/x-youtube";
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
}

/// Why a link could not become a course.
#[derive(Debug, Error)]
pub enum YouTubeAddError {
    /// The text is not a YouTube video or playlist link.
    #[error(transparent)]
    Link(#[from] learning_domain::YouTubeLinkError),
    /// Playlists are not supported yet.
    #[error("playlists are not supported yet; paste a link to one video")]
    Playlist,
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
    let details = source.video(&video).await?;
    let title = clean_youtube_text(&details.title, MAX_YOUTUBE_TEXT_CHARS);
    let channel = details
        .channel
        .map(|channel| clean_youtube_text(&channel, MAX_YOUTUBE_TEXT_CHARS))
        .filter(|channel| !channel.is_empty());
    Ok(YouTubeVideoCourse {
        details: VideoDetails {
            title: if title.is_empty() {
                format!("YouTube video {}", video.as_str())
            } else {
                title
            },
            channel,
        },
        video,
    })
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
    let video = &course.video;
    CourseManifest {
        schema_version: COURSE_SCHEMA_VERSION,
        course_id: youtube_video_course_id(video),
        version: "1.0.0".to_owned(),
        title: course.details.title.clone(),
        description: None,
        publisher: Publisher {
            name: course
                .details
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
            url: video.watch_url(),
        }],
        thumbnail_resource_id: None,
        estimated_duration_seconds: None,
        modules: vec![Module {
            id: "youtube".to_owned(),
            title: "Video".to_owned(),
            position: 0,
            lessons: vec![Lesson {
                id: format!("lesson-{}", video.as_str()),
                title: course.details.title.clone(),
                position: 0,
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
            }],
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
        let manifest = youtube_video_manifest(course);
        manifest
            .validate()
            .map_err(|error| YouTubeAddError::Library(LibraryError::Storage(error.into())))?;
        let videos = [YouTubeVideo {
            resource_id: youtube_resource_id(&course.video),
            video_id: course.video.as_str().to_owned(),
        }];
        let saved = self
            .storage
            .save_youtube_course(&manifest, &videos)
            .map_err(|error| match error {
                learning_storage::StorageError::CourseIdTaken => YouTubeAddError::CourseIdTaken,
                error => YouTubeAddError::Library(error.into()),
            })?;
        Ok(CourseImportReceipt {
            course_id: saved.course_id,
            course_version: saved.course_version,
            title: saved.title,
            verified_resource_count: videos.len(),
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
