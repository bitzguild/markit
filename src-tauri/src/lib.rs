//! Markit — Rust side: windows, native menu, file I/O, export and printing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{
    AppHandle, Emitter, EventTarget, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

mod export;

const BUILTIN_THEMES: &[&str] = &["Daylight", "Sepia", "Nocturne", "Slate"];
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif", "ico", "tif", "tiff"];

#[derive(Default)]
struct AppState {
    counter: AtomicUsize,
    /// Files waiting to be opened by a window whose frontend has not started yet.
    pending: Mutex<HashMap<String, String>>,
    ready: Mutex<HashSet<String>>,
    /// `--export-pdf <out>`: render the first document to PDF, then quit.
    auto_export: Mutex<Option<String>>,
}

// ------------------------------------------------------------------ windows

fn create_window<R: Runtime>(app: &AppHandle<R>, path: Option<String>) -> tauri::Result<WebviewWindow<R>> {
    let state = app.state::<AppState>();
    let n = state.counter.fetch_add(1, Ordering::SeqCst);
    let label = if n == 0 { "main".to_string() } else { format!("doc-{n}") };
    if let Some(p) = path {
        state.pending.lock().unwrap().insert(label.clone(), p);
    }
    let offset = (n % 8) as f64 * 26.0;
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("Markit")
        .inner_size(1120.0, 800.0)
        .min_inner_size(520.0, 360.0)
        .position(120.0 + offset, 80.0 + offset)
        .build()
}

fn focused_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    let windows = app.webview_windows();
    windows
        .values()
        .find(|w| w.is_focused().unwrap_or(false))
        .cloned()
        .or_else(|| windows.get("main").cloned())
        .or_else(|| windows.values().next().cloned())
}

/// Route a file the OS asked us to open (Finder, `open -a`, CLI).
fn open_external<R: Runtime>(app: &AppHandle<R>, path: String) {
    let ready = app.state::<AppState>().ready.lock().unwrap().clone();
    match focused_window(app) {
        Some(w) if ready.contains(w.label()) => {
            let _ = app.emit_to(EventTarget::webview_window(w.label()), "open-path", path);
        }
        Some(w) => {
            let state = app.state::<AppState>();
            let busy = {
                let mut pending = state.pending.lock().unwrap();
                if pending.contains_key(w.label()) {
                    true
                } else {
                    pending.insert(w.label().to_string(), path.clone());
                    false
                }
            };
            if busy {
                let _ = create_window(app, Some(path));
            }
        }
        None => {
            let _ = create_window(app, Some(path));
        }
    }
}

#[tauri::command]
fn frontend_ready(window: WebviewWindow, state: tauri::State<AppState>) -> Option<String> {
    state.ready.lock().unwrap().insert(window.label().to_string());
    state.pending.lock().unwrap().remove(window.label())
}

#[tauri::command]
fn take_auto_export(window: WebviewWindow, state: tauri::State<AppState>) -> Option<String> {
    if window.label() != "main" {
        return None;
    }
    state.auto_export.lock().unwrap().take()
}

#[tauri::command]
fn file_size(path: String) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn new_window(app: AppHandle, path: Option<String>) -> Result<(), String> {
    create_window(&app, path).map(|_| ()).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ files

#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

#[tauri::command]
fn write_file(path: String, content: String) -> Result<(), String> {
    write_atomic(Path::new(&path), content.as_bytes())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("invalid path")?;
    let name = path.file_name().ok_or("invalid path")?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.markit-tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .map(|e| IMAGE_EXT.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Let the webview load one local image through the asset protocol.
#[tauri::command]
fn allow_asset(app: AppHandle, path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !is_image(&p) {
        return Err("not an image".into());
    }
    app.asset_protocol_scope().allow_file(&p).map_err(|e| e.to_string())
}

/// Store pasted / uploaded image bytes in `<doc dir>/assets/` and return the relative path.
#[tauri::command]
fn save_asset(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let header = |k: &str| -> Result<String, String> {
        let raw = request.headers().get(k).ok_or(format!("missing {k}"))?;
        let raw = raw.to_str().map_err(|e| e.to_string())?;
        Ok(percent_decode(raw))
    };
    let doc = PathBuf::from(header("x-doc-path")?);
    let file_name = sanitize_file_name(&header("x-file-name")?);
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw image bytes".into());
    };
    let assets = doc.parent().ok_or("document has no folder")?.join("assets");
    std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;

    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file_name.clone(), ".png".to_string()),
    };
    let mut target = assets.join(format!("{stem}{ext}"));
    let mut i = 1;
    while target.exists() {
        target = assets.join(format!("{stem}-{i}{ext}"));
        i += 1;
    }
    std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
    Ok(format!("assets/{}", target.file_name().unwrap().to_string_lossy()))
}

fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("image");
    let clean: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '-' })
        .collect();
    let clean = clean.trim_matches('.').to_string();
    if clean.is_empty() || clean == "image" {
        "image.png".into()
    } else {
        clean
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------------ themes

#[derive(Serialize)]
struct UserTheme {
    name: String,
    css: String,
}

fn themes_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?.join("themes");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

fn scan_user_themes<R: Runtime>(app: &AppHandle<R>) -> Vec<UserTheme> {
    let Some(dir) = themes_dir(app) else { return vec![] };
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut themes: Vec<UserTheme> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "css").unwrap_or(false))
        .filter_map(|p| {
            let css = std::fs::read_to_string(&p).ok()?;
            let stem = p.file_stem()?.to_string_lossy().to_string();
            Some(UserTheme { name: title_case(&stem), css })
        })
        .collect();
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

fn title_case(s: &str) -> String {
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tauri::command]
fn list_user_themes(app: AppHandle) -> Vec<UserTheme> {
    scan_user_themes(&app)
}

fn reveal_dir(dir: &Path) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

// ------------------------------------------------------------------ export & print

#[tauri::command]
fn export_html(markdown: String, title: String, css: String, path: String) -> Result<(), String> {
    let html = export::render_html_document(&markdown, &title, &css);
    write_atomic(Path::new(&path), html.as_bytes())
}

#[tauri::command]
fn render_markdown(markdown: String) -> String {
    export::render_body(&markdown)
}

#[tauri::command]
fn print_document(window: WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

/// Returns "file" when the PDF was written directly, "dialog" when the user
/// must finish via the system print dialog (non-macOS platforms).
#[tauri::command]
fn export_pdf(window: WebviewWindow, path: String) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        window
            .with_webview(move |wv| {
                let r = unsafe { export::mac::print_to_pdf(wv.inner(), wv.ns_window(), &path) };
                let _ = tx.send(r);
            })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())??;
        Ok("file".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        window.print().map_err(|e| e.to_string())?;
        Ok("dialog".into())
    }
}

// ------------------------------------------------------------------ menu

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let item = |id: &str, text: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, text);
        match accel {
            Some(a) => b.accelerator(a).build(app),
            None => b.build(app),
        }
    };

    let app_menu = SubmenuBuilder::new(app, "Markit")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let export_menu = SubmenuBuilder::new(app, "Export")
        .item(&item("export-pdf", "PDF…", Some("CmdOrCtrl+Alt+P"))?)
        .item(&item("export-html", "HTML…", None)?)
        .item(&item("export-html-plain", "HTML (without styles)…", None)?)
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new", "New", Some("CmdOrCtrl+N"))?)
        .item(&item("open", "Open…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&item("save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&item("save-as", "Save As…", Some("CmdOrCtrl+Shift+S"))?)
        .item(&item("reload", "Reload from Disk", None)?)
        .separator()
        .item(&export_menu)
        .item(&item("print", "Print…", Some("CmdOrCtrl+P"))?)
        .separator()
        .close_window()
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&item("undo", "Undo", Some("CmdOrCtrl+Z"))?)
        .item(&item("redo", "Redo", Some("CmdOrCtrl+Shift+Z"))?)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let table = SubmenuBuilder::new(app, "Table")
        .item(&item("table", "Insert Table", Some("CmdOrCtrl+Alt+T"))?)
        .separator()
        .item(&item("table-row-above", "Add Row Above", None)?)
        .item(&item("table-row-below", "Add Row Below", Some("CmdOrCtrl+Enter"))?)
        .item(&item("table-col-before", "Add Column Before", None)?)
        .item(&item("table-col-after", "Add Column After", None)?)
        .separator()
        .item(&item("table-delete-cells", "Delete Selected Rows / Columns", None)?)
        .build()?;

    let mut paragraph = SubmenuBuilder::new(app, "Paragraph");
    for level in 1..=6 {
        paragraph = paragraph.item(&item(&format!("h{level}"), &format!("Heading {level}"), Some(&format!("CmdOrCtrl+{level}")))?);
    }
    let paragraph = paragraph
        .item(&item("paragraph", "Paragraph", Some("CmdOrCtrl+0"))?)
        .separator()
        .item(&table)
        .item(&item("code-block", "Code Fence", Some("CmdOrCtrl+Alt+C"))?)
        .item(&item("math-block", "Math Block", Some("CmdOrCtrl+Alt+B"))?)
        .separator()
        .item(&item("quote", "Quote", Some("CmdOrCtrl+Alt+Q"))?)
        .item(&item("ol", "Ordered List", Some("CmdOrCtrl+Alt+O"))?)
        .item(&item("ul", "Unordered List", Some("CmdOrCtrl+Alt+U"))?)
        .item(&item("task", "Task List", Some("CmdOrCtrl+Alt+X"))?)
        .separator()
        .item(&item("hr", "Horizontal Line", None)?)
        .build()?;

    let format = SubmenuBuilder::new(app, "Format")
        .item(&item("bold", "Strong", Some("CmdOrCtrl+B"))?)
        .item(&item("italic", "Emphasis", Some("CmdOrCtrl+I"))?)
        .item(&item("code", "Code", Some("CmdOrCtrl+Shift+`"))?)
        .item(&item("strike", "Strike", Some("CmdOrCtrl+Shift+X"))?)
        .item(&item("inline-math", "Inline Math", Some("CmdOrCtrl+Shift+M"))?)
        .separator()
        .item(&item("link", "Hyperlink", Some("CmdOrCtrl+K"))?)
        .item(&item("image", "Insert Image…", Some("CmdOrCtrl+Shift+I"))?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item("source-mode", "Toggle Source Code Mode", Some("CmdOrCtrl+/"))?)
        .separator()
        .item(&item("sidebar", "Toggle Sidebar", Some("CmdOrCtrl+Shift+L"))?)
        .item(&item("status-bar", "Toggle Status Bar", None)?)
        .item(&item("focus-mode", "Focus Mode", Some("F8"))?)
        .separator()
        .item(&item("zoom-reset", "Actual Size", Some("CmdOrCtrl+Shift+0"))?)
        .item(&item("zoom-in", "Zoom In", Some("CmdOrCtrl+Shift+="))?)
        .item(&item("zoom-out", "Zoom Out", Some("CmdOrCtrl+Shift+-"))?)
        .separator()
        .fullscreen()
        .build()?;

    let mut themes = SubmenuBuilder::new(app, "Themes");
    for name in BUILTIN_THEMES {
        themes = themes.item(&item(&format!("theme:{name}"), name, None)?);
    }
    let user = scan_user_themes(app);
    if !user.is_empty() {
        themes = themes.separator();
        for t in user.iter().filter(|t| !BUILTIN_THEMES.contains(&t.name.as_str())) {
            themes = themes.item(&item(&format!("theme:{}", t.name), &t.name, None)?);
        }
    }
    let themes = themes
        .separator()
        .item(&item("open-themes-folder", "Open Theme Folder", None)?)
        .item(&item("reload-themes", "Reload Themes", None)?)
        .build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;

    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &paragraph, &format, &view, &themes, &window])
        .build()
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "new" => {
            let _ = create_window(app, None);
        }
        "open-themes-folder" => {
            if let Some(dir) = themes_dir(app) {
                reveal_dir(&dir);
            }
        }
        "reload-themes" => {
            if let Ok(menu) = build_menu(app) {
                let _ = app.set_menu(menu);
            }
            for w in app.webview_windows().values() {
                let _ = app.emit_to(EventTarget::webview_window(w.label()), "menu", id);
            }
        }
        _ if id.starts_with("theme:") => {
            // Themes apply to every open window.
            for w in app.webview_windows().values() {
                let _ = app.emit_to(EventTarget::webview_window(w.label()), "menu", id);
            }
        }
        _ => {
            if let Some(w) = focused_window(app) {
                let _ = app.emit_to(EventTarget::webview_window(w.label()), "menu", id);
            } else if id == "open" {
                let _ = create_window(app, None);
            }
        }
    }
}

