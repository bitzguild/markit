import { Crepe } from "@milkdown/crepe";
import { remarkStringifyOptionsCtx } from "@milkdown/kit/core";
import "@milkdown/crepe/theme/common/style.css";
import { callCommand, insert, replaceAll } from "@milkdown/kit/utils";
import {
  createCodeBlockCommand,
  insertHrCommand,
  insertImageCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  toggleLinkCommand,
  toggleStrongCommand,
  turnIntoTextCommand,
  wrapInBlockquoteCommand,
  wrapInBulletListCommand,
  wrapInHeadingCommand,
  wrapInOrderedListCommand,
} from "@milkdown/kit/preset/commonmark";
import {
  addColAfterCommand,
  addColBeforeCommand,
  addRowAfterCommand,
  addRowBeforeCommand,
  deleteSelectedCellsCommand,
  insertTableCommand,
  toggleStrikethroughCommand,
} from "@milkdown/kit/preset/gfm";
import { redoCommand, undoCommand } from "@milkdown/kit/plugin/history";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ask, message, open, save } from "@tauri-apps/plugin-dialog";
import katex from "katex";
import "katex/dist/katex.min.css";

import "./doc.css";
import "./app.css";
import docCss from "./doc.css?inline";
import daylight from "./themes/daylight.css?inline";
import sepia from "./themes/sepia.css?inline";
import nocturne from "./themes/nocturne.css?inline";
import slate from "./themes/slate.css?inline";
import gothic from "./themes/gothic.css?inline";

// ---------------------------------------------------------------- state

const win = getCurrentWebviewWindow();
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const editorRoot = $<HTMLDivElement>("editor");
const sourceEl = $<HTMLTextAreaElement>("source");

let crepe: Crepe | null = null;
let docPath: string | null = null;
let savedMarkdown = "";
let dirty = false;
let sourceMode = false;
let zoom = 1;
let closing = false;

const MD_EXT = ["md", "markdown", "mdown", "mkd", "txt"];
const IMG_EXT = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];

// ---------------------------------------------------------------- paths

const norm = (p: string) => p.replace(/\\/g, "/");
const dirname = (p: string) => norm(p).replace(/\/[^/]*$/, "") || "/";
const basename = (p: string) => norm(p).split("/").pop() || p;
const extname = (p: string) => (basename(p).match(/\.([^.]+)$/)?.[1] ?? "").toLowerCase();
const isAbsolute = (p: string) => p.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(p);

function joinPath(dir: string, rel: string): string {
  const parts = norm(dir).split("/");
  for (const seg of norm(rel).split("/")) {
    if (seg === "..") parts.pop();
    else if (seg !== "." && seg !== "") parts.push(seg);
  }
  return parts.join("/") || "/";
}

function relativePath(fromDir: string, to: string): string {
  const a = norm(fromDir).split("/").filter(Boolean);
  const b = norm(to).split("/").filter(Boolean);
  if (a[0] !== b[0] && /:$/.test(a[0] ?? "")) return norm(to); // different drives
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  return [...Array(a.length - i).fill(".."), ...b.slice(i)].join("/");
}

const encodeSrc = (p: string) => p.split("/").map(encodeURIComponent).join("/").replace(/%3A/gi, ":");

// ---------------------------------------------------------------- images

/** Map a Markdown image src to something the webview can display. */
async function proxyImageUrl(url: string): Promise<string> {
  if (/^(https?:|data:|blob:|asset:)/i.test(url)) return url;
  let p = url.replace(/^file:\/\//i, "");
  try { p = decodeURI(p); } catch { /* keep raw */ }
  if (!isAbsolute(p)) {
    if (!docPath) return url;
    p = joinPath(dirname(docPath), p);
  }
  try {
    await invoke("allow_asset", { path: p });
  } catch {
    return url;
  }
  return convertFileSrc(p);
}

/** Persist an uploaded/pasted image next to the document; returns the Markdown src. */
async function storeImage(file: File): Promise<string> {
  if (!docPath) {
    return await new Promise<string>((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result));
      r.onerror = () => reject(r.error);
      r.readAsDataURL(file);
    });
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  const rel = await invoke<string>("save_asset", bytes, {
    headers: {
      "x-doc-path": encodeURIComponent(docPath),
      "x-file-name": encodeURIComponent(file.name || `image.${file.type.split("/")[1] ?? "png"}`),
    },
  });
  return encodeSrc(rel);
}

