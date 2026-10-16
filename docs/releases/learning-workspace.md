# Learning workspace and themes

Status: complete. The user asked for a revamp of the whole UI: a rich, responsive learning experience that fits on one screen (2026-10-16). Most of the code first landed by accident inside the parallel session's commit `03a7821` ("docs: record the Windows keychain result"). The follow-up commit adds the fixes, tests, and this note.

## Delivered

### 1. One screen, no page scrolling

- The app fills the window (`h-full`). `body` never scrolls; each pane scrolls on its own.
- Every scroll area is a positioning container, so screen-reader-only text (`sr-only` is absolutely positioned) cannot stretch the page.
- The shell header is a 48 px bar: logo, Library/Publish, theme toggle, and Network.

### 2. Course workspace (`features/course/`)

A course opens into three panes:

- **Contents (left, 300 px):** progress, download controls for torrent courses, and the modules. Each lesson row shows its number, status, duration, and material chips (Play video / Open PDF / Read). Notes, mark complete, and keep offline are icon buttons with the same accessible names as before.
- **Stage (centre):**
  - **Course overview** until a lesson opens: description, module/lesson/video totals, progress, a Start or Continue button for the next lesson, the publisher badge, the torrent note, sharing, annotation moves, and the syllabus.
  - **Open lesson:** "Module · Lesson n of N", the title, a Video/PDF/Reading switcher when the lesson has more than one, Mark complete, and previous/next lesson.
- **Side panel (right):** Notes, Sketch, and Transcript tabs (Transcript only while a video plays). Drag or use the arrow keys to resize it between 300 and 720 px.

Layout follows the window width:

- **1280 px and wider:** all three panes are docked.
- **1024–1279 px:** contents becomes a drawer.
- **Under 1024 px:** the side panel is a drawer too, so the lesson gets the full width down to the 760 × 560 minimum window.

Which panes are open and the side-panel width are saved in `localStorage` (`zeron.workspace`); losing them is harmless.

### 3. Player, PDF, reader, notes, sketch

- **Player:** the video keeps its shape (`object-contain`) inside the stage, so its controls are always visible. A toolbar adds −10 s / +10 s, speed (0.75×–2×), and "Note at m:ss". "Resumed where you left off" is now an overlay on the video.
- **PDF:** the page scrolls inside its own area. The toolbar has a page-number field, zoom from 50% to 300%, fit width / fit page, and Highlight area. The page re-renders when its area is resized.
- **Reader:** a centred reading column in a serif face.
- **Notes:** the list scrolls, and the composer is pinned at the bottom.
  - The composer has a formatting toolbar: bold, italic, strikethrough, inline code, headings, lists, quote, code block, undo, and redo.
  - Ctrl+Enter saves and Escape cancels.
  - "Add note at" follows the player every second.
  - The player and the N shortcut start a note through a one-shot request (`NoteRequest` plus `onRequestHandled`), so a request is never replayed when you switch lessons.
- **Sketch:** Excalidraw opens straight away, fills the panel, and follows the app theme.

### 4. Keyboard

| Key | Action |
| --- | --- |
| K | Play or pause |
| J / L | Back or forward 10 s |
| N | New note at this moment or PDF page |
| ← / → | Previous or next PDF page |
| [ | Show or hide contents |
| ] | Show or hide the side panel |
| ? | List these shortcuts |

Shortcuts are ignored while you type in a field, the note editor, or Excalidraw.

### 5. Light and dark themes

- `styles.css` defines tokens for each theme:
  - **light:** warm paper with a wine accent;
  - **dark:** ink with a rose accent.
- `data-theme` on `<html>` selects the theme. It is applied before the first render, follows the desktop until the learner chooses, and is saved as `zeron.theme`.
- New components use semantic utilities (`bg-surface`, `text-muted`, `border-line`, `bg-accent`, …).
- The palette names the older screens use (`white`, `black`, `slate-*`, `cyan-*`, `amber-*`, `red-*`, `emerald-*`, `violet-*`, `sky-*`) are aliased to theme tokens. This re-themes Publish, Network, and every card without rewriting them; Publish keeps its amber accent.

### 6. Library home

- A "Pick up where you left off" card for a course in progress.
- Course cards with a stable generated cover colour, source badges, and progress.
- Skeleton loading.
- An "Add courses" section for torrent import and catalogs.

