/**
 * 整库静态站点导出的执行层。
 *
 * 路径计算和 DOM 改写在 `noteSiteExport.ts`(纯函数),这里负责跑那个循环:逐篇读盘
 * → 渲染 → 增强 wikilink → 站点化改写 → 内联图 → 写页面,最后补首页。
 *
 * 三件事是刻意的:
 * - **逐篇 await**,不并发。一次全库导出可能是几百篇,并发起来会把后端的读文件线程池
 *   打满,而用户在这期间还要能继续用编辑器。慢一点换一个不卡的界面。
 * - **单篇失败不中断**。一篇笔记读不到(权限、正在被外部写)不该让另外 299 篇白跑。
 *   失败计数最后如实报给用户。
 * - **可取消**。`signal` 在每篇开头检查,用户点了取消就停在页边界上,不会写出半个文件。
 */

import { buildLinkIndex } from "./noteLinks";
import { enhanceWikiLinks } from "./enhanceWikiLinks";
import { renderNoteMarkdown } from "./noteRender";
import { noteDirOf } from "./attachmentUrls";
import { wrapStandaloneHtml } from "./noteExportHtml";
import { inlineLocalImages } from "./noteExportImages";
import {
  buildIndexHtml,
  exportableNotes,
  rewriteForSite,
  siteRelPath,
  type SiteExportResult,
  type SitePage,
} from "./noteSiteExport";
import { exportSiteWrite, peekNote, readAttachment } from "./notebookApi";

/** 站点导出可以注入的外部通道。 */
export type SiteExportDeps = {
  /** 读一篇笔记的正文。用 `peekNote` 而不是 `openNote` —— 后者会登记编辑基线。 */
  readNote: (path: string) => Promise<string>;
  /** 读一张本地图的字节。 */
  readImage: (path: string) => Promise<ArrayBuffer>;
  /** 写站点里的一页。 */
  writePage: (outDir: string, relPath: string, content: string) => Promise<void>;
  /** 首页上「共 N 页」的文案。 */
  pageCountLabel: (count: number) => string;
  /** 嵌入链接的前缀。 */
  embedPrefix: string;
};

/** 导出进度。`current` 是正在处理的笔记标题,收尾时为空串。 */
export type SiteExportProgress = {
  done: number;
  total: number;
  current: string;
};

export type SiteExportInput = {
  vault: string;
  siteTitle: string;
  /** 全库笔记(路径 + 标题)。附件会在这一层被过滤掉。 */
  notes: readonly { path: string; title: string }[];
  outDir: string;
};

/** 接真实通道的默认依赖。 */
export function defaultSiteExportDeps(
  pageCountLabel: (count: number) => string,
  embedPrefix: string,
): SiteExportDeps {
  return {
    readNote: async (path: string) => (await peekNote(path)).content,
    readImage: readAttachment,
    writePage: exportSiteWrite,
    pageCountLabel,
    embedPrefix,
  };
}

/**
 * 跑一次整库导出。
 *
 * @param onProgress 每篇开始前调一次,收尾时再调一次(`current` 为空)。
 * @param signal 取消信号。
 */
export async function runSiteExport(
  input: SiteExportInput,
  deps: SiteExportDeps,
  onProgress?: (progress: SiteExportProgress) => void,
  signal?: AbortSignal,
): Promise<SiteExportResult> {
  const notes = exportableNotes(input.notes);
  /* 链接索引用**全部**笔记建,不只是要导出的那些:一篇笔记可能链到一个还没被导出
     过滤掉的目标,索引缺了它会让那条链接被当成死链、退化成纯文本。 */
  const index = buildLinkIndex(input.notes.map((n) => ({ path: n.path, title: n.title })));
  const pages: SitePage[] = [];
  let written = 0;
  let failed = 0;

  for (let i = 0; i < notes.length; i += 1) {
    if (signal?.aborted) break;
    const note = notes[i]!;
    onProgress?.({ done: i, total: notes.length, current: note.title });
    const rel = siteRelPath(note.path, input.vault);
    try {
      const body = await deps.readNote(note.path);
      const { html } = renderNoteMarkdown(body, { taskLines: false });
      const container = document.createElement("div");
      container.innerHTML = html;
      /* 增强 wikilink 只为了拿到 `data-wiki-path`(解析结果)。文案参数在站点产物里
         看不到 —— `rewriteForSite` 会把这些节点整个换成朴素 `<a>`。 */
      enhanceWikiLinks(container, index, {
        open: (title) => title,
        missing: (target) => target,
        ambiguous: (title) => title,
      });
      rewriteForSite(container, rel, input.vault, deps.embedPrefix);
      await inlineLocalImages(container, noteDirOf(note.path), deps.readImage);
      await deps.writePage(input.outDir, rel, wrapStandaloneHtml(note.title, container.innerHTML));
      pages.push({ path: note.path, rel, title: note.title });
      written += 1;
    } catch {
      // 一篇失败不影响其余。计数如实报给用户,不静默。
      failed += 1;
    }
  }

  // 取消时也把已经写出去的页面补上首页 —— 否则那些文件没有入口。
  if (pages.length > 0) {
    try {
      await deps.writePage(
        input.outDir,
        "index.html",
        buildIndexHtml(pages, input.siteTitle, deps.pageCountLabel(pages.length)),
      );
    } catch {
      // 首页失败不致命:每一页本身都是自洽的,直接打开也能读。
    }
  }

  onProgress?.({ done: written + failed, total: notes.length, current: "" });
  return { written, failed, total: notes.length };
}
