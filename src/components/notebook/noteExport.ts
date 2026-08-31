/**
 * 单篇笔记的导出编排:渲染 → 内联图 → 落盘 / 打印 / 进剪贴板。
 *
 * 纯计算在 `noteExportHtml.ts`(包页面)和 `noteExportImages.ts`(内联图),这一层负责
 * 把它们串起来并接上外部世界(保存对话框、剪贴板、打印)。所有外部通道都是可注入的
 * 参数,测试不需要真的开对话框。
 */

import { renderNoteMarkdown } from "./noteRender";
import { noteDirOf } from "./attachmentUrls";
import { exportFileName, wrapStandaloneHtml } from "./noteExportHtml";
import { inlineLocalImages, type InlineImagesResult } from "./noteExportImages";
import { exportWriteFile, readAttachment } from "./notebookApi";

/** 一篇要导出的笔记。 */
export type ExportSource = {
  /** 笔记的绝对路径。图片相对它解析。 */
  path: string;
  title: string;
  body: string;
};

/** 导出可以注入的外部通道。默认值接真实实现,测试传假的。 */
export type ExportDeps = {
  /** 保存对话框。返回 null 表示用户取消。 */
  pickPath: (defaultName: string, extension: string, label: string) => Promise<string | null>;
  /** 落盘。 */
  write: (path: string, content: string) => Promise<void>;
  /** 写纯文本剪贴板。 */
  writeText: (text: string) => Promise<void>;
  /** 唤起打印。 */
  print: (html: string) => Promise<void>;
  /** 读一张本地图的字节,用来内联。 */
  readImage: (path: string) => Promise<ArrayBuffer>;
  /** `<html lang>`。 */
  lang: string;
};

/** 一次导出的结果。`cancelled` 是用户在对话框里取消,不是失败。 */
export type ExportOutcome = {
  cancelled: boolean;
  /** 落盘路径(取消 / 非落盘类导出时为 null)。 */
  path: string | null;
  images: InlineImagesResult;
};

const NO_IMAGES: InlineImagesResult = { inlined: 0, skipped: 0 };

/**
 * 渲染一篇笔记并把本地图内联进去,返回正文 HTML。
 *
 * 不复用阅读态那份 DOM:阅读态的图是 blob URL,作用域限于当前文档,写进导出物就是
 * 死链(见 `noteExportImages.ts` 的模块注释)。这里从源码重新渲染一遍。
 */
export async function renderForExport(
  source: ExportSource,
  readImage: (path: string) => Promise<ArrayBuffer> = readAttachment,
): Promise<{ html: string; images: InlineImagesResult }> {
  // taskLines 关掉:那些 `data-task-line` 是给阅读态的可点复选框用的,导出物里点不了,
  // 带出去只是噪音。
  const { html } = renderNoteMarkdown(source.body, { taskLines: false });
  const container = document.createElement("div");
  container.innerHTML = html;
  const images = await inlineLocalImages(container, noteDirOf(source.path), readImage);
  return { html: container.innerHTML, images };
}

/** 导出成单文件 HTML。 */
export async function exportAsHtml(source: ExportSource, deps: ExportDeps): Promise<ExportOutcome> {
  const { html, images } = await renderForExport(source, deps.readImage);
  const target = await deps.pickPath(exportFileName(source.title), "html", "HTML");
  // 对话框取消之后**不能**继续:渲染已经做完了,但落盘要停在这里。
  if (!target) return { cancelled: true, path: null, images };
  await deps.write(target, wrapStandaloneHtml(source.title, html, deps.lang));
  return { cancelled: false, path: target, images };
}

/**
 * 导出成 PDF:拼出独立 HTML 交给打印通道,用户在打印对话框里选「存为 PDF」。
 *
 * 不引第三方 PDF 库 —— 那要把整套排版和字体嵌入自己实现一遍,而系统打印栈已经在
 * 用同一份 CSS 做这件事(`@media print` 那段)。
 */
