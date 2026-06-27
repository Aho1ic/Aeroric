import type { HighlighterGeneric } from "shiki/core";

export type NotebookCodeLanguage =
  | "text"
  | "sql"
  | "python"
  | "javascript"
  | "typescript"
  | "json"
  | "bash";

type ShikiCodeLanguage = "sql" | "python" | "javascript" | "typescript" | "json" | "shellscript";
type ShikiTheme = "github-light" | "github-dark" | "solarized-light";

export const NOTEBOOK_CODE_LANGUAGE_OPTIONS: readonly [NotebookCodeLanguage, string][] = [
  ["text", "Text"],
  ["sql", "SQL"],
  ["python", "Python"],
  ["javascript", "JavaScript"],
  ["typescript", "TypeScript"],
  ["json", "JSON"],
  ["bash", "Bash"],
];

let highlighterPromise: Promise<HighlighterGeneric<ShikiCodeLanguage, ShikiTheme>> | null = null;

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (char) => {
    switch (char) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#39;";
    }
  });
}

function getShikiLanguage(language: string): ShikiCodeLanguage | null {
  if (language === "bash") return "shellscript";
  if (
    language === "sql" ||
    language === "python" ||
    language === "javascript" ||
    language === "typescript" ||
    language === "json"
  ) {
    return language;
  }
  return null;
}

function getHighlighter(): Promise<HighlighterGeneric<ShikiCodeLanguage, ShikiTheme>> {
  if (!highlighterPromise) {
    highlighterPromise = Promise.all([
      import("shiki/core"),
      import("shiki/engine/javascript"),
    ]).then(([{ createBundledHighlighter }, { createJavaScriptRegexEngine }]) => {
      const createHighlighter = createBundledHighlighter<ShikiCodeLanguage, ShikiTheme>({
        langs: {
          sql: () => import("shiki/dist/langs/sql.mjs"),
          python: () => import("shiki/dist/langs/python.mjs"),
          javascript: () => import("shiki/dist/langs/javascript.mjs"),
          typescript: () => import("shiki/dist/langs/typescript.mjs"),
          json: () => import("shiki/dist/langs/json.mjs"),
          shellscript: () => import("shiki/dist/langs/shellscript.mjs"),
        },
        themes: {
          "github-light": () => import("shiki/dist/themes/github-light.mjs"),
          "github-dark": () => import("shiki/dist/themes/github-dark.mjs"),
          "solarized-light": () => import("shiki/dist/themes/solarized-light.mjs"),
        },
        engine: createJavaScriptRegexEngine,
      });
      return createHighlighter({
        langs: ["sql", "python", "javascript", "typescript", "json", "shellscript"],
        themes: ["github-light", "github-dark", "solarized-light"],
      });
    });
  }
  return highlighterPromise;
}

function extractCodeInnerHtml(html: string): string {
  return html.match(/<code[^>]*>([\s\S]*)<\/code>/)?.[1] ?? html;
}

export async function highlightCodeInnerHtml(
  code: string,
  language: string,
  theme: ShikiTheme = "github-light",
): Promise<string> {
  const shikiLanguage = getShikiLanguage(language);
  if (!shikiLanguage) return escapeHtml(code);
  try {
    const highlighter = await getHighlighter();
    return extractCodeInnerHtml(
      highlighter.codeToHtml(code, {
        lang: shikiLanguage,
        theme,
      }),
    );
  } catch {
    return escapeHtml(code);
  }
}
