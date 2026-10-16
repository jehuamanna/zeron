//! Rules for moving a learner's annotations to a new version of a course.
//!
//! A new version can rename, replace, or remove lessons. Annotations are
//! keyed by stable lesson and resource IDs, so those the new version no
//! longer has would be lost from view. These rules suggest where they belong
//! now; nothing moves until the learner accepts a suggestion.

use std::collections::BTreeMap;

use crate::Anchor;

/// A resource of a lesson, identified by its content digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineResource {
    /// Stable resource ID.
    pub id: String,
    /// Lowercase hexadecimal SHA-256 of the resource bytes.
    pub sha256: String,
}

/// A lesson of one course version, reduced to what matching needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineLesson {
    /// Stable lesson ID.
    pub id: String,
    /// Display title.
    pub title: String,
    /// The lesson's resources.
    pub resources: Vec<OutlineResource>,
}

/// Why a lesson of the new version was suggested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchReason {
    /// The lesson kept its ID, but some resources its notes point at changed.
    SameLesson,
    /// The new lesson has at least one file with identical contents.
    SameContent,
    /// The new lesson is the only one with the same title.
    SameTitle,
}

impl MatchReason {
    /// Stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SameLesson => "same_lesson",
            Self::SameContent => "same_content",
            Self::SameTitle => "same_title",
        }
    }
}

/// Suggest where an old lesson's annotations belong in a new version: the
/// lesson sharing the most files with identical contents, or else the only
/// lesson with the same title (ignoring case and spacing). Ties between
/// lessons with the same number of shared files have no suggestion.
#[must_use]
pub fn suggest_lesson(old: &OutlineLesson, new: &[OutlineLesson]) -> Option<(String, MatchReason)> {
    let mut best: Option<(&OutlineLesson, usize)> = None;
    let mut tied = false;
    for candidate in new {
        let shared = old
            .resources
            .iter()
            .filter(|resource| {
                candidate
                    .resources
                    .iter()
                    .any(|other| other.sha256 == resource.sha256)
            })
            .count();
        if shared == 0 {
            continue;
        }
        match best {
            Some((_, most)) if shared < most => {}
            Some((_, most)) if shared == most => tied = true,
            _ => {
                best = Some((candidate, shared));
                tied = false;
            }
        }
    }
    if let Some((lesson, _)) = best.filter(|_| !tied) {
        return Some((lesson.id.clone(), MatchReason::SameContent));
    }
    let title = normalized_title(&old.title);
    let mut same_title = new
        .iter()
        .filter(|candidate| normalized_title(&candidate.title) == title);
    match (same_title.next(), same_title.next()) {
        (Some(lesson), None) if !title.is_empty() => {
            Some((lesson.id.clone(), MatchReason::SameTitle))
        }
        _ => None,
    }
}

/// Map the old lesson's resource IDs to the new lesson's: an unchanged ID
/// whose contents are unchanged maps to itself, and otherwise a resource maps
/// to a new one with identical contents. Resources whose contents are gone
/// have no entry.
#[must_use]
pub fn resource_map(old: &OutlineLesson, new: &OutlineLesson) -> BTreeMap<String, String> {
    old.resources
        .iter()
        .filter_map(|resource| {
            let same_id = new
                .resources
                .iter()
                .find(|other| other.id == resource.id && other.sha256 == resource.sha256);
            same_id
                .or_else(|| {
                    new.resources
                        .iter()
                        .find(|other| other.sha256 == resource.sha256)
                })
                .map(|other| (resource.id.clone(), other.id.clone()))
        })
        .collect()
}