function insertImageFromPath(path: string) {
  const src = docPath ? encodeSrc(relativePath(dirname(docPath), path)) : encodeSrc(norm(path));
  run(insertImageCommand.key, { src, alt: basename(path).replace(/\.[^.]+$/, "") });
}

// ---------------------------------------------------------------- editor

async function createEditor(markdown: string) {
  if (crepe) await crepe.destroy();
  editorRoot.innerHTML = "";
  crepe = new Crepe({
    root: editorRoot,
    defaultValue: markdown,
    featureConfigs: {
      [Crepe.Feature.ImageBlock]: {
        proxyDomURL: proxyImageUrl,
        onUpload: storeImage,
        inlineOnUpload: storeImage,
        blockOnUpload: storeImage,
      },
      [Crepe.Feature.Placeholder]: { text: "Start writing…", mode: "doc" },
    },
  });
  crepe.editor.config((ctx) => {
    ctx.update(remarkStringifyOptionsCtx, (prev) => ({ ...prev, bullet: "-" as const, rule: "-" as const, emphasis: "*" as const, strong: "*" as const, fence: "`" as const }));
  });
  crepe.on((l) => {
    l.markdownUpdated((_ctx, md) => {
      setDirty(md !== savedMarkdown);
      scheduleRefresh();
    });
  });
  await crepe.create();
  scheduleRefresh();
}

function currentMarkdown(): string {
  if (sourceMode) return sourceEl.value;
  return crepe ? crepe.getMarkdown() : "";
}

function run<T>(key: unknown, payload?: T) {
  if (!crepe || sourceMode) return;
  crepe.editor.action(callCommand(key as never, payload as never));
}

function insertMarkdown(md: string) {
  if (!crepe || sourceMode) return;
  crepe.editor.action(insert(md));
}

async function setSourceMode(on: boolean) {
  if (on === sourceMode) return;
  if (on) {
    sourceEl.value = currentMarkdown();
    sourceMode = true;
    editorRoot.classList.add("hidden");
    sourceEl.classList.remove("hidden");
    sourceEl.focus();
  } else {
    const md = sourceEl.value;
    sourceMode = false;
    sourceEl.classList.add("hidden");
    editorRoot.classList.remove("hidden");
    if (crepe) crepe.editor.action(replaceAll(md));
    else await createEditor(md);
    setDirty(md !== savedMarkdown);
  }
  $("status-mode").textContent = sourceMode ? "Source" : "";
  scheduleRefresh();
}

sourceEl.addEventListener("input", () => {
  setDirty(sourceEl.value !== savedMarkdown);
  scheduleRefresh();
});
sourceEl.addEventListener("keydown", (e) => {
  if (e.key === "Tab") {
    e.preventDefault();
    const { selectionStart: s, selectionEnd: t } = sourceEl;
    sourceEl.setRangeText("    ", s, t, "end");
    sourceEl.dispatchEvent(new Event("input"));
  }
});

// Paste images from the clipboard (capture phase so it runs before ProseMirror).
editorRoot.addEventListener(
  "paste",
  async (e) => {
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length) return;
    e.preventDefault();
    e.stopPropagation();
    for (const f of files) {
      const src = await storeImage(f);
      run(insertImageCommand.key, { src, alt: f.name.replace(/\.[^.]+$/, "") });
    }
  },
  true,
);

// ---------------------------------------------------------------- outline, counts, focus

let refreshTimer = 0;
function scheduleRefresh() {
  clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(refreshPanels, 150);
}

