//! HTML rendering (comrak) and macOS paginated PDF output (WebKit print operation).

use comrak::{markdown_to_html, Options};

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.superscript = true;
    o.extension.math_dollars = true;
    o.extension.front_matter_delimiter = Some("---".into());
    o.render.r#unsafe = true; // documents are the user's own; keep inline HTML
    o
}

pub fn render_body(markdown: &str) -> String {
    markdown_to_html(markdown, &options())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn render_html_document(markdown: &str, title: &str, css: &str) -> String {
    let body = render_body(markdown);
    let has_math = body.contains("data-math-style");
    let math = if has_math {
        r#"
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>
<script>
document.addEventListener("DOMContentLoaded", function () {
  if (!window.katex) return;
  document.querySelectorAll("[data-math-style]").forEach(function (el) {
    try {
      katex.render(el.textContent, el, { displayMode: el.dataset.mathStyle === "display", throwOnError: false });
    } catch (e) {}
  });
});
</script>"#
    } else {
        ""
    };
    let style = if css.is_empty() {
        String::new()
    } else {
        format!("<style>\nbody {{ margin: 0; background: var(--md-bg); }}\n{css}\n</style>")
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"Markit\">\n<title>{title}</title>\n{style}{math}\n</head>\n<body>\n<article class=\"md-static\">\n{body}</article>\n</body>\n</html>\n",
        title = escape(title),
    )
}

#[cfg(target_os = "macos")]
pub mod mac {
    use std::ffi::c_void;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Sel};
    use objc2::{class, msg_send};
    use objc2_app_kit::{NSPrintJobDisposition, NSPrintJobSavingURL, NSPrintSaveJob};
    use objc2_foundation::{NSRect, NSString, NSURL};

    /// Paginate the WKWebView with print media styles and write it to `path`,
    /// without showing the print panel. Must run on the main thread.
    pub unsafe fn print_to_pdf(webview: *mut c_void, ns_window: *mut c_void, path: &str) -> Result<(), String> {
        if webview.is_null() || ns_window.is_null() {
            return Err("webview is not available".into());
        }
        let webview = webview as *mut AnyObject;
        let window = ns_window as *mut AnyObject;

        let shared: *mut AnyObject = msg_send![class!(NSPrintInfo), sharedPrintInfo];
        let info: Retained<AnyObject> = msg_send![shared, copy];
        let margin = 50.0f64; // ~18 mm
        let _: () = msg_send![&*info, setTopMargin: margin];
        let _: () = msg_send![&*info, setBottomMargin: margin];
        let _: () = msg_send![&*info, setLeftMargin: margin];
        let _: () = msg_send![&*info, setRightMargin: margin];
        let _: () = msg_send![&*info, setHorizontalPagination: 1isize]; // fit width
        let _: () = msg_send![&*info, setVerticalPagination: 0isize]; // automatic
        let _: () = msg_send![&*info, setHorizontallyCentered: false];
        let _: () = msg_send![&*info, setVerticallyCentered: false];

        let dict: *mut AnyObject = msg_send![&*info, dictionary];
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let _: () = msg_send![dict, setObject: NSPrintSaveJob, forKey: NSPrintJobDisposition];
        let _: () = msg_send![dict, setObject: &*url, forKey: NSPrintJobSavingURL];

        let op: *mut AnyObject = msg_send![webview, printOperationWithPrintInfo: &*info];
        if op.is_null() {
            return Err("WebKit did not create a print operation".into());
        }
        let _: () = msg_send![op, setShowsPrintPanel: false];
        let _: () = msg_send![op, setShowsProgressPanel: false];
        // WKWebView's print view starts with a zero frame; size it or pages render blank.
        let view: *mut AnyObject = msg_send![op, view];
        let bounds: NSRect = msg_send![webview, bounds];
        if !view.is_null() {
            let _: () = msg_send![view, setFrame: bounds];
        }
        let _: () = msg_send![
            op,
            runOperationModalForWindow: window,
            delegate: std::ptr::null_mut::<AnyObject>(),
            didRunSelector: None::<Sel>,
            contextInfo: std::ptr::null_mut::<c_void>()
        ];
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_gfm() {
        let md = "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n\n~~gone~~ and $x^2$\n\n![pic](assets/p.png)\n";
        let html = render_body(md);
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("<del>gone</del>"));
        assert!(html.contains("data-math-style"));
        assert!(html.contains("src=\"assets/p.png\""));
    }

    #[test]
    fn document_wraps_body_and_escapes_title() {
        let doc = render_html_document("hi", "<T>", "p{}");
        assert!(doc.contains("<title>&lt;T&gt;</title>"));
        assert!(doc.contains("<article class=\"md-static\">"));
        assert!(!doc.contains("katex"));
    }
}

