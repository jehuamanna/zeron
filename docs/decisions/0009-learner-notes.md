# ADR 0009: Learner notes, anchors, and never overwriting them

Status: accepted (2026-10-16)

## Context

Phase 6 adds notes that point at places in a lesson. The roadmap requires that annotations are never overwritten, and that learner data stays outside course packages. Notes are rich text written in Tiptap. Later, search needs their plain text.

## Decision

1. **Typed anchors in `learning-domain`.** There are three kinds:
   - `{"kind":"lesson"}`;
   - `{"kind":"videoTime","resourceId","seconds"}`, where `seconds` is finite and between 0 and 48 h;
   - `{"kind":"pdfPage","resourceId","page","region"?}`, where `page` is between 1 and 100,000 and the region lies within the page in fractions from 0 to 1, with a positive size.

   Parsing is strict, so unknown kinds and fields are rejected. The application also checks that the lesson exists and that the anchor's resource belongs to it with the matching kind: a video for timestamps, a PDF for pages.
2. **Bodies are validated Tiptap JSON.** A body is at most 256 KiB, must have a top-level `doc`, can nest at most 64 levels, and may carry text only in string `text` fields. Plain text is extracted on save and stored beside the document for previews and search. The UI renders saved notes with a read-only Tiptap editor on the same schema, so no stored HTML is ever injected.
3. **Compare-and-swap revisions.** Notes start at revision 0. Updates and deletes must name the revision they saw. A mismatch is reported as a conflict with the current revision, and nothing is written. The UI keeps the learner's draft on screen and offers to load the latest version, rather than retrying blindly.
4. **Storage.** Notes live in the learner database (schema v9, table `notes`), keyed by course. Removing a course keeps them (ADR 0015). They are never stored in course packages, and they are kept across re-imports of the same course.

## Consequences

- Two windows or a retried request can never silently replace a note.
- Anchors that point at a resource missing from a newer course version stay readable; seeking to them simply finds nothing. Suggesting how to migrate annotations between versions is Phase 7 work.
- The stored plain text is ready for the search slice.