### 7. Simpler contents list (follow-up)

- **Controls were rendering too large:** the `font: inherit` reset on buttons and fields sat outside Tailwind's layers, so it overrode every text-size utility. It now lives in `@layer base`, which makes buttons app-wide the size they were written to be.
- **One 32 px line per lesson:**
  - Each row shows its status, number, and title, small icons for its materials (video, PDF, reading), and the duration.
  - For torrent courses, a coloured dot shows availability.
  - Notes, mark complete, keep offline, and remove download replace the duration on hover or keyboard focus.
  - Download controls are icons in the Contents header.
  - Accessible names are unchanged.

### 8. Full-height note writing and the course notebook (follow-up)

- **Writing a note:** the editor fills the whole Notes panel. The note's anchor and **Cancel** / **Save note** sit at the top. The list and the add buttons return when the note is saved or cancelled.
- **Notebook** (button in the course bar): every note and sketch of the course in one place.
  - Grouped by lesson, in course order. Within a lesson: whole-lesson notes first, then by video moment, then by PDF page.
  - Filters for All, Notes, and Sketches, and a search over note text.
  - A timestamp or page chip jumps there; "Edit notes" and "Open sketch" open the lesson with that tab.
  - Sketches show as still previews. `SketchPreview` loads lazily, renders with Excalidraw's `exportToSvg`, and shows the SVG through an `<img>` with a `data:` URL, so nothing in it can run and the CSP is unchanged.
  - Notes on lessons that a newer course version dropped are not shown here; Annotation moves on the overview still offers to move them.
- **New IPC command:** `list_drawn_lessons(courseId) -> string[]`, backed by `CourseLibrary::drawn_lessons` and the existing `drawing_lessons` storage query. It returns lesson IDs only, so the notebook stays light. The command table in `lib.rs` now carries `#[expect(clippy::too_many_lines)]`.

### 9. Library notebook (follow-up)

- **Where:** a **Notebook** button beside the Library search shows every note and sketch across all courses. It groups by course, then by lesson in course order.
- **Filters:** the All / Notes / Sketches filter and the note search are shared with the course notebook (`features/notebook/LessonGroups.tsx`). A course picker narrows to one course; courses with nothing captured are left out.
- **Opening entries:**
  - Clicking a timestamp, a page, "Edit notes", or "Open sketch" opens that course at that place.
  - `CourseWorkspace` now takes a `WorkspaceTarget` (`search`, `moment`, `page`, `lesson` with a tab) instead of a bare search result.
  - Leaving the course returns to the notebook (`useLibraryUi.notebookOpen`).
- **Loading:** no new IPC. Each course's outline, notes (`list_notes` with no lesson), and drawn lessons load in parallel with `useQueries`; the outlines share the workspace's cache entries.

### 10. PDF reader, rich notes, tooltips, and the video player (follow-up)

- **PDF reader** (`PdfDocument`, `PdfSidebar`, `DocumentViewer`):
  - **Layout:** one continuous column of pages; only pages near the view render. Each rendered page has a pdf.js canvas and a selectable text layer.
  - **Toolbar:** sidebar, page field, zoom (fit width, fit page, 50–300%), rotate, two pages side by side, paper/sepia/night page colours, a highlight colour palette, note on this page, box an area, find in document, and full screen.
  - **Sidebar:** Pages (lazy thumbnails), Contents (the PDF outline), Highlights, and Details (metadata, size, version).
- **Text highlights are notes:**
  - Selecting text shows colour dots, underline, strike-through, "Highlight and comment", and copy.
  - Clicking a saved highlight recolours, restyles, comments on, copies, or deletes it.
  - The anchor gains an optional `highlight` (`color`, `style`, `quote`, `rects` in page fractions). It is validated in `learning-domain`: quote 1–4,000 characters, 1–256 boxes inside the page.
  - It is stored inside the existing `anchor_json`, so no migration is needed; notes without it serialize exactly as before. The highlight is boxed so `Anchor` stays small.
  - The note body also quotes the text, so a highlight that annotation moves re-attach to a whole lesson keeps its words.
  - Rectangles are stored for the unrotated page and turned for display.
