//! Learner notes and the places in a lesson they point at.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Largest stored note document.
pub const MAX_NOTE_BYTES: usize = 256 * 1024;
/// Longest video a timestamp may point into.
const MAX_SECONDS: f64 = 48.0 * 3600.0;
/// Highest PDF page number accepted.
const MAX_PAGE: u32 = 100_000;
/// Deepest nesting accepted in a note document.
const MAX_DEPTH: usize = 64;
/// Longest quoted passage a text highlight keeps.
pub const MAX_QUOTE_CHARS: usize = 4_000;
/// Most rectangles one text highlight may cover.
pub const MAX_HIGHLIGHT_RECTS: usize = 256;

/// A rectangle on a PDF page, in fractions of the page (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// Colours a learner can highlight PDF text with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HighlightColor {
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Blue.
    Blue,
    /// Pink.
    Pink,
    /// Orange.
    Orange,
    /// Purple.
    Purple,
}

/// How a text highlight is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HighlightStyle {
    /// A coloured background.
    Highlight,
    /// A coloured line under the text.
    Underline,
    /// A coloured line through the text.
    Strike,
}

/// Selected text on a PDF page: what it says and where it lies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextHighlight {
    /// Colour.
    pub color: HighlightColor,
    /// Drawing style.
    pub style: HighlightStyle,
    /// The selected text.
    pub quote: String,
    /// Line boxes of the selection, in page fractions.
    pub rects: Vec<Region>,
}

impl Region {
    /// Whether the rectangle lies inside the page and has a positive size.
    #[must_use]
    pub fn fits_page(&self) -> bool {
        let inside = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
        [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(inside)
            && self.width > 0.0
            && self.height > 0.0
            && self.x + self.width <= 1.0 + f64::EPSILON
            && self.y + self.height <= 1.0 + f64::EPSILON
    }
}

/// Where in a lesson a note points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Anchor {
    /// The lesson as a whole.
    Lesson {},
    /// A moment in a video resource of the lesson.
    #[serde(rename_all = "camelCase")]
    VideoTime {
        /// Video resource ID.
        resource_id: String,
        /// Seconds from the start.
        seconds: f64,
    },
    /// A page, optionally a region of it, in a PDF resource of the lesson.
    #[serde(rename_all = "camelCase")]
    PdfPage {
        /// PDF resource ID.
        resource_id: String,
        /// One-based page number.
        page: u32,
        /// Highlighted part of the page.
        region: Option<Region>,
        /// Highlighted text on the page; absent in notes made before text highlights.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        highlight: Option<Box<TextHighlight>>,
    },
}

