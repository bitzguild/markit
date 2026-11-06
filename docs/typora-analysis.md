# Typora 1.14.15 (macOS) — reverse-engineering notes

Purpose: capture Typora's *feature set and architecture* so Markit can offer
equivalent functionality in an original Rust/Tauri implementation. No Typora
code, CSS, fonts or documents are copied into this repository.

## Method

| Step | Tool | Evidence |
|---|---|---|
| Bundle layout | `ls` of `/Applications/Typora.app/Contents` | observed |
| JS application graph of `TypeMark/appsrc/main.js` + `index.html` | REA `analyze_javascript_application` (summary, scoped) | `ev_0e7ac848…fd88` (parent), `ev_a98e9ca6…fd88` (summary view) |
| Menu / command inventory | `Base.lproj/Menu.strings` (301 entries) | observed |
| Editor DOM vocabulary, export targets | literal string frequency in `main.js` | observed (static; not runtime) |

A whole-tree REA run (30 MB incl. MathJax/Mermaid) exceeded the client tool
deadline; the scoped run covered the 1.7 MB editor bundle. REA reports the
bundle is a single minified script (0 webpack modules, 0 Electron IPC), parsed
inertly — nothing was executed.

## Architecture (observed)

- **Native shell**: Cocoa app (`MacOS/Typora`, Sparkle updater, nibs for
  MainMenu/Document/Preferences). Hosts a WKWebView. *Not* Electron on macOS.
- **Editor engine "TypeMark"**: `TypeMark/index.html` loads `appsrc/main.js`
  (≈1.7 MB) plus MathJax 4 (`tex-svg-full`). Vendored: CodeMirror modes (code
  fences), Mermaid + flowchart/sequence (`lib/diagram`), ripgrep binaries
  (`lib/bin/{arm,x64}/rg`, used for folder search).
- **Live-render WYSIWYG**: one editable `#write` root; each Markdown block is a
  DOM node tagged `md-*` (`md-fences`, `md-math-block`, `md-table`,
  `md-htmlblock-container`, `md-meta-block` …). Inline syntax markers are kept
  in `md-meta` spans and revealed only when the caret enters the span
  (`md-expand`, `md-focus`). Source Code Mode swaps to a CodeMirror view of the
  raw text.
- **Themes**: plain CSS files in `style/themes/*.css` (github, newsprint, night,
  pixyll, whitey, gothic) with sibling asset folders (fonts, dark CodeMirror
  overrides). Selectors target `#write` and the `md-*` classes. User themes
  are dropped into a themes folder; the Themes menu lists them.
- **Export**: native PDF; HTML / HTML-without-styles; image; and docx, epub,
  latex, rst, textile, opml, mediawiki… via external **pandoc**.

## Feature inventory (from the menu)

**File** New, New Window/Tab, Open, Open Folder, Open Quickly, Open Recent,
Reopen Closed File, Save, Save As, Save All, Reveal in Finder, Reload from Disk,
Reopen with Encoding, Line Endings (LF/CRLF), Import, Export (+ settings,
"export with previous"), Page Setup, Print, Browse All Versions.

**Edit** Undo/Redo, Cut/Copy/Paste, Copy as Markdown / HTML / Plain Text /
Image, Paste as Plain Text, Select & Delete (word, line, block, styled scope),
Find / Replace, Jump To (top, bottom, selection), Math tools, Image tools,
Spelling, Smart punctuation, Emoji & Symbols.

**Paragraph** Heading 1–6, Paragraph, Increase/Decrease heading level, Table
(insert, add/delete/move rows & columns, copy, delete), Math Block, Code
Fences, HTML Block, Quote, Callouts (Note, Tip, Important, Warning, Caution),
Ordered / Unordered / Task list (toggle status), Indent / Outdent, Insert
paragraph before/after, Link Reference, Footnotes, Horizontal Line, Table of
Contents, YAML Front Matter.

**Format** Strong, Emphasis, Underline, Code, Strike, Highlight, Superscript,
Subscript, Comment, Inline Math, Hyperlink, Image (insert, local images,
upload), Clear Format.

**View** Source Code Mode, Sidebar (File Tree / Articles / Outline), Outline
(collapsible/flat), Focus Mode, Typewriter Mode, Status Bar, Word Count,
Toolbar, Actual Size / Zoom In / Zoom Out, Always on Top, Full Screen.

**Themes** list of CSS themes.

## Markit scope mapping

| Typora feature | Markit v0.1 | Notes |
|---|---|---|
| Live WYSIWYG Markdown | ✅ Milkdown (ProseMirror + remark) | design choice: off-the-shelf engine, not a TypeMark clone |
| Headings, emphasis, strike, code, links, quotes, lists, tasks, HR | ✅ | GFM |
| Tables with row/column editing | ✅ | GFM tables + table toolbar |
| Code fences with highlighting | ✅ | CodeMirror via Crepe |
| Inline + block math | ✅ | KaTeX (Typora uses MathJax) |
| Embedded images (local, relative, URL, paste/drop) | ✅ | relative paths resolved against the document folder |
| Source Code Mode | ✅ | plain textarea toggle |
| Selectable themes | ✅ | 4 original themes + user CSS folder |
| Outline sidebar, word count, focus/typewriter | ✅ outline + word count; ⏳ focus/typewriter | |
| Open / Save / Save As / recent, dirty-state tracking | ✅ | Rust commands |
| Print | ✅ | webview print dialog |
| PDF export | ✅ | macOS: paginated PDF via WebKit print operation; other OS: print → PDF |
| HTML export | ✅ | Rust `comrak` render + inlined theme CSS |
| Pandoc formats, Mermaid diagrams, file tree, image upload, versions | ⏳ later | |