- **Note editor:** the toolbar has undo/redo, H1–H3, bold, italic, underline, strike-through, highlight, inline code, link, bulleted, numbered, and checklist lists, quote, code block, and divider.
  - This adds `@tiptap/extension-highlight` and `@tiptap/extension-list`, pinned to 3.32.0.
  - Links accept only `http(s)` and `mailto`, and never navigate the app window.
  - The Notes panel starts notes from the reader with a quoted body and opens existing notes for editing (`NoteRequest`).
- **Tooltips:** one app-wide `Tooltips` layer explains every icon-only control (its `aria-label`) and every control with a `title`, on hover and keyboard focus. The native tooltip is held back so only one shows.
- **Video player:** custom controls replace the built-in ones.
  - **Seek bar:** played, buffered, and verified data, notes as markers, and a time preview under the pointer.
  - **Controls:** play/pause, ±10 s, volume and mute, elapsed or remaining time, Note at m:ss, captions from the transcript, a speed menu, a settings menu, picture-in-picture where available, next lesson, and full screen.
  - **Autoplay:** "Autoplay next lesson" counts down 5 s with Play now / Cancel.
  - The controls hide while playing. Speed, volume, mute, captions, and autoplay are remembered (`zeron.player`).
  - **Shortcuts:** ←/→, ↑/↓, M, C, and F.
- **Full screen** for the whole workspace from the course bar.

### 11. Bundled typefaces and consistent icons (follow-up)

- **Typefaces:** the named fonts (Inter, Iowan Old Style) were not installed on Linux, so titles fell back to a plain system sans. The app now bundles three variable fonts, all SIL OFL, from `@fontsource-variable`; they are served by the app itself, so the CSP is unchanged:
  - interface: Inter, with optical sizing;
  - titles, notes, and reading: Source Serif 4, roman and italic;
  - code and timestamps: JetBrains Mono.
- **Swapping fonts:** `src/fonts.css` is the only file to change for licensed fonts: declare them and put their names first in `--zr-font-sans`, `--zr-font-display`, and `--zr-font-mono`.
- **Icons:** every lucide icon uses one 1.75 line weight, and icons never shrink in tight bars.
- **Drop-downs:** selects lose the system button for a theme-coloured chevron.
- **Form controls:** checkboxes, radios, and ranges take the accent colour.

### 12. Monochrome theme (follow-up)

- **Palette:** the user asked for a black-and-white look, after a reference dashboard.
  - **Light theme:** white surfaces on `#fafafa`, near-black ink and buttons, zinc greys, hairline borders, and soft shadows.
  - **Dark theme:** the same palette mirrored.
  - **Other colour:** red, amber, and green remain only for errors, warnings, and success, and the six highlight colours stay the learner's own.
- **Type:** headings are bold Inter. A new `--font-reading` keeps Source Serif 4 for notes, quotes, and lesson readings.
- **Covers:** course covers are near-black tiles with diagonal bands at a per-course angle.
- **Dark theme, revised:** it follows a second reference as layered charcoal.
  - **Layers:** a new `--zr-panel` token (white in light) carries the header, course bar, contents, side panel, and PDF sidebar at the darkest step. The stage is mid-grey (`#25262a`) and cards a step lighter (`#2d2e33`).
  - **Controls:** buttons are light grey with dark text.
- **Light theme, revised to match:** the same layering in daylight, with charcoal (`#2d2e33`) rather than black for ink and buttons.
  - **Layers:** soft grey panels (`#ececef`), a pale stage (`#f4f4f5`), and white cards.
- **Video player:** the seek bar and switches are white on black.
- **Publish:** its main buttons use the shared accent instead of amber.

### 13. Removing courses, a tidier overview, and a Previous button (follow-up)

- **Remove from library** is in each course card's "⋯" menu and in the open course's actions menu (ADR 0015). The confirmation names what goes and what stays.
  - **Local courses:** only Zeron's record goes; the folder is never touched.
  - **Torrent courses:** the transfer stops and Zeron's download directory is deleted. Before deleting, Zeron checks the path, refuses links, and keeps a torrent another course shares. A crash is finished at the next start.
  - **Kept:** notes, highlights, sketches, and progress stay. They return on re-import and stay readable, marked "Removed from library", in the library notebook.
  - New IPC: `remove_course` and `list_removed_courses`. `get_course_detail` falls back to a removed course's kept manifest so the notebook can order its notes. Storage is at schema 15.