/// Why a note or anchor was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NoteError {
    /// A timestamp, page, or region is out of range.
    #[error("the note's place in the lesson is not valid: {0}")]
    InvalidAnchor(&'static str),
    /// The note document is not a Tiptap document within the limits.
    #[error("the note is not a valid document: {0}")]
    InvalidBody(&'static str),
}

impl Anchor {
    /// Check ranges: a finite timestamp within 48 hours, a page from 1, and a
    /// region inside the page with a positive size.
    ///
    /// # Errors
    /// Returns [`NoteError::InvalidAnchor`].
    pub fn validate(&self) -> Result<(), NoteError> {
        match self {
            Self::Lesson {} => Ok(()),
            Self::VideoTime { seconds, .. } => {
                if seconds.is_finite() && (0.0..=MAX_SECONDS).contains(seconds) {
                    Ok(())
                } else {
                    Err(NoteError::InvalidAnchor("the timestamp is out of range"))
                }
            }
            Self::PdfPage {
                page,
                region,
                highlight,
                ..
            } => {
                if !(1..=MAX_PAGE).contains(page) {
                    return Err(NoteError::InvalidAnchor("the page number is out of range"));
                }
                if region.is_some_and(|region| !region.fits_page()) {
                    return Err(NoteError::InvalidAnchor("the region is outside the page"));
                }
                if let Some(highlight) = highlight {
                    let quote = highlight.quote.trim();
                    if quote.is_empty() || quote.chars().count() > MAX_QUOTE_CHARS {
                        return Err(NoteError::InvalidAnchor(
                            "the highlighted text is empty or too long",
                        ));
                    }
                    if highlight.rects.is_empty()
                        || highlight.rects.len() > MAX_HIGHLIGHT_RECTS
                        || !highlight.rects.iter().all(Region::fits_page)
                    {
                        return Err(NoteError::InvalidAnchor(
                            "the highlighted text is not on the page",
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    /// Parse a stored or submitted anchor; unknown kinds and fields are refused.
    ///
    /// # Errors
    /// Returns [`NoteError::InvalidAnchor`].
    pub fn from_json(json: &str) -> Result<Self, NoteError> {
        serde_json::from_str(json).map_err(|_| NoteError::InvalidAnchor("it could not be read"))
    }

    /// The anchor as stored.
    #[must_use]
    pub fn to_json(&self) -> String {
        // Serializing this plain enum cannot fail.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The lesson resource the anchor points into, if any.
    #[must_use]
    pub fn resource_id(&self) -> Option<&str> {
        match self {
            Self::Lesson {} => None,
            Self::VideoTime { resource_id, .. } | Self::PdfPage { resource_id, .. } => {
                Some(resource_id)
            }
        }
    }
}

/// A validated Tiptap (`ProseMirror`) document and its plain text.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteBody {
    /// The document as stored.
    pub json: String,
    /// Text of every text node, for search and previews.
    pub text: String,
}

impl NoteBody {
    /// Parse and check a document: valid JSON within [`MAX_NOTE_BYTES`], a
    /// top-level `doc` node, bounded nesting, and text only in string `text`
    /// fields. Plain text is collected for search.
    ///
    /// # Errors
    /// Returns [`NoteError::InvalidBody`].
    pub fn parse(json: &str) -> Result<Self, NoteError> {
        if json.len() > MAX_NOTE_BYTES {
            return Err(NoteError::InvalidBody("the note is too large"));
        }
        let value: Value =
            serde_json::from_str(json).map_err(|_| NoteError::InvalidBody("it is not JSON"))?;
        if value.get("type").and_then(Value::as_str) != Some("doc") {
            return Err(NoteError::InvalidBody("it is not a document"));
        }
        let mut text = String::new();
        collect_text(&value, 0, &mut text)?;
        Ok(Self {
            json: json.to_owned(),
            text: text.trim().to_owned(),
        })
    }
}

fn collect_text(node: &Value, depth: usize, text: &mut String) -> Result<(), NoteError> {
    if depth > MAX_DEPTH {
        return Err(NoteError::InvalidBody("it is nested too deeply"));
    }
    let Some(object) = node.as_object() else {
        return Err(NoteError::InvalidBody("a node is not an object"));
    };
    if let Some(value) = object.get("text") {
        let Some(value) = value.as_str() else {
            return Err(NoteError::InvalidBody("text is not a string"));
        };
        text.push_str(value);
    }
    if let Some(children) = object.get("content") {
        let Some(children) = children.as_array() else {
            return Err(NoteError::InvalidBody("content is not a list"));
        };
        for child in children {
            collect_text(child, depth + 1, text)?;
        }
        if matches!(
            object.get("type").and_then(Value::as_str),
            Some("paragraph" | "heading" | "codeBlock" | "listItem" | "blockquote")
        ) {
            text.push('\n');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_parse_strictly_and_validate_ranges() {
        let video: Anchor =
            serde_json::from_str(r#"{"kind":"videoTime","resourceId":"r1","seconds":75.5}"#)
                .unwrap();
        assert_eq!(video.resource_id(), Some("r1"));
        video.validate().unwrap();
        let pdf: Anchor = serde_json::from_str(
            r#"{"kind":"pdfPage","resourceId":"p1","page":3,"region":{"x":0.1,"y":0.2,"width":0.5,"height":0.3}}"#,
        )
        .unwrap();
        pdf.validate().unwrap();
        let lesson: Anchor = serde_json::from_str(r#"{"kind":"lesson"}"#).unwrap();
        assert_eq!(Anchor::from_json(&pdf.to_json()).unwrap(), pdf);
        assert!(Anchor::from_json("{}").is_err());
        assert_eq!(lesson.resource_id(), None);
        assert!(serde_json::from_str::<Anchor>(r#"{"kind":"lesson","path":"/etc"}"#).is_err());
        assert!(serde_json::from_str::<Anchor>(r#"{"kind":"file","resourceId":"x"}"#).is_err());

        let bad = [
            Anchor::VideoTime {
                resource_id: "r".into(),
                seconds: -1.0,
            },
            Anchor::VideoTime {
                resource_id: "r".into(),
                seconds: f64::NAN,
            },
            Anchor::PdfPage {
                resource_id: "p".into(),
                page: 0,
                region: None,
                highlight: None,
            },
            Anchor::PdfPage {
                resource_id: "p".into(),
                page: 1,
                region: Some(Region {
                    x: 0.8,
                    y: 0.0,
                    width: 0.5,
                    height: 0.1,
                }),
                highlight: None,
            },
            Anchor::PdfPage {
                resource_id: "p".into(),
                page: 1,
                region: Some(Region {
                    x: 0.1,
                    y: 0.1,
                    width: 0.0,
                    height: 0.1,
                }),
                highlight: None,
            },
        ];
        for anchor in bad {
            assert!(
                matches!(anchor.validate(), Err(NoteError::InvalidAnchor(_))),
                "{anchor:?}"
            );
        }
    }

    #[test]
    fn text_highlights_round_trip_and_older_page_notes_still_read() {
        let older: Anchor =
            serde_json::from_str(r#"{"kind":"pdfPage","resourceId":"p1","page":3,"region":null}"#)
                .unwrap();
        assert_eq!(
            older,
            Anchor::PdfPage {
                resource_id: "p1".into(),
                page: 3,
                region: None,
                highlight: None
            }
        );
        // Notes without a highlight are stored exactly as before.
        assert!(!older.to_json().contains("highlight"));

        let line = Region {
            x: 0.1,
            y: 0.2,
            width: 0.6,
            height: 0.02,
        };
        let highlighted = |quote: &str, rects: Vec<Region>| Anchor::PdfPage {
            resource_id: "p1".into(),
            page: 43,
            region: None,
            highlight: Some(Box::new(TextHighlight {
                color: HighlightColor::Pink,
                style: HighlightStyle::Underline,
                quote: quote.into(),
                rects,
            })),
        };
        let good = highlighted("improve consistency across sessions", vec![line, line]);
        good.validate().unwrap();
        let json = good.to_json();
        assert!(json.contains(r#""color":"pink""#) && json.contains(r#""style":"underline""#));
        assert_eq!(Anchor::from_json(&json).unwrap(), good);
        assert!(
            Anchor::from_json(&json.replace(r#""style""#, r#""weight":1,"style""#)).is_err(),
            "unknown highlight fields are refused"
        );

        let outside = Region { x: 0.9, ..line };
        for bad in [
            highlighted("  ", vec![line]),
            highlighted(&"x".repeat(MAX_QUOTE_CHARS + 1), vec![line]),
            highlighted("text", vec![]),
            highlighted("text", vec![outside]),
            highlighted("text", vec![line; MAX_HIGHLIGHT_RECTS + 1]),
        ] {
            assert!(
                matches!(bad.validate(), Err(NoteError::InvalidAnchor(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn note_bodies_are_tiptap_documents_with_extractable_text() {
        let body = NoteBody::parse(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Load "},{"type":"text","marks":[{"type":"bold"}],"text":"balancers"}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"round robin"}]}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(body.text, "Load balancers\nround robin");
        assert_eq!(NoteBody::parse(r#"{"type":"doc"}"#).unwrap().text, "");
        for bad in [
            "not json",
            r#"{"type":"paragraph"}"#,
            r#"{"type":"doc","content":"text"}"#,
            r#"{"type":"doc","content":[{"type":"text","text":5}]}"#,
            r#"{"type":"doc","content":[1]}"#,
        ] {
            assert!(
                matches!(NoteBody::parse(bad), Err(NoteError::InvalidBody(_))),
                "{bad}"
            );
        }
        let deep = format!(
            "{}{}",
            r#"{"type":"doc","content":["#.repeat(70),
            "]}".repeat(70)
        );
        assert!(NoteBody::parse(&deep).is_err());
        let huge = format!(
            r#"{{"type":"doc","content":[{{"type":"text","text":"{}"}}]}}"#,
            "x".repeat(MAX_NOTE_BYTES)
        );
        assert!(NoteBody::parse(&huge).is_err());
    }
}
