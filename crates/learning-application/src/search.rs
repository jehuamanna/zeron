//! Offline search across courses, lessons, notes, transcripts, and lesson
//! text, and indexing the text of course files once their bytes are verified.

use std::collections::HashMap;

use course_format::{CourseManifest, ResourceKind};
use learning_domain::{fts_query, parse_transcript, text_passages, transcript_passages};
use learning_storage::SearchEntry;

use crate::{CourseLibrary, DocumentSource, LibraryError, documents::transcript_for};

/// Most results returned for one query.
const MAX_RESULTS: usize = 50;

/// What a search result points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    /// A course's title, description, publisher, tags, or modules.
    Course,
    /// A lesson title.
    Lesson,
    /// A learner note.
    Note,
    /// A passage of a media transcript.
    Transcript,
    /// A passage of a Markdown or text lesson resource.
    Text,
}

/// One search match, ready to show and open.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// What matched.
    pub kind: SearchKind,
    /// Course of the match.
    pub course_id: String,
    /// Its title.
    pub course_title: String,
    /// Lesson of the match, if not the course itself.
    pub lesson_id: Option<String>,
    /// Its title.
    pub lesson_title: Option<String>,
    /// The note, media (for transcripts), or resource the result opens.
    pub source_id: String,
    /// Seconds into the media, for transcripts.
    pub position: Option<f64>,
    /// Matching excerpt; matched terms are wrapped in U+E000 and U+E001.
    pub snippet: String,
}

impl CourseLibrary {
    /// Best matches for what the learner typed. Every word must match; the
    /// last may be the start of a word.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn search_learning(&self, input: &str) -> Result<Vec<SearchResult>, LibraryError> {
        let Some(query) = fts_query(input) else {
            return Ok(Vec::new());
        };
        let hits = self.storage.search(&query, MAX_RESULTS)?;
        let mut manifests: HashMap<String, Option<CourseManifest>> = HashMap::new();
        let mut results = Vec::with_capacity(hits.len());
        for hit in hits {
            let kind = match hit.kind.as_str() {
                "course" => SearchKind::Course,
                "lesson" => SearchKind::Lesson,
                "note" => SearchKind::Note,
                "transcript" => SearchKind::Transcript,
                "text" => SearchKind::Text,
                _ => continue,
            };
            if !manifests.contains_key(&hit.course_id) {
                manifests.insert(hit.course_id.clone(), self.manifest(&hit.course_id)?);
            }
            let Some(Some(manifest)) = manifests.get(&hit.course_id) else {
                continue;
            };
            let lesson_title = manifest
                .modules
                .iter()
                .flat_map(|module| &module.lessons)
                .find(|lesson| lesson.id == hit.lesson_id)
                .map(|lesson| lesson.title.clone());
            results.push(SearchResult {
                kind,
                course_title: manifest.title.clone(),
                lesson_id: (!hit.lesson_id.is_empty()).then_some(hit.lesson_id),
                lesson_title,
                course_id: hit.course_id,
                source_id: hit.source_id,
                position: hit.position,
                snippet: hit.snippet,
            });
        }
        Ok(results)
    }

    /// Index the text of a transcript, subtitle, Markdown, or text resource
    /// from bytes already verified against the course. Does nothing when it
    /// is already indexed at its digest or is another kind of resource.
    /// Returns whether the index changed.
    ///
    /// # Errors
    ///
    /// Returns a storage failure. Unreadable text indexes nothing.
    pub fn index_text_resource(
        &mut self,
        course_id: &str,
        resource_id: &str,
        bytes: &[u8],
    ) -> Result<bool, LibraryError> {
        let Some(manifest) = self.manifest(course_id)? else {
            return Ok(false);
        };
        let Some((lesson, resource)) = manifest
            .modules
            .iter()
            .flat_map(|module| &module.lessons)
            .find_map(|lesson| {
                lesson
                    .resources
                    .iter()
                    .find(|resource| resource.id == resource_id)
                    .map(|resource| (lesson, resource))
            })
        else {
            return Ok(false);
        };
        if self
            .storage
            .is_resource_indexed(course_id, resource_id, &resource.sha256)?
        {
            return Ok(false);
        }
        let entry = |kind, source_id: &str, position, body| SearchEntry {
            lesson_id: lesson.id.clone(),
            kind,
            source_id: source_id.to_owned(),
            position,
            body,
        };
        let entries: Vec<SearchEntry> = match resource.kind {
            ResourceKind::Transcript | ResourceKind::Subtitle => {
                // Indexed under the media it belongs to, so a result can play it.
                let media = lesson.resources.iter().find(|media| {
                    transcript_for(lesson, media).is_some_and(|found| found.id == resource.id)
                });
                match (media, parse_transcript(bytes)) {
                    (Some(media), Ok(cues)) => transcript_passages(&cues)
                        .into_iter()
                        .map(|(start, text)| entry("transcript", &media.id, Some(start), text))
                        .collect(),
                    _ => Vec::new(),
                }
            }
            ResourceKind::Markdown | ResourceKind::Text => std::str::from_utf8(bytes)
                .map(|text| {
                    text_passages(text)
                        .into_iter()
                        .map(|passage| entry("text", resource_id, None, passage))
                        .collect()
                })
                .unwrap_or_default(),
            _ => return Ok(false),
        };
        self.storage
            .index_resource(course_id, resource_id, &resource.sha256, &entries)?;
        Ok(true)
    }

    /// Text resources of local courses not yet indexed at their digest, with
    /// where to read them. Torrent courses are indexed as their text is read,
    /// so searching never starts a download.
    ///
    /// # Errors
    ///
    /// Returns a storage failure.
    pub fn unindexed_local_texts(
        &self,
    ) -> Result<Vec<(String, String, DocumentSource)>, LibraryError> {
        let mut pending = Vec::new();
        for course in self.storage.list_courses()? {
            if self.torrent_info_hash(&course.course_id)?.is_some() {
                continue;
            }
            let Some(manifest) = self.manifest(&course.course_id)? else {
                continue;
            };
            for resource in manifest
                .modules
                .iter()
                .flat_map(|module| &module.lessons)
                .flat_map(|lesson| &lesson.resources)
            {
                let textual = matches!(
                    resource.kind,
                    ResourceKind::Transcript
                        | ResourceKind::Subtitle
                        | ResourceKind::Markdown
                        | ResourceKind::Text
                );
                if !textual
                    || self.storage.is_resource_indexed(
                        &course.course_id,
                        &resource.id,
                        &resource.sha256,
                    )?
                {
                    continue;
                }
                if let Some(local) = self.local_resource(&course.course_id, &resource.id)? {
                    pending.push((
                        course.course_id.clone(),
                        resource.id.clone(),
                        DocumentSource::Local {
                            path: local.path,
                            sha256: resource.sha256.clone(),
                            byte_length: local.byte_length,
                        },
                    ));
                }
            }
        }
        Ok(pending)
    }
}
