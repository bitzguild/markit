# Markit

A live-rendering (WYSIWYG) Markdown editor built with **Rust + Tauri 2**. Its feature set is modelled on
Typora (see [docs/typora-analysis.md](docs/typora-analysis.md) for the reverse-engineering notes), but it's
an original implementation: no Typora code, styles or assets are used.

## Features

- **Live editing**: Markdown renders as you type (Milkdown / ProseMirror). Toggle **Source Code Mode** with `⌘/`.
- **Type styles**: headings 1–6, strong, emphasis, strikethrough, inline code, links, blockquotes,
  ordered/unordered/task lists, and horizontal rules.
- **Tables**: GFM tables with column alignment. Insert one with `⌥⌘T`, add rows and columns from the
  Paragraph ▸ Table menu, or use the in-editor table handles.
- **Code fences**: CodeMirror editing with syntax highlighting and a language picker.
- **Math**: inline `$…$` and block `$$…$$`, rendered with KaTeX.
- **Images**: relative and absolute local paths, URLs, drag-and-drop, Insert Image (`⇧⌘I`), and paste
  from the clipboard. Pasted and uploaded images are saved to `assets/` next to the document.
- **Themes**: Daylight, Sepia, Nocturne (dark), Slate and Gothic are built in. To add your own, drop `.css` files
  into the theme folder (Themes ▸ Open Theme Folder), then choose Themes ▸ Reload Themes.
- **Print** (`⌘P`) and **PDF export** (`⌥⌘P`). On macOS the PDF is paginated and written directly by
  WebKit; on other platforms you finish through the print dialog. Dark themes print in a light palette.
- **HTML export**, with or without the theme styles (rendered in Rust by `comrak`).
- **Sidebar**: an outline (click to jump) and recent files. The status bar shows word and character counts.
- **Focus Mode** (`F8`), zoom, unsaved-changes prompts, multiple windows, and Finder "Open With" for `.md` files.

### Command line

```bash
markit notes.md                          # open a file
markit notes.md --export-pdf notes.pdf   # render to PDF and quit (macOS)
```

## Writing a theme

A theme mostly sets CSS variables. It can also add layout rules that target
`:is(.milkdown .ProseMirror, .md-static)` (see [src/themes/gothic.css](src/themes/gothic.css)). The shared document stylesheet ([src/doc.css](src/doc.css)) does the
rest, so themes apply the same way in the editor, in print/PDF, and in HTML export.

```css
:root {
  --md-bg: #fff; --md-fg: #222; --md-heading: #111; --md-muted: #666;
  --md-surface: #f5f5f5; --md-surface-low: #eee; --md-border: #ddd;
  --md-accent: #3b82f6; --md-link: #0969da; --md-code-fg: #b42318; --md-code-bg: #f3f4f6;
  --md-hover: #eee; --md-selection: #cfe3ff;
  --md-font-body: Georgia, serif; --md-font-heading: var(--md-font-body); --md-font-mono: Menlo, monospace;
  --chrome-bg: #f7f7f8; --chrome-fg: #4b5563; --chrome-border: #e5e7eb;
  /* optional: --md-font-size, --md-line-height, --md-measure, --md-quote-bg, --md-table-stripe, --md-mark */
  /* add `color-scheme: dark;` for dark themes (print then switches to a light palette) */
}
```

## Development

Prerequisites: Rust (stable), Node 20+, and Xcode command-line tools on macOS.

```bash
npm install
```

```bash
npx tauri dev
```

```bash
npm run dist
```

That command builds `src-tauri/target/release/bundle/macos/Markit.app` and a DMG in
`src-tauri/target/release/bundle/dmg/`. The app is ad-hoc signed, so it runs on the machine that
built it. To distribute it to other Macs, sign it with a Developer ID and notarize it.

Tests:

```bash
cd src-tauri && cargo test
```

To work on the UI in a plain browser, run `npm run dev` and open `http://localhost:1420/harness.html`.
The harness stubs Tauri's IPC and loads [examples/showcase.md](examples/showcase.md).

## Architecture

| Layer | Responsibility |
|---|---|
| `src/main.ts` | Editor lifecycle (Milkdown Crepe), menu dispatch, open/save, source mode, outline, themes, image handling, print view |
| `src/doc.css`, `src/themes/*.css` | Document typography (shared by editor and export) and theme variables |
| `src-tauri/src/lib.rs` | Windows, native menu, file I/O (atomic writes), asset-protocol scoping for images, user themes |
| `src-tauri/src/export.rs` | `comrak` Markdown → HTML; macOS paginated PDF via `WKWebView.printOperation` |

Print, PDF and HTML export all use the same static render (comrak HTML + KaTeX), not the live editor DOM.
That keeps the three outputs consistent.

## Known limitations

- Saving re-serializes the document, so unusual Markdown formatting may be normalized.
  Bullets (`-`), rules (`---`) and emphasis markers are kept as-is.
- Not implemented yet: find/replace, Mermaid diagrams, Pandoc formats (docx/epub/…), folder file tree,
  typewriter mode, and YAML front-matter editing. Front matter is preserved, but not shown specially.