export async function exportAsPdf(source: ExportSource, deps: ExportDeps): Promise<ExportOutcome> {
  const { html, images } = await renderForExport(source, deps.readImage);
  await deps.print(wrapStandaloneHtml(source.title, html, deps.lang));
  return { cancelled: false, path: null, images };
}

/** 导出成 Markdown 文件(原样落盘,不经渲染)。 */
export async function exportAsMarkdown(
  source: ExportSource,
  deps: ExportDeps,
): Promise<ExportOutcome> {
  const target = await deps.pickPath(exportFileName(source.title), "md", "Markdown");
  if (!target) return { cancelled: true, path: null, images: NO_IMAGES };
  await deps.write(target, source.body);
  return { cancelled: false, path: target, images: NO_IMAGES };
}

/** 把渲染后的 HTML 片段(不带 `<html>` 外壳)放进剪贴板。 */
export async function copyAsHtml(source: ExportSource, deps: ExportDeps): Promise<ExportOutcome> {
  const { html, images } = await renderForExport(source, deps.readImage);
  await deps.writeText(html);
  return { cancelled: false, path: null, images };
}

/** 把 Markdown 原文放进剪贴板。 */
export async function copyAsMarkdown(
  source: ExportSource,
  deps: ExportDeps,
): Promise<ExportOutcome> {
  await deps.writeText(source.body);
  return { cancelled: false, path: null, images: NO_IMAGES };
}

/** 打开保存对话框。放在这里而不是组件里:导出的每条路径都要用它。 */
export async function pickSavePath(
  defaultName: string,
  extension: string,
  label: string,
): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  const target = await save({
    defaultPath: `${defaultName}.${extension}`,
    filters: [{ name: label, extensions: [extension] }],
  });
  return typeof target === "string" ? target : null;
}

/** 打开选目录对话框(整库导出用)。返回 null 表示用户取消。 */
export async function pickExportDir(title: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const target = await open({ directory: true, multiple: false, title });
  // multiple: false 下返回的是单个路径,但类型里仍然带着数组分支。
  return typeof target === "string" ? target : null;
}

/**
 * 用一个隐藏 iframe 唤起打印。
 *
 * 为什么不是 `window.open` + `document.write`(Markio 的做法):在 Tauri 的 WebView 里
 * `window.open` 要么被拦,要么开出一个**真的系统窗口**(需要在 tauri.conf 里配),行为
 * 跨平台不一致。同源 iframe 走的是 `frame-src 'self'`,已经在 CSP 里放行了;导出物里
 * 的图是 `data:` URL,也在 `img-src` 里。
 *
 * 打印是同步阻塞的,返回之后才移除 iframe —— 提前移除会让打印预览拿到空文档。
 */
export function printViaIframe(html: string): Promise<void> {
  return new Promise((resolve) => {
    const frame = document.createElement("iframe");
    // 不用 `display: none`:某些 WebView 对不参与布局的 frame 不做排版,打印出来是空白。
    frame.setAttribute("aria-hidden", "true");
    frame.style.position = "fixed";
    frame.style.right = "0";
    frame.style.bottom = "0";
    frame.style.width = "1px";
    frame.style.height = "1px";
    frame.style.opacity = "0";
    frame.style.border = "0";
    frame.style.pointerEvents = "none";

    const cleanup = () => {
      // 交给下一个宏任务:print() 返回时打印栈可能还在读文档。
      window.setTimeout(() => frame.remove(), 0);
      resolve();
    };

    frame.onload = () => {
      try {
        const view = frame.contentWindow;
        if (!view) {
          cleanup();
          return;
        }
        view.focus();
        view.print();
      } catch {
        // 打印被拒(无打印机、用户环境不支持)不该把整条导出链路变成异常。
      }
      cleanup();
    };

    frame.srcdoc = html;
    document.body.appendChild(frame);
  });
}

/** 接真实通道的默认依赖。 */
export function defaultExportDeps(lang: string): ExportDeps {
  return {
    pickPath: pickSavePath,
    write: exportWriteFile,
    writeText: async (text: string) => {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    },
    print: printViaIframe,
    readImage: readAttachment,
    lang,
  };
}
