/**
 * 整库静态站点导出的纯模型层。
 *
 * 把 vault 里每篇 `.md` 变成一棵自洽的 `.html`:笔记之间的 `[[wikilink]]` 和相对
 * `.md` 链接改写成站内相对 `.html`,再补一个首页。产物可以直接丢到任意静态托管上,
 * 不需要中央服务。
 *
 * 这一层只做**路径计算和 DOM 改写**,不碰文件系统、不碰 Tauri。落盘在 `noteExport.ts`。
 */

import { escapeHtmlText, wrapStandaloneHtml } from "./noteExportHtml";
import { WIKI_EMBED_CLASS, WIKI_LINK_CLASS } from "./enhanceWikiLinks";
import { compareNotebookPath } from "../../lib/notebookSort";

/** 会被当成笔记、参与导出的扩展名。和后端 `fs_ops::is_note_file` 保持一致。 */
const NOTE_EXT_RE = /\.(md|markdown|mdx)$/i;

/** 一个待导出的页面。 */
export type SitePage = {
  /** 笔记的绝对路径。 */
  path: string;
  /** 站内相对路径(已经是 `.html`)。 */
  rel: string;
  /** 页面标题。 */
  title: string;
};

/**
 * 绝对路径 → 站内相对 `.html` 路径。
 *
 * 大小写不敏感地剥 vault 前缀:macOS 的默认文件系统是大小写不敏感的,同一个目录
 * 从不同入口拿到的大小写可能不同(用户手打的路径 vs. 系统返回的规范路径)。
 */
export function siteRelPath(absPath: string, vault: string): string {
  const normalized = absPath.replace(/\\/g, "/");
  const root = vault.replace(/\\/g, "/").replace(/\/+$/, "");
  let rel = normalized;
  if (root && normalized.toLowerCase().startsWith(`${root.toLowerCase()}/`)) {
    rel = normalized.slice(root.length + 1);
  }
  rel = rel.replace(/^\/+/, "");
  return rel.replace(NOTE_EXT_RE, ".html");
}

/**
 * 从 `fromRel` 所在目录看向 `toRel` 的相对链接。
 *
 * 站点是**相对链接**而不是绝对路径:绝对路径要求站点挂在域名根上,而用户很可能把
 * 它放在 `example.com/notes/` 这样的子路径下。
 */
export function relativeHref(fromRel: string, toRel: string): string {
  const fromDir = fromRel.split("/").slice(0, -1);
  const to = toRel.split("/");
  let shared = 0;
  while (shared < fromDir.length && shared < to.length - 1 && fromDir[shared] === to[shared]) {
    shared += 1;
  }
  const ups = fromDir.slice(shared).map(() => "..");
  const downs = to.slice(shared);
  const parts = [...ups, ...downs];
  // 同目录同名(自己链自己)时 parts 为空,退回纯文件名。
  return parts.length > 0 ? parts.join("/") : (to[to.length - 1] ?? "");
}

/**
 * 站点化改写一棵已经 `enhanceWikiLinks` 过的 DOM。
 *
 * 三件事:
 * 1. `a.notebook-wikilink` → 站内相对链接;解析不到目标的退化成纯文本(留一个指向
 *    不存在页面的链接,在站点上点了是 404,比纯文本更糟)。
 * 2. `.notebook-embed` → 链到目标页。v1 不把嵌入内容真的内联进来:那要处理循环嵌入
 *    和嵌套深度,而一个链接已经能到达目标。
 * 3. 相对 `.md` 链接 → `.html`,锚点保留。
 *
 * @param container 就地改写。
 * @param currentRel 当前页的站内相对路径,用来算相对链接。
 * @param vault vault 根,用来把 `data-wiki-path` 的绝对路径折成站内路径。
 * @param embedPrefix 嵌入链接的前缀文案(i18n 给,默认一个箭头)。
 */
export function rewriteForSite(
  container: HTMLElement,
  currentRel: string,
  vault: string,
  embedPrefix = "↪",
): void {
  const doc = container.ownerDocument;

  for (const link of Array.from(container.querySelectorAll<HTMLElement>(`a.${WIKI_LINK_CLASS}`))) {
    const path = link.dataset.wikiPath;
    const text = link.textContent ?? "";
    if (!path) {
      link.replaceWith(doc.createTextNode(text));
      continue;
    }
    const anchor = doc.createElement("a");
    anchor.setAttribute("href", relativeHref(currentRel, siteRelPath(path, vault)));
    anchor.textContent = text;
    link.replaceWith(anchor);
  }

  for (const embed of Array.from(container.querySelectorAll<HTMLElement>(`.${WIKI_EMBED_CLASS}`))) {
    const path = embed.dataset.embedPath;
    const target = embed.dataset.embedTarget ?? "";
    if (!path) {
      // 没解析到就把原始语法留下来 —— 读的人至少知道这里本来指向什么。
      embed.replaceWith(doc.createTextNode(embed.dataset.embedRaw ?? `![[${target}]]`));
      continue;
    }
    const anchor = doc.createElement("a");
    anchor.setAttribute("href", relativeHref(currentRel, siteRelPath(path, vault)));
    anchor.textContent = `${embedPrefix} ${target}`.trim();
    embed.replaceWith(anchor);
  }

  for (const anchor of Array.from(container.querySelectorAll<HTMLAnchorElement>("a[href]"))) {
    const href = anchor.getAttribute("href");
    if (!href) continue;
    // 带 scheme 的(http:、mailto:、javascript:)、纯锚点、协议相对地址都不是站内 md 链接。
    if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("#") || href.startsWith("//")) {
      continue;
    }
    const match = /^([^#?]*)\.(md|markdown|mdx)(#.*)?$/i.exec(href);
    if (match) anchor.setAttribute("href", `${match[1]}.html${match[3] ?? ""}`);
  }
}

/**
 * 站点首页:按站内路径排序列出所有页面。
 *
 * @param pageCountLabel 「共 N 页」的文案,由调用方按 i18n 组好。
 */
export function buildIndexHtml(
  pages: readonly SitePage[],
  siteTitle: string,
  pageCountLabel: string,
): string {
  const sorted = [...pages].sort((a, b) => compareNotebookPath(a.rel, b.rel));
  const items = sorted
    .map((page) => {
      const href = escapeHtmlText(page.rel);
      const title = escapeHtmlText(page.title);
      const rel = escapeHtmlText(page.rel);
      return (
        `<li><a href="${href}">${title}</a>` +
        `<span style="color:#86868b;font-size:0.85em;margin-left:8px">${rel}</span></li>`
      );
    })
    .join("\n");
  const body = `<h1>${escapeHtmlText(siteTitle)}</h1>
<p style="color:#6e6e73">${escapeHtmlText(pageCountLabel)}</p>
<ul style="list-style:none;padding-left:0">
${items}
</ul>`;
  return wrapStandaloneHtml(siteTitle, body);
}

/** 一次站点导出的结果。 */
export type SiteExportResult = {
  written: number;
  failed: number;
  total: number;
};

/** 参与导出的笔记(把 vault 扫描结果收窄到这一层需要的字段)。 */
export type ExportableNote = {
  path: string;
  title: string;
};

/** 只保留 markdown 笔记 —— vault 里还有附件。 */
export function exportableNotes<T extends { path: string }>(notes: readonly T[]): T[] {
  return notes.filter((note) => NOTE_EXT_RE.test(note.path));
}
