//! Excalidraw drawings kept with a lesson, and the images placed in them.

use serde_json::Value;
use thiserror::Error;

/// Largest stored scene document.
pub const MAX_SCENE_BYTES: usize = 2 * 1024 * 1024;
/// Largest stored image, as a base64 data URL.
pub const MAX_FILE_DATA_URL_BYTES: usize = 10 * 1024 * 1024;
/// Most images one drawing may hold.
pub const MAX_FILES_PER_DRAWING: usize = 64;
/// Image types a drawing may embed. SVG is refused because it can carry script.
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Why a drawing or image was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DrawingError {
    /// The scene is not an Excalidraw scene within the limits.
    #[error("the drawing is not valid: {0}")]
    InvalidScene(&'static str),
    /// An embedded image is not an accepted image within the limits.
    #[error("an image in the drawing is not valid: {0}")]
    InvalidFile(&'static str),
}

/// A validated Excalidraw scene: `{ "elements": [...], ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// The scene as stored.
    pub json: String,
}

impl Scene {
    /// Accept a JSON object with an `elements` array, within [`MAX_SCENE_BYTES`].
    ///
    /// # Errors
    /// Returns [`DrawingError::InvalidScene`].
    pub fn parse(json: &str) -> Result<Self, DrawingError> {
        if json.len() > MAX_SCENE_BYTES {
            return Err(DrawingError::InvalidScene("it is too large"));
        }
        let value: Value =
            serde_json::from_str(json).map_err(|_| DrawingError::InvalidScene("it is not JSON"))?;
        if !value.get("elements").is_some_and(Value::is_array) {
            return Err(DrawingError::InvalidScene("it has no elements"));
        }
        Ok(Self {
            json: json.to_owned(),
        })
    }
}

/// An image embedded in a drawing, as Excalidraw stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingFile {
    /// Excalidraw file ID.
    pub id: String,
    /// Image media type.
    pub mime_type: String,
    /// `data:<mime>;base64,<data>`.
    pub data_url: String,
}

impl DrawingFile {
    /// Accept a PNG, JPEG, WebP, or GIF data URL whose declared type matches,
    /// with a short ID and valid base64, within [`MAX_FILE_DATA_URL_BYTES`].
    ///
    /// # Errors
    /// Returns [`DrawingError::InvalidFile`].
    pub fn new(id: &str, mime_type: &str, data_url: &str) -> Result<Self, DrawingError> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(DrawingError::InvalidFile("its ID is not valid"));
        }
        if !IMAGE_TYPES.contains(&mime_type) {
            return Err(DrawingError::InvalidFile(
                "only PNG, JPEG, WebP, and GIF images are kept",
            ));
        }
        if data_url.len() > MAX_FILE_DATA_URL_BYTES {
            return Err(DrawingError::InvalidFile("it is too large"));
        }
        let data = data_url
            .strip_prefix("data:")
            .and_then(|rest| rest.strip_prefix(mime_type))
            .and_then(|rest| rest.strip_prefix(";base64,"))
            .ok_or(DrawingError::InvalidFile(
                "its data does not match its type",
            ))?;
        let base64 = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=');
        if data.is_empty() || data.len() % 4 != 0 || !data.bytes().all(base64) {
            return Err(DrawingError::InvalidFile("its data is not base64"));
        }
        Ok(Self {
            id: id.to_owned(),
            mime_type: mime_type.to_owned(),
            data_url: data_url.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_need_elements_and_a_bounded_size() {
        assert_eq!(
            Scene::parse(r#"{"elements":[],"appState":{}}"#)
                .unwrap()
                .json,
            r#"{"elements":[],"appState":{}}"#
        );
        for bad in ["nope", "[]", r#"{"elements":{}}"#, r#"{"appState":{}}"#] {
            assert!(Scene::parse(bad).is_err(), "{bad}");
        }
        let huge = format!(
            r#"{{"elements":[],"pad":"{}"}}"#,
            "x".repeat(MAX_SCENE_BYTES)
        );
        assert!(Scene::parse(&huge).is_err());
    }

    #[test]
    fn images_must_be_raster_data_urls_matching_their_type() {
        DrawingFile::new("file_1", "image/png", "data:image/png;base64,iVBORw0KGgo=").unwrap();
        let bad = [
            ("", "image/png", "data:image/png;base64,AAAA"),
            ("../x", "image/png", "data:image/png;base64,AAAA"),
            ("f", "image/svg+xml", "data:image/svg+xml;base64,PHN2Zz4="),
            ("f", "image/png", "data:image/jpeg;base64,AAAA"),
            ("f", "image/png", "data:image/png,rawdata"),
            ("f", "image/png", "data:image/png;base64,AA A"),
            ("f", "image/png", "data:image/png;base64,AAA"),
            ("f", "image/png", "javascript:alert(1)"),
        ];
        for (id, mime, url) in bad {
            assert!(
                DrawingFile::new(id, mime, url).is_err(),
                "{id} {mime} {url}"
            );
        }
        let huge = format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_FILE_DATA_URL_BYTES)
        );
        assert!(DrawingFile::new("f", "image/png", &huge).is_err());
    }
}
