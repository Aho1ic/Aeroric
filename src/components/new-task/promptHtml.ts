import DOMPurify from "dompurify";

const PROMPT_TAGS = ["br", "div", "p", "span", "svg", "path"];
const PROMPT_ATTRIBUTES = [
  "contenteditable",
  "data-file-path",
  "data-file-ext",
  "data-project-id",
  "data-project-path",
  "data-project-name",
  "style",
  "width",
  "height",
  "viewBox",
  "fill",
  "stroke",
  "stroke-width",
  "stroke-linecap",
  "stroke-linejoin",
  "d",
];

/** Sanitize persisted editor markup while retaining Aeroric file chips. */
export function sanitizePromptHtml(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: PROMPT_TAGS,
    ALLOWED_ATTR: PROMPT_ATTRIBUTES,
  });
}