/// Point an anchor at the mapped resource. An anchor whose resource has no
/// mapping, because its file changed, points at the whole lesson instead,
/// since its timestamp or page may no longer mean the same place. Returns
/// the anchor and whether it had to be widened.
#[must_use]
pub fn remap_anchor(anchor: &Anchor, map: &BTreeMap<String, String>) -> (Anchor, bool) {
    match anchor {
        Anchor::Lesson {} => (Anchor::Lesson {}, false),
        Anchor::VideoTime {
            resource_id,
            seconds,
        } => map
            .get(resource_id)
            .map_or((Anchor::Lesson {}, true), |mapped| {
                (
                    Anchor::VideoTime {
                        resource_id: mapped.clone(),
                        seconds: *seconds,
                    },
                    false,
                )
            }),
        Anchor::PdfPage {
            resource_id,
            page,
            region,
        } => map
            .get(resource_id)
            .map_or((Anchor::Lesson {}, true), |mapped| {
                (
                    Anchor::PdfPage {
                        resource_id: mapped.clone(),
                        page: *page,
                        region: *region,
                    },
                    false,
                )
            }),
    }
}

fn normalized_title(title: &str) -> String {
    title
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(id: &str, title: &str, resources: &[(&str, &str)]) -> OutlineLesson {
        OutlineLesson {
            id: id.to_owned(),
            title: title.to_owned(),
            resources: resources
                .iter()
                .map(|(id, sha)| OutlineResource {
                    id: (*id).to_owned(),
                    sha256: (*sha).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn identical_files_win_over_titles() {
        let old = lesson("l_old", "Intro", &[("r1", "aa"), ("r2", "bb")]);
        let new = [
            lesson("l_title", "intro", &[("x", "zz")]),
            lesson("l_one", "Welcome", &[("y", "aa")]),
            lesson("l_two", "Getting started", &[("p", "aa"), ("q", "bb")]),
        ];
        assert_eq!(
            suggest_lesson(&old, &new),
            Some(("l_two".to_owned(), MatchReason::SameContent))
        );
    }

    #[test]
    fn a_unique_title_is_the_fallback_and_ties_suggest_nothing() {
        let old = lesson("l_old", "  Design   Tinder ", &[("r1", "aa")]);
        let new = [
            lesson("l_a", "design tinder", &[("x", "zz")]),
            lesson("l_b", "Design Uber", &[("y", "yy")]),
        ];
        assert_eq!(
            suggest_lesson(&old, &new),
            Some(("l_a".to_owned(), MatchReason::SameTitle))
        );
        let duplicate_titles = [
            lesson("l_a", "Design Tinder", &[]),
            lesson("l_b", "design tinder", &[]),
        ];
        assert_eq!(suggest_lesson(&old, &duplicate_titles), None);
        let tied_content = [
            lesson("l_a", "A", &[("x", "aa")]),
            lesson("l_b", "B", &[("y", "aa")]),
        ];
        assert_eq!(suggest_lesson(&old, &tied_content), None);
        assert_eq!(
            suggest_lesson(&lesson("l", "", &[]), &[lesson("m", "", &[])]),
            None
        );
    }

    #[test]
    fn resources_map_by_id_then_contents_and_anchors_widen_when_files_changed() {
        let old = lesson(
            "l",
            "L",
            &[("video", "aa"), ("slides", "bb"), ("gone", "cc")],
        );
        let new = lesson(
            "l",
            "L",
            &[("video", "aa"), ("deck", "bb"), ("other", "dd")],
        );
        let map = resource_map(&old, &new);
        assert_eq!(map.get("video").map(String::as_str), Some("video"));
        assert_eq!(map.get("slides").map(String::as_str), Some("deck"));
        assert!(!map.contains_key("gone"));

        let at = Anchor::VideoTime {
            resource_id: "video".to_owned(),
            seconds: 12.5,
        };
        assert_eq!(remap_anchor(&at, &map), (at.clone(), false));
        let page = Anchor::PdfPage {
            resource_id: "slides".to_owned(),
            page: 3,
            region: None,
        };
        assert_eq!(
            remap_anchor(&page, &map),
            (
                Anchor::PdfPage {
                    resource_id: "deck".to_owned(),
                    page: 3,
                    region: None
                },
                false
            )
        );
        let lost = Anchor::VideoTime {
            resource_id: "gone".to_owned(),
            seconds: 4.0,
        };
        assert_eq!(remap_anchor(&lost, &map), (Anchor::Lesson {}, true));
        assert_eq!(
            remap_anchor(&Anchor::Lesson {}, &map),
            (Anchor::Lesson {}, false)
        );
    }
}