// ------------------------------------------------------------------ entry

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            frontend_ready,
            take_auto_export,
            file_size,
            quit_app,
            new_window,
            read_file,
            write_file,
            allow_asset,
            save_asset,
            list_user_themes,
            export_html,
            render_markdown,
            export_pdf,
            print_document,
        ])
        .setup(|app| {
            let handle = app.handle();
            app.set_menu(build_menu(handle)?)?;
            app.on_menu_event(|app, event| on_menu(app, event.id().as_ref()));

            let mut files: Vec<String> = Vec::new();
            let mut args = std::env::args().skip(1);
            while let Some(a) = args.next() {
                if a == "--export-pdf" {
                    *app.state::<AppState>().auto_export.lock().unwrap() = args.next();
                } else if !a.starts_with('-') && Path::new(&a).is_file() {
                    files.push(a);
                }
            }
            let mut files = files.into_iter();
            create_window(handle, files.next())?;
            for f in files {
                create_window(handle, Some(f))?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Markit");

    app.run(|app, event| match event {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        RunEvent::Opened { urls } => {
            for url in urls {
                if let Ok(p) = url.to_file_path() {
                    open_external(app, p.to_string_lossy().into_owned());
                }
            }
        }
        RunEvent::WindowEvent { label, event: tauri::WindowEvent::Destroyed, .. } => {
            let state = app.state::<AppState>();
            state.ready.lock().unwrap().remove(&label);
            state.pending.lock().unwrap().remove(&label);
        }
        // Keep the app alive on macOS when the last window closes.
        #[cfg(target_os = "macos")]
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { has_visible_windows: false, .. } => {
            let _ = create_window(app, None);
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("%2FUsers%2Fme%2Fa%20b.md"), "/Users/me/a b.md");
        assert_eq!(percent_decode("caf%C3%A9.png"), "café.png");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("my shot (1).png"), "my-shot--1-.png");
        assert_eq!(sanitize_file_name(""), "image.png");
    }
}
