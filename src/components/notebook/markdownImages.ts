const IMAGE_URL_RE = /^!\[([^\]]*)\]\(([^\s)]+)(?:\s+(?:"([^"]*)"|'([^']*)'))?\s*\)$/;

const SIZE_RE = /(?:^|\s)(?:width|w|scale|zoom)\s*=\s*(\d{1,4}(?:px|%)?)(?=\s|$)/i;

export interface ImageParts {
  alt: string;
  url: string;
  title?: string;
}

export function parseImageMarkdown(text: string): ImageParts | null {
  const m = text.trim().match(IMAGE_URL_RE);
  if (!m) return null;
  return {
    alt: m[1]!,
    url: m[2]!,
    title: m[3] ?? m[4],
  };
}

export function imageWidthFromTitle(title?: string): string | null {
  const m = title?.match(SIZE_RE);
  if (!m) return null;
  const raw = m[1]!;
  if (raw.endsWith("%") || raw.endsWith("px")) return raw;
  return `${raw}px`;
}

function escapeAlt(input: string): string {
  return input.replace(/]/g, "\\]");
}

function escapeTitle(input: string): string {
  return input.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

function withoutImageSize(title?: string): string {
  return (title ?? "").replace(SIZE_RE, " ").replace(/\s+/g, " ").trim();
}

export function buildImageMarkdown(parts: ImageParts): string {
  const title = parts.title?.trim();
  const titlePart = title ? ` "${escapeTitle(title)}"` : "";
  return `![${escapeAlt(parts.alt)}](${parts.url}${titlePart})`;
}

export function setImageMarkdownWidth(text: string, width: string | null): string | null {
  const parts = parseImageMarkdown(text);
  if (!parts) return null;
  const title = withoutImageSize(parts.title);
  const nextTitle =
    width && width.trim() ? [title, `width=${width.trim()}`].filter(Boolean).join(" ") : title;
  return buildImageMarkdown({ ...parts, title: nextTitle || undefined });
}

export function applyImageElementSizing(img: HTMLImageElement, title?: string | null) {
  const width = imageWidthFromTitle(title ?? undefined);
  if (!width) return;
  img.style.width = width;
  img.style.height = "auto";
  img.style.maxWidth = "100%";
  img.dataset.markioImageWidth = width;
}

export function enhanceMarkdownImages(root: HTMLElement) {
  root.querySelectorAll<HTMLImageElement>("img[title]").forEach((img) => {
    const title = img.getAttribute("title");
    applyImageElementSizing(img, title);
  });
}
