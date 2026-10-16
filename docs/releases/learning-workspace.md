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

## Acceptance evidence

- **Vitest:** all 176 tests pass, 3 of them new.
  - The workspace opens the next lesson from the overview, shows "Lesson 1 of 2", steps to the next lesson, hides and shows contents with `[`, starts a timestamped note with `N`, and has no axe violations.
  - A note request starts a draft once, reports it handled, and Ctrl+Enter saves it.
  - The theme toggle switches `data-theme` and saves the choice.
- The existing Library, player, PDF, notes, transcript, and drawing tests pass unchanged, apart from the new optional props.
- The repository gates pass: `cargo fmt`, `clippy`, `cargo test`, `pnpm test`, `pnpm typecheck`, `pnpm build`, and `git diff --check`.

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
