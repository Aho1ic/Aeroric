import { Marked } from "marked";
import DOMPurify from "dompurify";

const lspMarkdown = new Marked();

export function renderLspMarkdownToHtml(markdown: string): string {
  const html = lspMarkdown.parse(markdown, { async: false }) as string;
  return DOMPurify.sanitize(html);
}

export function renderLspMarkdownInlineToHtml(markdown: string): string {
  const html = lspMarkdown.parseInline(markdown, { async: false }) as string;
  return DOMPurify.sanitize(html);
}

export function appendLspMarkdown(dom: HTMLElement, markdown: string): void {
  dom.innerHTML = renderLspMarkdownToHtml(markdown);
}

export function appendLspMarkdownInline(dom: HTMLElement, markdown: string): void {
  dom.innerHTML = renderLspMarkdownInlineToHtml(markdown);
}
