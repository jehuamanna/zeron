# ADR 0007: Builder drag-and-drop keeps dropped paths in Rust

Status: accepted (2026-10-16)

## Context

Publishers asked to build courses by dragging folders and files into the Builder. The user decided on 2026-10-16 that each dropped folder becomes a module, its media and PDF files become lessons in name order, and loose files dropped onto a lesson attach to it. AGENTS.md requires untrusted paths to be resolved in Rust and never accepted from React.

Tauri 2.12 delivers native drops as `WindowEvent::DragDrop`. Its default handler also emits `tauri://drag-drop`, with the paths, to the webview. It also widens the fs and asset-protocol scopes for the dropped paths. The only way to stop this is to disable the handler (`dragDropEnabled: false`), but then Rust receives no drop events at all. WebKitGTK's HTML5 drop would then hand JavaScript `File` objects, so multi-gigabyte media would have to be streamed through IPC.

## Decision

1. Keep Tauri's native handler. The Admin app handles `WindowEvent::DragDrop` in Rust (`drops::handle_drag_drop`). React never subscribes to `tauri://drag-drop` and never sends a path to a command.
2. On a drop, Rust scans the paths off the event loop with `plan_drop`:
   - The scan is bounded by `DropLimits`: 5,000 entries, 1,000 files, and 100 folders.
   - It reads one folder level and never follows symbolic links (`symlink_metadata`). Hidden entries are skipped.
   - Folder files that are not video, audio, or PDF are skipped.
   - It sorts with a natural comparison, so `lesson2` comes before `lesson10`.
   - Titles are cleaned up: known media extensions are removed repeatedly (`x.mp4.mp4`), as is a leading ordinal (`01 - `).
   - Everything left out is reported by name, relative to what was dropped, with a reason.
3. The plan is kept in a `DropInbox` under an opaque `drop_<hex>` token. The inbox holds at most four plans, for ten minutes each. The UI receives a path-free `builder-drop` summary: the token, the drop point in CSS pixels, module and lesson titles, file names, kinds, and skipped entries.
4. The UI finds the lesson under the drop point (`elementFromPoint` → `[data-lesson-id]`) and preselects it for loose files. It then calls `apply_drop { draftId, expectedRevision, token, target: { lessonId } }` through the Builder's `draftWriter`. `PublisherDrafts::apply_drop` re-checks every file (regular file, canonical path), skips any that vanished, inspects media headers, and saves everything as one revision with the new `DraftCourse::import`. A successful apply consumes the token.

5. **Amendment, 2026-10-16: bulk adding** (user decisions of 2026-10-16).
   - **Pickers:** "Add folders…" and "Add files…" open the native multi-select picker in Rust (`choose_lesson_sources { kind }`). The picked paths go through the same `plan_drop`, inbox, token, and `apply_drop` as a drop, so React still never sees a path.
   - **Course folders:** a folder with no lessons of its own but with folders inside is a course folder, and each folder inside it becomes a module. Folders deeper than that are reported, not flattened. A folder with lessons of its own stays one module, as before.
   - **Loose files:** without a chosen lesson, they become the lessons of one new module, in name order. The module is named after their shared folder, or "New module" when they come from different folders. Only video, audio, and PDF become lessons; other files are reported. Files dropped onto a lesson still preselect it, and attaching to a lesson still accepts any file. The `no_target_lesson` reason is gone.

## Consequences

- Dropped paths reach the webview's event bus through Tauri's built-in event, but no Zeron code reads them, and the CSP allows only bundled scripts. The scope widening has no effect, because the Admin app enables neither the fs plugin nor the asset protocol. Enabling either would require revisiting this ADR.
- A drop is applied atomically. A stale revision triggers the same conflict banner as any other edit, and a drop can be retried until its token expires.
- At most one folder level below a course folder is imported. Deeper folders are reported, not flattened.
- The native drag gesture itself cannot be automated with WebDriver. The scan and apply logic is tested against real course folders in Rust, and the UI against scripted events.
