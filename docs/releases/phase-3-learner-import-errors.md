# Phase 3: local import failures no longer look like a broken library

Status: complete. A failed local-folder import is reported as an import failure, with its reason, instead of the "could not open the local library" alert.

## Delivered

- **Learner UI:** the library alert now appears only when readiness or the course list fails. A failed `choose_and_import_course` shows a separate "Course import failed" alert with the command error's message. It appears in both the library and course views, and **Dismiss** clears it. If the failure is not a command error, the alert shows a generic message instead.
- **Tauri command:** import errors are now made path-free before crossing IPC:
  - I/O failures report only the operation that failed (for example "the course folder could not be read while opening course.json"). The OS error is dropped.
  - A package root that cannot be resolved, and a folder the dialog cannot express as a local path (`unsupported_folder_location`), report fixed messages.
  - Storage failures while saving a verified course return `library_unavailable` with a fixed message. SQLite details are dropped.
  - Validation failures still report their reason. These name only manifest-relative paths and resource IDs; `course.json` validation already rejects absolute package paths.

## Acceptance evidence

- **Vitest (`App.test.tsx`):**
  - A rejected import shows its message, and the library's empty state and course list remain visible.
  - The library alert does not appear for an import failure, and Dismiss removes the import alert.
  - A failure that is not a command error shows the generic message, not the raw error.
  - A list failure still shows the library alert.
  - **Mutation check:** restoring the old combined condition (`|| importCourse.isError`) fails both import tests.
- **Rust (`local_import_failures_never_name_local_directories`):** I/O, package-location, and storage failures whose sources contain `/home/…` produce messages without that path. A missing-resource failure keeps its manifest-relative path.
- All repository gates pass.

## Manual verification

These steps have not yet been run in a real window for this slice. The folder picker is a native dialog, so WebDriver cannot drive it.

1. Start the Learner app with an empty or populated library.
2. Choose **Import local course** and select a folder without `course.json`.
3. Expect "Course import failed" and "invalid course manifest file: course.json is missing". The library stays usable.
4. Choose **Dismiss**. The alert disappears.

## Current limitations

- Validation messages come from the application layer and are not yet written as learner-facing copy (they are lowercase and technical).
- There is no retry button. Choosing **Import local course** again starts a new import and clears the old error.
