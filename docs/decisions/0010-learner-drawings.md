# ADR 0010: Learner drawings and keeping the window on the app

Status: accepted (2026-10-16)

## Context

Phase 6 asks for Excalidraw scenes persisted with local assets. Excalidraw is a large editor that by default loads fonts from a CDN, offers web embeds (YouTube, Figma and others), and links out to its own web services. The Learner must stay offline-first, must never overwrite learner data, and must not become a browser for remote content.

## Decision

1. **One drawing per lesson.** Each lesson has at most one drawing (schema v10, `lesson_drawings`), keyed by course and lesson. Removing a course keeps it (ADR 0015). It never lives in course packages.
2. **Compare-and-swap like notes.** A new drawing is saved with no expected revision; later saves must name the revision they saw. A conflict writes nothing, and the UI stops autosaving until the learner loads the saved drawing.
3. **Validated scene.** `learning-domain::Scene` accepts a JSON object with an `elements` array, up to 2 MiB.
4. **Images stored once, never changed.** They are kept in `drawing_files` as data URLs:
   - **Accepted:** PNG, JPEG, WebP or GIF only. SVG is refused because it can carry script.
   - **Checks:** the declared type must match the data URL, the base64 must be valid, each image is at most 10 MiB, an ID is at most 128 characters of `[A-Za-z0-9_-]`, and a drawing holds at most 64 images.
   - **Write-once:** an image whose ID is already stored is never replaced. Excalidraw's file IDs are content hashes.
   - **Saves are lean:** only images the window has not sent yet travel over IPC.
5. **Saving.**
   - **Autosave:** the window saves 800 ms after drawing pauses.
   - **What is saved:** the scene, without deleted elements and without view state other than the background colour.
   - **Restoring:** the first change Excalidraw reports after loading is the reserialised saved scene, so it becomes the baseline rather than a save.
6. **No remote content in the editor.**
   - **Fonts ship with the app:** a Vite plugin publishes Excalidraw's Latin fonts under `/excalidraw/fonts/`, leaving out the 13 MB Xiaolai CJK font. `window.EXCALIDRAW_ASSET_PATH` is set before Excalidraw is evaluated.
   - **CSP unchanged:** it still has `font-src 'self'`, so Excalidraw's built-in esm.sh fallback is blocked.
   - **Embeds disabled:** `validateEmbeddable` rejects every link.
   - **Menu trimmed:** scene loading, saving to a file and export are hidden.
7. **Navigation guard.** A `navigation-guard` plugin refuses every top-level navigation except the bundled app: `tauri://localhost` or `http(s)://tauri.localhost`, plus the Vite dev server on `localhost:1420` in debug builds only. The rule applies to every webview of the Learner, not only the drawing editor.

## Consequences

- Drawing works with no network, and no learner drawing can be lost to a second window.
- Data URLs make images about a third larger than their bytes. A drawing at its limits could hold up to about 640 MiB, which is acceptable for learner-made sketches. Content-addressed files on disk can replace them later without changing the IPC shape.
- WebKitGTK reports a CSP violation for each blocked fallback font URL even when the local font loads; these reports are expected.
- The editor is a 1 MB chunk (plus its own 1.8 MB dependency chunk), loaded only when a drawing is opened.
