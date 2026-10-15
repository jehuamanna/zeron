# Phase 6: Lesson reader with code highlighting

Status: complete. Markdown and plain-text lesson resources open in a reader in place of the player. Markdown renders with tables and other GitHub-flavoured syntax, and fenced code is coloured by Shiki. Lesson text cannot run script, load images, or navigate anywhere.

## Delivered

- **Application:** `CourseLibrary::text_source(course, resource)` covers `markdown` and `text` resources and says which kind it is. Bytes go through the same verified local and torrent reads as PDFs and transcripts.
- **Command:** `read_text_resource { courseId, resourceId }` returns `{ format: "markdown" | "text", text }`.
  - Resources are limited to 2 MiB (`MAX_TEXT_BYTES`); larger ones return `document_too_large`.
  - Non-UTF-8 bytes return `text_unreadable`, and other resources return `not_a_document`.
- **Reader (`features/reader`):**
  - **Rendering:** `react-markdown` 10.1.0 with `remark-gfm` 4.0.1 renders through React elements only. `skipHtml` drops raw HTML, so no `innerHTML` is used.
  - **Links and images:** links render as text with the target in a tooltip, and images render as `[image: alt]`. Lesson text can neither navigate nor fetch.
  - **Plain text:** shown as written, wrapped.
  - **Styles:** reader typography is in `styles.css` (`.reader`).
  - **Loading:** the reader is loaded lazily, so the main bundle does not carry Markdown or Shiki.
- **Highlighting:** Shiki 4.5.0 (fine-grained `shiki/core`) with the JavaScript regex engine, because the CSP allows no WebAssembly.
  - **Theme:** `github-dark`.
  - **Grammars:** 23 are bundled, each loaded the first time a fence uses it: bash, C, C++, C#, CSS, Go, HTML, Java, JavaScript, JSON, JSX, Kotlin, Markdown, PHP, Python, Ruby, Rust, SQL, Swift, TOML, TSX, TypeScript and YAML. Common aliases (`ts`, `py`, `sh`, `c++` and others) are recognised.
  - **Rendering:** tokens render as React `<span>`s with a colour style. Code shows plain until its grammar loads, and other languages stay plain.
- **Course view:** Markdown and text resources show "Read" ("Reading" while open). Opening one closes the PDF viewer or player, and the other way round.
- **Dependencies:** all three are MIT-licensed exact pins.

## Acceptance evidence

- **Application test:** a Markdown resource reads as Markdown and a text resource as text. PDFs, transcripts, videos and unknown resources are not text resources.
- **Vitest** (real react-markdown and Shiki under jsdom):
  - A heading and a GFM table render, and inline code stays inline.
  - The document has no `<a>`, `<img>` or `<script>`, and the injected HTML never ran. The link text carries its URL as a title, and an image shows as `[image: ring]`.
  - A `ts` fence is tokenised into coloured spans with its text intact, and an unknown language stays plain.
  - Plain text is not parsed as Markdown, and a read error shows its alert.
  - Fence names map to grammars.
- **Real window** (WebKitGTK, a scratch "Reader Check" course with `guide.md` and `checklist.txt`):
  - The heading and a three-row table rendered.
  - Python code got 22 coloured tokens and Rust 39; the Rust `fn` keyword was `rgb(249, 117, 131)`.
  - The rendered guide had no anchors and no scripts, and the window title stayed "Zeron" despite a `<script>` in the Markdown.
  - The text checklist showed with its indentation.
- All repository gates pass.

## Current limitations

- Images in lesson Markdown are not displayed, because there is no path-free way to serve package images to the reader yet. Links are not clickable.
- Highlighting is limited to the bundled grammars. Code blocks in learner notes (Tiptap) are not highlighted yet.
- Lesson text is not yet part of offline search. That comes with the FTS5 slice.
