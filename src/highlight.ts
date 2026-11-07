/*
 * Syntax highlighting shared by the live editor and the static render.
 *
 * Both sides tag tokens with @lezer/highlight's `classHighlighter` classes
 * (tok-keyword, tok-string, …). doc.css colours those classes from the --md-syn-*
 * variables, so every theme highlights the same way in the editor, print/PDF and
 * HTML export.
 */
import { LanguageDescription, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { classHighlighter, highlightCode } from "@lezer/highlight";

/*
 * CodeMirror injects its layout CSS as a runtime <style>. Tauri adds a nonce to
 * style-src, which (per CSP) disables 'unsafe-inline', so that style is blocked
 * unless it carries the same nonce. Tauri stamps the nonce on index.html's own
 * <style> elements; borrow it from there.
 */
const nonced = document.querySelector<HTMLStyleElement>("style[nonce]");
const styleNonce = nonced?.nonce || nonced?.getAttribute("nonce") || undefined;

/** CodeMirror extensions for Crepe code blocks (replaces Crepe's default One Dark). */
export const codeBlockTheme: Extension = [
  syntaxHighlighting(classHighlighter),
  EditorView.lineWrapping, // wrap like print/PDF does, rather than scroll sideways
  styleNonce ? EditorView.cspNonce.of(styleNonce) : [],
];

function fenceLanguage(code: Element): string | undefined {
  for (const c of code.classList) if (c.startsWith("language-")) return c.slice(9);
  return undefined;
}

/** Highlight every fenced block (`pre > code.language-*`) under `root`, in place. */
export async function highlightStatic(root: ParentNode): Promise<void> {
  const blocks = [...root.querySelectorAll<HTMLElement>("pre > code")];
  await Promise.all(
    blocks.map(async (code) => {
      const name = fenceLanguage(code);
      const desc = name ? LanguageDescription.matchLanguageName(languages, name, true) : null;
      if (!desc) return;
      let support;
      try {
        support = await desc.load();
      } catch {
        return; // unknown or failed grammar: leave the block plain
      }
      const text = code.textContent ?? "";
      const out = document.createDocumentFragment();
      highlightCode(
        text,
        support.language.parser.parse(text),
        classHighlighter,
        (token, classes) => {
          if (!classes) return void out.append(token);
          const span = document.createElement("span");
          span.className = classes;
          span.textContent = token;
          out.append(span);
        },
        () => out.append("\n"),
      );
      code.replaceChildren(out);
    }),
  );
}
