# Phase 10: accessibility pass

Status: complete for automated checks; manual checks are listed below. The main screens are checked with axe-core's WCAG 2.0 and 2.1 A and AA rules, plus its best-practice rules, inside the frontend test suite.

## Delivered

- **`src/test/accessibility.ts`:** `accessibilityViolations(container)` runs axe-core 4.12.1 (a dev dependency only) against a rendered screen. Colour contrast needs real layout, so it is checked in the window, not in jsdom. The `region` rule is off for isolated components.
- **Screens checked**, each in its realistic state: open releases, staged restores, catalogs expanded, suggestions shown.
  - Library home with a course, and the course workspace with the verified-publisher badge.
  - Network settings with a restart notice.
  - Courses with a release open.
  - Distribution with seeding rows.
  - Publish → Identity and Publish → Catalog.
  - Publisher configuration, the activity log, Library catalogs, backup with a pending restore, the publisher badge, and annotation moves.
- **A negative control** proves the check catches real problems: an image without `alt` and a button without a name are reported.

## Acceptance evidence

- Every screen above reports no violations. The screens use the existing patterns:
  - labelled inputs and buttons with accessible names;
  - `aria-expanded` and `aria-controls` on disclosure buttons;
  - `role="alert"` and `role="status"` for outcomes;
  - `alertdialog` confirmations that move focus to Cancel.
- All repository gates pass.

## Manual verification (pending)

1. Check colour contrast in the running app with the WebKit inspector's accessibility audit, especially the slate-500 helper text on dark panels.
2. Use Orca on Linux, VoiceOver on macOS, or Narrator on Windows to publish a release, import a course, and add a note.
3. Do the same flows with the keyboard only. The native file dialogs and the Excalidraw canvas are outside Zeron's control.

## Current limitations

- jsdom cannot check contrast, focus visibility, or reading order. Those need the manual steps above.
- The video player, the PDF.js canvas, and Excalidraw are third-party components; only the controls around them are checked.
