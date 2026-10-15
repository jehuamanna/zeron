# Phase 6: Offline search

Status: complete. The Library's search box finds courses, lessons, the learner's notes, transcript passages, and the text of Markdown and text lessons, without a network. Results show the matched words highlighted and open the place they came from: a transcript match plays its video from that moment, a text match opens the reader, and a note or lesson match opens its lesson.

## Delivered

- **Storage (learner schema v11):** one FTS5 table, `learning_search` (`unicode61`, diacritics removed), with columns `title` and `body` searched and the rest unindexed. A companion table, `indexed_resources`, records which course file was indexed at which SHA-256.
  - **Course and lesson rows** are written with each import: course title, then description, publisher, tags and module titles; one row per lesson with its module title. A new version of a course replaces them and drops the text taken from its files, since those may have changed.
  - **Note rows** are kept in step by triggers on `notes` (insert, body update, delete), so a note can never be saved without being searchable.
  - **The migration** from v10 builds the table and indexes every stored course and note.
  - `search` ranks with `bm25`, titles weighted five times the body, and returns snippets with matched terms wrapped in U+E000 and U+E001.
- **Domain (`learning-domain::search`):**
  - `fts_query` turns what the learner typed into a query: each word (letters and digits) quoted, at most 12 words, the last one as a prefix so results appear while typing. No input can form FTS5 syntax.
  - `transcript_passages` groups cues into passages of up to 30 seconds or about 600 characters, each starting at its first cue. `text_passages` splits at blank lines and joins short paragraphs up to about 600 characters.
- **Application:**
  - `CourseLibrary::search_learning` returns at most 50 results with course and lesson titles.
  - `index_text_resource` indexes a transcript, subtitle, Markdown or text resource from bytes already verified against the course, unless it is already indexed at its digest. Transcript passages are stored under the media they belong to (the same rule as the transcript panel, now shared as `transcript_for`), so a result can play it.
  - `unindexed_local_texts` lists the text files of local courses not yet indexed.
- **Indexing in the app:**
  - Local courses: at startup and after each folder import, a background task reads each pending file, re-hashes it against the manifest, and indexes it with the library unlocked during the read. Files over the text and transcript limits are skipped.
  - Torrent courses: a transcript or text is indexed when the window reads it, from the verified bytes. Searching never starts a download.
- **Command:** `search_library { query }` returns path-free results (`kind`, course and lesson ids and titles, `sourceId`, `position`, `snippet`). Queries over 256 bytes are refused with `invalid_input`.
- **UI (`LibrarySearch`):** replaces the "Search arrives with lesson indexing" placeholder. It searches 200 ms after typing pauses, labels each result by kind ("Transcript at 1:01:41", "Your note", "Lesson text"), shows the course and lesson, says when nothing matches, and reports failures.

## Acceptance evidence

- **Domain tests:** queries quote every word and prefix the last; FTS5 operators, column filters, `NEAR`, and quotes become plain words; punctuation-only input searches nothing; long input is capped at 12 words. Cues and paragraphs group into passages at the time and length limits.
- **Storage tests:**
  - Course titles are found and highlighted. Notes are found after insert, follow updates, and disappear on delete.
  - Indexing a file records its digest; re-indexing at a new digest replaces its passages; re-importing the course drops file text but keeps notes.
  - A malformed FTS5 query is an error, not a crash.
  - A v10 database with a course and a note migrates to v11 with both searchable; the v4 migration path still reaches the current schema.
- **Application test** (a local fixture with a video, two transcripts, a Markdown file and a text file):
  - All four text files are pending; each indexes once, then nothing is pending; a video is not indexed.
  - `hel` finds transcript passages at 1 s under both videos, `notes` finds both text files, the course title and lesson title are found as their kinds, and a new note is found with its snippet highlighted.
  - FTS5 syntax typed by the learner returns no results instead of an error, and two words must both match.
- **Vitest:**
  - Snippets mark exactly the matched terms.
  - Search waits for typing to pause, labels results, and clears itself when a result is opened.
  - No-match and failure states show.
  - In the App, a transcript match opens its course and plays the matched video.
- All repository gates pass.

## Manual verification

Not yet done in a real window. Steps:

1. Import a local course with a WebVTT transcript and a Markdown lesson, then restart the app.
2. Type a word from the transcript; check that a "Transcript at m:ss" result appears and that opening it plays the video from that moment.
3. Type a word from the Markdown lesson and from a note you wrote; check that each opens its lesson.
4. For a torrent course, open a transcript once, then check that it becomes searchable.

## Current limitations

- Torrent courses' transcripts and text are searchable only after they have been read once, so search never downloads anything.
- PDF text is not indexed.
- A text match opens the reader at the top, not at the passage. A note match opens its lesson, not the note's video moment or PDF page.
- Background indexing of local courses is bounded per file and resumes after a restart, since finished files are recorded, but it cannot be cancelled and reports no progress.
- The `unicode61` tokenizer does no stemming and does not segment Chinese, Japanese or Korean text.