function refreshPanels() {
  const outline = $("outline");
  outline.innerHTML = "";
  if (sourceMode) {
    const lines = sourceEl.value.split("\n");
    let inFence = false;
    lines.forEach((line, idx) => {
      if (/^\s*(```|~~~)/.test(line)) inFence = !inFence;
      const m = !inFence && line.match(/^(#{1,6})\s+(.*)$/);
      if (m) addOutlineItem(outline, m[1].length, m[2], () => jumpToSourceLine(idx));
    });
  } else {
    editorRoot.querySelectorAll<HTMLElement>(".ProseMirror > h1, .ProseMirror > h2, .ProseMirror > h3, .ProseMirror > h4, .ProseMirror > h5, .ProseMirror > h6")
      .forEach((h) => addOutlineItem(outline, Number(h.tagName[1]), h.textContent ?? "", () => h.scrollIntoView({ behavior: "smooth", block: "start" })));
  }
  if (!outline.children.length) outline.innerHTML = `<div class="empty">No headings</div>`;

  const text = sourceMode ? sourceEl.value : (editorRoot.querySelector(".ProseMirror") as HTMLElement | null)?.innerText ?? "";
  const words = (text.match(/[\p{L}\p{N}][\p{L}\p{N}'’_-]*/gu) ?? []).length;
  $("status-count").textContent = `${words.toLocaleString()} words · ${text.length.toLocaleString()} chars`;
}

function addOutlineItem(parent: HTMLElement, level: number, text: string, onClick: () => void) {
  const a = document.createElement("a");
  a.dataset.level = String(level);
  a.textContent = text.trim() || "(untitled)";
  a.title = a.textContent;
  a.onclick = onClick;
  parent.appendChild(a);
}

function jumpToSourceLine(line: number) {
  const pos = sourceEl.value.split("\n").slice(0, line).join("\n").length + (line ? 1 : 0);
  sourceEl.focus();
  sourceEl.setSelectionRange(pos, pos);
  const lh = parseFloat(getComputedStyle(sourceEl).lineHeight) || 22;
  $("scroller").scrollTop = line * lh;
}

document.addEventListener("selectionchange", () => {
  if (!document.body.classList.contains("focus-mode")) return;
  const root = editorRoot.querySelector(".ProseMirror");
  let node = document.getSelection()?.anchorNode ?? null;
  while (node && node.parentNode !== root) node = node.parentNode;
  root?.querySelectorAll(".md-focused").forEach((el) => el !== node && el.classList.remove("md-focused"));
  if (node instanceof HTMLElement) node.classList.add("md-focused");
});

// ---------------------------------------------------------------- title, toast, recent

function setDirty(v: boolean) {
  dirty = v;
  updateTitle();
}

function updateTitle() {
  const name = docPath ? basename(docPath) : "Untitled";
  win.setTitle(`${dirty ? "• " : ""}${name} — Markit`);
  $("status-file").textContent = docPath ?? "Untitled (not saved)";
}

let toastTimer = 0;
function toast(msg: string) {
  const t = $("toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => t.classList.remove("show"), 2600);
}

function getRecent(): string[] {
  try { return JSON.parse(localStorage.getItem("recent") ?? "[]"); } catch { return []; }
}
function addRecent(path: string) {
  const list = [path, ...getRecent().filter((p) => p !== path)].slice(0, 15);
  try { localStorage.setItem("recent", JSON.stringify(list)); } catch { /* ignore */ }
  renderRecent();
}
function renderRecent() {
  const el = $("recent");
  el.innerHTML = "";
  const list = getRecent();
  if (!list.length) el.innerHTML = `<div class="empty">No recent files</div>`;
  for (const p of list) {
    const a = document.createElement("a");
    a.innerHTML = `${escapeHtml(basename(p))}<small>${escapeHtml(dirname(p))}</small>`;
    a.title = p;
    a.onclick = () => openPath(p);
    el.appendChild(a);
  }
}
const escapeHtml = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

document.querySelectorAll<HTMLButtonElement>(".side-tabs button").forEach((b) => {
  b.onclick = () => {
    document.querySelectorAll(".side-tabs button").forEach((x) => x.classList.toggle("active", x === b));
    document.querySelectorAll(".side-panel").forEach((p) => p.classList.toggle("hidden", p.id !== b.dataset.tab));
  };
});

// ---------------------------------------------------------------- files

async function openPath(path: string) {
  // Keep unsaved work: open into a new window unless this one is an untouched blank doc.
  if (dirty || (docPath && docPath !== path)) {
    await invoke("new_window", { path });
    return;
  }
  try {
    const content = await invoke<string>("read_file", { path });
    docPath = path;
    savedMarkdown = content;
    if (sourceMode) {
      sourceEl.value = content;
    } else {
      await createEditor(content);
    }
    setDirty(false);
    addRecent(path);
  } catch (err) {
    await message(`Could not open ${path}\n\n${err}`, { title: "Open failed", kind: "error" });
  }
}

async function openDialog() {
  const picked = await open({
    multiple: true,
    filters: [{ name: "Markdown", extensions: MD_EXT }, { name: "All files", extensions: ["*"] }],
  });
  if (!picked) return;
  for (const p of Array.isArray(picked) ? picked : [picked]) await openPath(p);
}

async function saveDoc(forceDialog = false): Promise<boolean> {
  let path = docPath;
  if (!path || forceDialog) {
    path = await save({
      defaultPath: docPath ?? "Untitled.md",
      filters: [{ name: "Markdown", extensions: ["md", "markdown"] }],
    });
    if (!path) return false;
  }
  const md = currentMarkdown();
  try {
    await invoke("write_file", { path, content: md });
  } catch (err) {
    await message(`Could not save ${path}\n\n${err}`, { title: "Save failed", kind: "error" });
    return false;
  }
  const moved = path !== docPath;
  docPath = path;
  savedMarkdown = md;
  setDirty(false);
  addRecent(path);
  if (moved && !sourceMode) await createEditor(md); // re-resolve relative image paths
  toast("Saved");
  return true;
}

async function confirmDiscard(): Promise<boolean> {
  if (!dirty) return true;
  return await ask(`"${docPath ? basename(docPath) : "Untitled"}" has unsaved changes. Discard them?`, {
    title: "Unsaved changes",
    kind: "warning",
    okLabel: "Discard",
    cancelLabel: "Cancel",
  });
}

win.onCloseRequested(async (e) => {
  if (closing) return;
  if (!(await confirmDiscard())) e.preventDefault();
  else closing = true;
});

// ---------------------------------------------------------------- export & print

const exportBase = () => (docPath ? docPath.replace(/\.[^./\\]+$/, "") : "Untitled");

/*
 * Print / PDF use a static rendering (comrak HTML, same as HTML export) instead of
 * the live editor: Crepe's widgets (lazy CodeMirror, math previews, handles) do not
 * print reliably. #print-view is visible only under print media.
 */
const printView = document.createElement("div");
printView.id = "print-view";
printView.className = "md-static";
document.body.appendChild(printView);

async function buildPrintView() {
  const inner = printView;
  printView.classList.toggle("print-light", /color-scheme:\s*dark/.test(themeCss(currentTheme)));
  inner.innerHTML = await invoke<string>("render_markdown", { markdown: currentMarkdown() });
  inner.querySelectorAll<HTMLElement>("[data-math-style]").forEach((el) => {
    try {
      katex.render(el.textContent ?? "", el, { displayMode: el.dataset.mathStyle === "display", throwOnError: false });
    } catch { /* leave source text */ }
  });
  const imgs = [...inner.querySelectorAll<HTMLImageElement>("img")];
  await Promise.all(imgs.map(async (img) => {
    const src = img.getAttribute("src");
    if (src) img.src = await proxyImageUrl(src);
    if (!img.complete) await new Promise((r) => { img.onload = img.onerror = r; });
  }));
  // KaTeX loads glyph fonts on first layout; give them a frame to be requested, then wait.
  await new Promise((r) => requestAnimationFrame(() => r(null)));
  await document.fonts.ready;
}

async function withPrintView<T>(fn: () => Promise<T>): Promise<T> {
  await buildPrintView();
  return await fn();
}

async function exportHtml(withTheme = true) {
  const path = await save({ defaultPath: `${exportBase()}.html`, filters: [{ name: "HTML", extensions: ["html", "htm"] }] });
  if (!path) return;
  const css = withTheme ? `${docCss}\n${themeCss(currentTheme)}` : "";
  try {
    await invoke("export_html", {
      markdown: currentMarkdown(),
      title: docPath ? basename(docPath).replace(/\.[^.]+$/, "") : "Untitled",
      css,
      path,
    });
    toast(`Exported ${basename(path)}`);
  } catch (err) {
    await message(String(err), { title: "Export failed", kind: "error" });
  }
}

async function exportPdf() {
  const path = await save({ defaultPath: `${exportBase()}.pdf`, filters: [{ name: "PDF", extensions: ["pdf"] }] });
  if (!path) return;
  await withPrintView(async () => {
    document.body.classList.add("exporting");
    try {
      const mode = await invoke<string>("export_pdf", { path });
      if (mode === "dialog") toast("Choose “Save as PDF” in the print dialog");
      else toast(`Exported ${basename(path)}`);
    } catch (err) {
      await message(String(err), { title: "PDF export failed", kind: "error" });
    } finally {
      document.body.classList.remove("exporting");
    }
  });
}

async function printDoc() {
  await withPrintView(() => invoke("print_document"));
}

// ---------------------------------------------------------------- themes

type Theme = { name: string; css: string; builtin: boolean };
const builtinThemes: Theme[] = [
  { name: "Daylight", css: daylight, builtin: true },
  { name: "Sepia", css: sepia, builtin: true },
  { name: "Nocturne", css: nocturne, builtin: true },
  { name: "Slate", css: slate, builtin: true },
  { name: "Gothic", css: gothic, builtin: true },
];
let themes: Theme[] = [...builtinThemes];
let currentTheme = "Daylight";

function themeCss(name: string) {
  return themes.find((t) => t.name === name)?.css ?? daylight;
}

function applyTheme(name: string) {
  if (!themes.some((t) => t.name === name)) name = "Daylight";
  currentTheme = name;
  const el = $("theme");
  el.textContent = themeCss(name);
  document.head.appendChild(el); // keep theme rules after the base stylesheets so they can override layout
  try { localStorage.setItem("theme", name); } catch { /* ignore */ }
}

async function loadUserThemes() {
  try {
    const user = await invoke<{ name: string; css: string }[]>("list_user_themes");
    themes = [...builtinThemes, ...user.filter((u) => !builtinThemes.some((b) => b.name === u.name)).map((u) => ({ ...u, builtin: false }))];
  } catch {
    themes = [...builtinThemes];
  }
}

// ---------------------------------------------------------------- menu dispatch

function setZoom(z: number) {
  zoom = Math.min(2.5, Math.max(0.5, Math.round(z * 10) / 10));
  ($("scroller").style as CSSStyleDeclaration & { zoom: string }).zoom = String(zoom);
}

async function handleMenu(id: string) {
  if (id.startsWith("theme:")) return applyTheme(id.slice(6));
  const heading = id.match(/^h([1-6])$/);
  if (heading) return run(wrapInHeadingCommand.key, Number(heading[1]));

  switch (id) {
    case "new": return invoke("new_window", { path: null });
    case "open": return openDialog();
    case "save": return saveDoc();
    case "save-as": return saveDoc(true);
    case "reload": if (docPath && (await confirmDiscard())) { const p = docPath; dirty = false; docPath = null; return openPath(p); } return;
    case "export-html": return exportHtml(true);
    case "export-html-plain": return exportHtml(false);
    case "export-pdf": return exportPdf();
    case "print": return printDoc();

    case "undo": if (sourceMode) return void document.execCommand("undo"); return run(undoCommand.key);
    case "redo": if (sourceMode) return void document.execCommand("redo"); return run(redoCommand.key);

    case "paragraph": return run(turnIntoTextCommand.key);
    case "quote": return run(wrapInBlockquoteCommand.key);
    case "ul": return run(wrapInBulletListCommand.key);
    case "ol": return run(wrapInOrderedListCommand.key);
    case "task": return insertMarkdown("- [ ] ");
    case "code-block": return run(createCodeBlockCommand.key);
    case "math-block": return insertMarkdown("$$\n\n$$");
    case "table": return run(insertTableCommand.key, { row: 3, col: 3 });
    case "table-row-above": return run(addRowBeforeCommand.key);
    case "table-row-below": return run(addRowAfterCommand.key);
    case "table-col-before": return run(addColBeforeCommand.key);
    case "table-col-after": return run(addColAfterCommand.key);
    case "table-delete-cells": return run(deleteSelectedCellsCommand.key);
    case "hr": return run(insertHrCommand.key);

    case "bold": return run(toggleStrongCommand.key);
    case "italic": return run(toggleEmphasisCommand.key);
    case "strike": return run(toggleStrikethroughCommand.key);
    case "code": return run(toggleInlineCodeCommand.key);
    case "inline-math": return insertMarkdown("$x$");
    case "link": return run(toggleLinkCommand.key, { href: "" });
    case "image": {
      const picked = await open({ multiple: true, filters: [{ name: "Images", extensions: IMG_EXT }] });
      if (!picked) return;
      for (const p of Array.isArray(picked) ? picked : [picked]) insertImageFromPath(p);
      return;
    }

    case "source-mode": return setSourceMode(!sourceMode);
    case "sidebar": return void document.body.classList.toggle("no-sidebar");
    case "status-bar": return void document.body.classList.toggle("no-status");
    case "focus-mode": return void document.body.classList.toggle("focus-mode");
    case "zoom-in": return setZoom(zoom + 0.1);
    case "zoom-out": return setZoom(zoom - 0.1);
    case "zoom-reset": return setZoom(1);

    case "reload-themes": await loadUserThemes(); applyTheme(currentTheme); return toast(`${themes.length} themes available`);
  }
}

// ---------------------------------------------------------------- boot

async function boot() {
  await loadUserThemes();
  let saved = "Daylight";
  try { saved = localStorage.getItem("theme") ?? saved; } catch { /* ignore */ }
  applyTheme(saved);
  renderRecent();
  updateTitle();

  await win.listen<string>("menu", (e) => void handleMenu(e.payload));
  await win.listen<string>("open-path", (e) => void openPath(e.payload));

  await win.onDragDropEvent(async (e) => {
    if (e.payload.type !== "drop") return;
    for (const p of e.payload.paths) {
      const ext = extname(p);
      if (IMG_EXT.includes(ext)) insertImageFromPath(p);
      else if (MD_EXT.includes(ext)) await openPath(p);
    }
  });

  const pending = await invoke<string | null>("frontend_ready");
  if (pending) await openPath(pending);
  else await createEditor("");

  const autoPdf = await invoke<string | null>("take_auto_export");
  if (autoPdf) await headlessPdfExport(autoPdf);
}

/** `markit doc.md --export-pdf out.pdf`: wait for layout, export, wait for the file, quit. */
async function headlessPdfExport(path: string) {
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  await sleep(300);
  try {
    await withPrintView(() => invoke("export_pdf", { path }));
    let last = -1;
    for (let i = 0; i < 60; i++) {
      await sleep(250);
      const size = await invoke<number | null>("file_size", { path });
      if (size && size === last) break;
      last = size ?? -1;
    }
  } finally {
    await invoke("quit_app");
  }
}

boot().catch((err) => {
  console.error(err);
  void message(String(err), { title: "Markit failed to start", kind: "error" });
});
