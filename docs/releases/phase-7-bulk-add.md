# Phase 7: Adding lessons in bulk

Status: complete. The Builder adds many lessons at once, from a native picker or by drag and drop. Folders become modules. A course folder (one whose folders hold the lessons) turns each of those folders into a module, and loose files become the lessons of one new module unless they are attached to a lesson. These are the user's decisions of 2026-10-16, recorded as an amendment to [ADR 0007](../decisions/0007-builder-drag-and-drop.md).

## Delivered

- **Pickers:** the Builder shows "Add folders…" and "Add files…", which open the native multi-select picker. "Add files…" offers video, audio, and PDF. `choose_lesson_sources { kind }` scans the picked paths in Rust with the same bounded `plan_drop` as a drop and returns the same path-free summary and token, or `null` on cancel.
- **Course folders:** `plan_drop` treats a folder with no lessons of its own, but with folders inside, as a course folder, and each folder inside becomes a module, in natural name order. Deeper folders, empty folders, and other files are reported with their relative names. The 100-module limit applies across the course folder.
- **Loose files:**
  - **Default:** they become the lessons of one new module, named after their shared folder or "New module". Non-media files are left out and reported.
  - **Attaching:** the panel's choice reads "Lessons of a new module, “Week 1”", or "Attachments of Module › Lesson". Files dropped onto a lesson still preselect that lesson.
- **Panel:** "Add files and folders" serves picks and drops alike; a new pick or drop replaces and discards the previous one.

## Acceptance evidence

- **Planning and applying tests (`publisher-application`):**
  - A course folder yields modules in natural order (`Module 2` before `Module 10`) and reports a deeper folder, an empty folder, and a non-media file.
  - A folder with its own lessons stays one module and reports its folders as nested.
  - A module limit of zero adds nothing.
  - Loose files from "Week 1" become module "Week 1" with lessons "Intro", "Caching", "handout" in file order, leaving out `notes.txt`.
  - The same files attach to a chosen lesson, all four of them. Files from different folders get "New module".
  - An empty plan changes nothing.
- **App test:** summaries carry `looseModuleTitle` and no paths, and a loose PDF without a lesson becomes a second module in the same revision.
- **Vitest:**
  - Cancelling the picker shows nothing. Picked folders show the module preview, and dismissing discards the token.
  - Picked files apply with no target lesson, and the new module appears.
  - The loose-file choice defaults to the new module and can switch to a lesson.
  - All Publish suites pass.
- All repository gates pass.

## Manual verification

1. In Publish → a draft, click "Add folders…" and pick a course folder whose folders hold the videos, for example `algoe-sixquevideo`'s parent or a `Course/Module N/` tree. Check that one module appears per folder.
2. Click "Add files…" and pick several videos from one folder. Check that a module named after that folder appears, holding one lesson per file.
3. Drag files onto an existing lesson. Check that they are offered as attachments of that lesson.

## Current limitations

- Only one level below a course folder is read; deeper trees are reported, not flattened.
- The native picker and drag gesture cannot be driven by WebDriver; the steps above are manual.
- "Add files…" filters to video, audio, and PDF. To attach other files to a lesson, use the lesson's own attach button or drop them onto the lesson.
