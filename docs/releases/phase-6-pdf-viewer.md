# Phase 6: PDF viewing and highlights

Status: complete. Lesson PDFs open in the Learner. Learners can page through them, highlight an area, and attach a note to a page or a highlight; a note's chip opens its page. Bytes reach the viewer only after verification, and no path crosses IPC.

## Delivered

- **`learning-application::documents`:**
  - `document_source(course, resource)` covers PDF resources only.
  - `read_local_document` makes a bounded read (64 MiB) and re-checks the length and the manifest SHA-256 on every read.
  - `read_torrent_document` reads through the verified engine stream (`open_lesson_stream`), within a 120 s limit.
  - `DocumentError` is one of `NotADocument`, `TooLarge`, `Changed`, `Unavailable`, or `Library`; none carries a path.
- **Learner command `read_document { courseId, resourceId }`:** returns the verified bytes as a raw binary IPC response (`tauri::ipc::Response`, no JSON or base64). Error codes are `not_a_document`, `document_too_large`, `document_changed`, `document_unavailable`, and `library_query_failed`.
  - **Why not the media gateway:** PDF.js fetches its document with `fetch`, which would need CORS headers on the HTTPS loopback gateway, a security-review change. The binary IPC response avoids touching the gateway.
- **UI:**
  - **pdf.js:** `pdfjs-dist` 6.4.299 (Apache-2.0, exact pin). `PdfCanvas` is loaded lazily, so pdf.js and its worker are separate chunks. It renders with `useWasm: false`, because the CSP allows no WebAssembly compilation; pdf.js uses no `eval`. The worker is served from the app origin.
  - **Viewer:** `DocumentViewer` has page navigation ("Page n of N") and a "Highlight area" mode. In that mode, dragging draws a normalised rectangle, which stays pending until a note is added or the learner clears it. Saved highlights for the page are drawn over it.
  - **Course view:** PDF resources show "Open PDF", and the viewer takes the player's place.
  - **Notes panel:** it offers "Add note on page N", or "Add note on highlight" when an area is pending. Page chips ("Page 2, highlight") open the PDF at that page.

## Acceptance evidence

- **Application tests:**
  - A local PDF is readable while it matches its course; it is refused once changed, missing, or too large.
  - Non-PDF and unknown resources are not documents.
  - A torrent file is read whole through the verified stream from a seeder, and a size over the limit is refused.
- **Command test:** error codes, and no path in library errors.
- **Vitest:**
  - The viewer loads bytes by course and resource, pages forward and back, and shows the page count.
  - Highlight mode reports the drawn region, then turns off; a pending region is drawn and can be cleared.
  - A changed-document error is shown.
  - The notes panel adds a note on a highlight with the region anchor, and a page chip opens its page.
- **Real window** (WebKitGTK with real pdf.js), on a generated three-page PDF imported into the scratch library as a local course:
  - The page rendered (about 19,000 coloured pixels in the sampled canvas), and "Page 1 of 3" → "Page 2 of 3".
  - An area drawn on page 2 got the note "Ring with virtual nodes"; one highlight showed on page 2 and none on page 1.
  - The "Page 2, highlight" chip returned to page 2 with its highlight.
  - After restarting the Learner, the note's chip opened page 2 again with the highlight.
- All repository gates pass.

## Current limitations

- There is no text layer yet, so PDF text cannot be selected or copied. Standard fonts not embedded in a PDF use pdf.js's fallback, because the standard font data is not bundled.
- Reading a PDF does not count toward lesson progress.
- Documents over 64 MiB are refused.