- **Course overview:**
  - The publisher, torrent, and sharing blocks became one row of chips: the publisher chip opens its explanation and "Forget publisher", the torrent chip explains streaming in a tooltip, and sharing is an inline switch.
  - "What's inside" modules expand to their lessons, with status and duration, and each lesson opens directly.
- **Notes and sketch panel:** it only appears once there is a lesson to take notes on.
- **Player:** a Previous lesson button sits beside Next.

### 14. Deleting a whole course from Publish (follow-up)

- **Where:** each course in Publish → Your courses has a delete button. Its confirmation lists what goes (drafts, seeding releases, catalog entries, the size of release files) and enables only once the exact title is typed (ADR 0016).
- **What happens:** the backend checks the title again, refuses while a draft is being prepared, stops seeding, then deletes drafts, archives and un-lists releases, and deletes their files with ADR 0006's crash-safe steps.
- **Afterwards:**
  - The course leaves the list. Its archived release records stay hidden, so versions are never reused.
  - A new draft or release with the same course ID brings it back.
  - The publisher database is at schema 7, and the activity log gains "Course deleted".
- **Also:** publisher identity backups (`zeron-publisher-key*.json`) are ignored by git.

## Acceptance evidence

- **Vitest:** all 176 tests pass, 3 of them new.
  - The workspace opens the next lesson from the overview, shows "Lesson 1 of 2", steps to the next lesson, hides and shows contents with `[`, starts a timestamped note with `N`, and has no axe violations.
  - A note request starts a draft once, reports it handled, and Ctrl+Enter saves it.
  - The theme toggle switches `data-theme` and saves the choice.
- The existing Library, player, PDF, notes, transcript, and drawing tests pass unchanged, apart from the new optional props.
- The repository gates pass: `cargo fmt`, `clippy`, `cargo test`, `pnpm test`, `pnpm typecheck`, `pnpm build`, and `git diff --check`.

- **Follow-up tests:**
  - `drawings_save_from_their_current_revision_and_keep_images` now also checks `drawn_lessons` before and after the first save.
  - `CourseNotebook.test.tsx` checks lesson order, ordering inside a lesson, counts, jumping to a moment, the search and Sketches filters, and axe.

- **Library notebook test** (`Library.test.tsx`): notes from two of three courses appear under their course headings with the right counts, the search narrows to one course, axe passes, and a timestamp opens the right course at that moment. Back to library returns to the notebook.

- **Follow-up tests:**
  - **Domain:** highlights round-trip, older page notes still read, and bad highlights are refused. Annotation moves carry highlights, and an application test saves and reads one.
  - **Viewer and geometry:** `DocumentViewer.test.tsx` (selection, colours, styles, comment, recolour, delete, area, layout controls) and `pdfGeometry.test.ts` (line joining, clipping, caps, rotation).
  - **Tooltips:** `Tooltips.test.tsx`.
  - **Player:** play/pause, speed memory, mute, captions, markers, and the autoplay countdown and its cancel and off switch.

## Manual verification

These checks used Vite in a browser, with faked Tauri IPC (`window.__TAURI_INTERNALS__`) and a real `algoexpert-sdfshortcourse` video:

- **1440 × 900:**
  - Library home, overview, video, PDF, and reading views fit the window; `scrollHeight` equals the window height.
  - In the video view, the controls, toolbar, and note composer are all visible at once.
- **860 × 600 and 760 × 560:** the contents and side-panel drawers open over the lesson; the Sketch drawer runs Excalidraw in the light theme.
- **Themes:** light and dark checked on the Library, workspace, Publish, and Network screens.
- **Screenshots:** the browser could not capture frames while a hardware-decoded video or Excalidraw canvas was on screen, so those layouts were checked with the video detached and by measuring the DOM.

## Limitations

- Publish and Network were re-themed, not redesigned.
- The theme toggle switches between light and dark. Returning to "follow the desktop" needs `zeron.theme` cleared.
- Lesson rows show durations but not per-lesson watch progress; playback positions are fetched one resource at a time.
- The Excalidraw toolbar is slightly clipped in the narrowest side-panel drawer.
- Not yet checked in the packaged WebKitGTK window: the speed menu and the native video controls.
