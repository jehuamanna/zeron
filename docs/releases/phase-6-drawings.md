# Phase 6: Lesson drawings

Status: complete. Each lesson has an Excalidraw drawing that the learner can open beside the notes. It saves as they draw, keeps pasted or dropped images, and survives restarts. A save never overwrites a newer drawing, and the editor never reaches the network. ADR 0010 records the decisions.

## Delivered

- **Domain (`learning-domain::drawings`):**
  - **`Scene::parse`:** a JSON object with an `elements` array, up to 2 MiB.
  - **`DrawingFile::new`:**
    - accepts PNG, JPEG, WebP or GIF data URLs with a matching type and valid base64, up to 10 MiB;
    - IDs are at most 128 characters of `[A-Za-z0-9_-]`;
    - SVG is refused.
- **Storage (learner schema v10):**
  - **Tables:** `lesson_drawings` (one per course and lesson, `revision`) and `drawing_files` (write-once images, cascading with the drawing).
  - **`save_drawing`:** creates or compare-and-swap replaces the drawing and adds new images in one transaction.
- **Application:**
  - `CourseLibrary::drawing` and `save_drawing` check that the lesson exists.
  - Images whose IDs are already stored are skipped, and a drawing is capped at 64 images.
  - The outcome is `DrawingSave::{Saved, Conflict}`.
- **Commands:**
  - `get_drawing { courseId, lessonId }` returns `{ sceneJson, revision, files }` or `null`.
  - `save_drawing { courseId, lessonId, expectedRevision, sceneJson, files }` returns `saved` or `conflict`. Invalid scenes and images return `invalid_input`; unknown lessons return `note_target_invalid`.
- **UI:**
  - **Panel:** each lesson's "Notes" view shows a "Drawing" panel with "Open drawing". The Excalidraw editor (`@excalidraw/excalidraw` 0.18.1, exact pin) loads lazily in dark theme.
  - **Saving:** autosave runs 800 ms after drawing pauses and shows "Saving…" or "Saved". Only images not yet sent are sent.
  - **Conflicts:** a conflict stops saving and offers "Load the saved drawing".
  - **Offline editor:** embeds, scene loading, saving to a file and export are disabled.
  - **Fonts:** Excalidraw's Latin fonts ship with the app under `/excalidraw/fonts/` through a small Vite plugin, which also serves them in dev.
- **Security:**
  - **Navigation guard:** a new `navigation-guard` plugin in the Learner refuses top-level navigation to anything but the bundled app (plus the dev server in debug builds).
  - **CSP:** unchanged. `font-src 'self'` blocks Excalidraw's CDN fallback, and `img-src 'self' data:` covers drawn images, since Excalidraw uses data URLs, not object URLs.
- **Tooling:** `@types/node` 26.6.5 (exact pin, dev only) so `vite.config.ts` type-checks its Node imports.

## Acceptance evidence

- **Domain tests:**
  - Scenes need an `elements` array and are size-limited.
  - Image IDs, types, prefixes and base64 are checked, SVG and mismatched types are refused, and images are size-limited.
- **Storage test:**
  - A drawing for an unknown course is refused.
  - Create, conflict on a second create, save from revision 0 and conflict on a stale revision all behave as expected.
  - Images are never replaced, and a conflicting save to a missing drawing creates nothing.
- **Application test:**
  - Revisions advance and stale saves conflict.
  - Stored images keep their first content while new ones are added.
  - Unknown lessons, scenes without elements, SVG images and the 65th image are refused, and none of them changes the drawing.
- **Command tests:** the navigation guard allows only the bundled origins and refuses web sites, the media gateway, other localhost ports, `file:` and `about:blank`.
- **Vitest** (editor stubbed):
  - The drawing is not fetched until it is opened, and an empty new scene is not saved.
  - The first save has no revision and carries the new image; the next save continues from revision 0 without resending it.
  - A restored scene's first report is not saved.
  - A conflict shows the alert and blocks later saves, and "Load the saved drawing" reloads it.
- **Real window** (WebKitGTK, scratch Learner library, *Design Slack* lesson of the real course):
  - A rectangle and an ellipse were drawn with pointer events, and the panel showed "Saved".
  - A PNG dropped on the canvas was stored once (`image/png`); dropping the same image again stored nothing new.
  - After a restart, the drawing reopened with both shapes and the image, and opening it caused no save.
  - Excalifont loaded from the app (`document.fonts.load` → loaded; a local font fetch → 200).
  - Setting `location.href` to `https://libraries.excalidraw.com/` left the window at `tauri://localhost`.
- All repository gates pass.

## Current limitations

- One drawing per lesson. There are no drawings anchored to a video moment or PDF page yet.
- CJK handwriting text falls back to a system font, because the Xiaolai font is not bundled.
- Excalidraw's library browser and help links point at web sites and do nothing, because navigation is refused.
- Images are stored as data URLs in SQLite, so a drawing at its limits is large. Drawings are not part of search yet.
