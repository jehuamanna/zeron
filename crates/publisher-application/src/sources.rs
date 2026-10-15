use std::path::Path;

use course_format::ResourceKind;
use course_publisher::{DraftCourse, DraftResource, MediaFinding, inspect_media};

const MAX_FILE_NAME_BYTES: usize = 120;

/// Display-safe facts about a resource's backend-owned source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    /// Final file-name component only; never a directory path.
    pub file_name: String,
    /// Current size when the source is an existing regular file.
    pub byte_length: Option<u64>,
}

/// Inspect a resource's source without exposing its location.
#[must_use]
pub fn source_info(resource: &DraftResource) -> SourceInfo {
    let file_name = resource
        .source_path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let byte_length = std::fs::metadata(&resource.source_path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len());
    SourceInfo {
        file_name,
        byte_length,
    }
}

/// Media compatibility findings for one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceMediaFindings {
    /// Stable resource ID.
    pub resource_id: String,
    /// Draft field path of the resource, for focusing the editor.
    pub field: String,
    /// Display-safe file name.
    pub file_name: String,
    /// Non-empty list of findings.
    pub findings: Vec<MediaFinding>,
}

/// Inspect every resource of a draft snapshot; resources without findings are omitted.
///
/// Inspection reads only container headers, so callers may run it on a
/// snapshot without holding draft storage locks.
#[must_use]
pub fn inspect_draft_media(draft: &DraftCourse) -> Vec<ResourceMediaFindings> {
    let mut results = Vec::new();
    for (module_index, module) in draft.modules.iter().enumerate() {
        for (lesson_index, lesson) in module.lessons.iter().enumerate() {
            for (resource_index, resource) in lesson.resources.iter().enumerate() {
                let findings = inspect_media(&resource.source_path, &resource.media_type).findings;
                if !findings.is_empty() {
                    results.push(ResourceMediaFindings {
                        resource_id: resource.id.clone(),
                        field: format!(
                            "modules[{module_index}].lessons[{lesson_index}].resources[{resource_index}]"
                        ),
                        file_name: source_info(resource).file_name,
                        findings,
                    });
                }
            }
        }
    }
    results
}

/// Infer the logical kind and declared media type from a file extension.
///
/// The declaration is confirmed later by packaging-time inspection.
#[must_use]
pub fn classify(path: &Path) -> (ResourceKind, &'static str) {
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "mp4" | "m4v" => (ResourceKind::Video, "video/mp4"),
        "webm" => (ResourceKind::Video, "video/webm"),
        "mov" => (ResourceKind::Video, "video/quicktime"),
        "mkv" => (ResourceKind::Video, "video/x-matroska"),
        "mp3" => (ResourceKind::Audio, "audio/mpeg"),
        "m4a" => (ResourceKind::Audio, "audio/mp4"),
        "aac" => (ResourceKind::Audio, "audio/aac"),
        "ogg" | "oga" | "opus" => (ResourceKind::Audio, "audio/ogg"),
        "wav" => (ResourceKind::Audio, "audio/wav"),
        "flac" => (ResourceKind::Audio, "audio/flac"),
        "pdf" => (ResourceKind::Pdf, "application/pdf"),
        "md" | "markdown" => (ResourceKind::Markdown, "text/markdown"),
        "txt" => (ResourceKind::Text, "text/plain"),
        "vtt" => (ResourceKind::Subtitle, "text/vtt"),
        "srt" => (ResourceKind::Subtitle, "application/x-subrip"),
        "png" => (ResourceKind::Image, "image/png"),
        "jpg" | "jpeg" => (ResourceKind::Image, "image/jpeg"),
        "webp" => (ResourceKind::Image, "image/webp"),
        "gif" => (ResourceKind::Image, "image/gif"),
        "zip" => (ResourceKind::SourceArchive, "application/zip"),
        _ => (ResourceKind::Other, "application/octet-stream"),
    }
}

/// Build a portable package path segment from an untrusted file name.
#[must_use]
pub fn package_file_name(file_name: &str) -> String {
    let mut name: String = file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    name = name.trim_start_matches('.').to_owned();
    if name.len() > MAX_FILE_NAME_BYTES {
        let keep_extension = name
            .rfind('.')
            .filter(|dot| name.len() - dot <= 16)
            .map(|dot| name[dot..].to_owned())
            .unwrap_or_default();
        name.truncate(MAX_FILE_NAME_BYTES - keep_extension.len());
        name.push_str(&keep_extension);
    }
    let name = name.trim_end_matches(['.', ' ']);
    if name.is_empty() {
        return "file".to_owned();
    }
    if is_windows_reserved(name.split('.').next().unwrap_or_default()) {
        format!("file-{name}")
    } else {
        name.to_owned()
    }
}

fn is_windows_reserved(stem: &str) -> bool {
    let stem = stem.to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_file_names_are_portable_segments() {
        for (input, expected) in [
            ("Lecture 1: Intro.mp4", "Lecture_1__Intro.mp4"),
            ("résumé.pdf", "r_sum_.pdf"),
            ("..hidden.txt", "hidden.txt"),
            ("trailing.", "trailing"),
            ("CON.mp4", "file-CON.mp4"),
            ("com1.txt", "file-com1.txt"),
            ("...", "file"),
            (".mp4", "mp4"),
            ("a\\b/c.zip", "a_b_c.zip"),
        ] {
            assert_eq!(package_file_name(input), expected, "{input}");
        }
        let long = format!("{}.mp4", "x".repeat(300));
        let shortened = package_file_name(&long);
        assert_eq!(shortened.len(), MAX_FILE_NAME_BYTES);
        assert_eq!(&shortened[shortened.len() - 4..], ".mp4");
    }

    #[test]
    fn classifies_by_lowercase_extension() {
        assert_eq!(
            classify(Path::new("talk.MP4")),
            (ResourceKind::Video, "video/mp4")
        );
        assert_eq!(
            classify(Path::new("notes.vtt")),
            (ResourceKind::Subtitle, "text/vtt")
        );
        assert_eq!(
            classify(Path::new("code.zip")),
            (ResourceKind::SourceArchive, "application/zip")
        );
        assert_eq!(
            classify(Path::new("README")),
            (ResourceKind::Other, "application/octet-stream")
        );
    }
}
