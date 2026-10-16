# ADR 0016: Deleting a whole course from Publish

Status: accepted (2026-10-16)

## Context

Publishers asked to delete a course from "Your courses". Until now they could only delete one draft, archive one release, or delete one archived release's files (ADR 0006). Two earlier decisions still apply:

- Release rows are kept so that a course version is never published again with other content (ADR 0003, ADR 0006 decision 4).
- Deletion needs consent checked in the backend (ADR 0006 decision 2).

The user chose (2026-10-16) "everything, keep history":

- delete all drafts;
- stop seeding every release and take it out of the catalog;
- delete all release files;
- hide the course;
- confirm by typing its title.

## Decision

1. **Consent in Rust.**
   - `PublisherDrafts::delete_course(releases_root, course_id, confirmation)` refuses unless `confirmation` equals the course title as the list shows it: the most recently edited draft's title, else the newest release's title, else "Untitled course".
   - The desktop command checks the title again before stopping anything.
2. **Order.**
   - The command refuses with `draft_busy` if any draft of the course is being prepared for publishing, so no release can appear behind the deletion.
   - It stops seeding each published or superseded release through `SeedingService::archive`, without holding the drafts lock, which seeding takes itself.
   - `delete_course` then:
     1. deletes every draft of the course;
     2. archives any release still in use, which is idempotent;
     3. un-lists each release from the catalog;
     4. deletes each release's files with the same recorded, rename-then-remove steps as ADR 0006 (`delete_archived_files`, now shared);
     5. hides the course;
     6. records one `course_deleted` activity event.
3. **History stays.**
   - Schema 7 adds `hidden_courses`. Release rows stay archived with `files_state = deleted`, so their versions are still refused.
   - The draft and release lists leave hidden courses out.
   - A new draft or release with the same course ID removes the hidden mark, so the course reappears with its history.
4. **Crash safety.** A crash part-way leaves:
   - deletions in `deleting`, which ADR 0006's start-up recovery finishes;
   - the course still listed, with whatever remains.

   Running Delete course again finishes the job.

## Consequences

- A deleted course cannot be restored; its files are gone. Learners who already downloaded it keep their copies; this computer no longer seeds it.
- An info hash reserved on the Library router for a deleted release stays reserved until the next start, when only releases with files are reserved again.
- Tests:
  - storage: `deleted_courses_are_hidden_until_a_new_draft_or_release_brings_them_back`, plus the schema version tests;
  - application: `deleting_a_course_needs_its_title_and_removes_drafts_files_and_listings`;
  - UI: `CourseList.test.tsx`, "deleting a whole course".
