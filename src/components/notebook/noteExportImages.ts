/**
 * 把导出 DOM 里的本地图片换成 `data:` URL。
 *
 * 这一步在 Markio 那边是后端渲染时做的。Aeroric 的渲染管线在前端,阅读态的图走的是
 * **blob URL**(`attachmentUrls.ts`:后端读字节 → 前端造 blob,绕开 CSP 对 asset 协议
 * 的限制)。blob URL 的作用域是当前文档,写进导出文件之后立刻就是死链 —— 所以导出
 * 必须自己把字节内联一遍,不能复用阅读态那份 DOM。
 *
 * 只处理**本地**图片。远端 `http(s)` 图保持原样:内联它们需要一条会发外部请求的通道,
 * 那要配 SSRF 防护(私网地址、重定向、大小上限),不在这一步的范围里。导出物在联网时
 * 仍然能显示它们。
 */

import { joinNotePath, needsVaultResolve } from "./attachmentUrls";
import { readAttachment } from "./notebookApi";

/** 扩展名 → MIME。`data:` URL 必须带对的 MIME,否则浏览器不当图片解。 */
const EXTENSION_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  avif: "image/avif",
  svg: "image/svg+xml",
  ico: "image/x-icon",
  tiff: "image/tiff",
  tif: "image/tiff",
  heic: "image/heic",
};

/** 单张图内联的上限。超了就留原路径 —— 一个 40MB 的 PNG 会让导出文件打不开。 */
const MAX_INLINE_BYTES = 8 * 1024 * 1024;

export function mimeFromPath(path: string): string {
  const ext = path.split(/[?#]/, 1)[0]!.split(".").pop()?.toLowerCase() ?? "";
  return EXTENSION_MIME[ext] ?? "application/octet-stream";
}

/**
 * ArrayBuffer → base64。
 *
 * 分块喂给 `String.fromCharCode`:一次性展开成参数列表会在几百 KB 量级上把调用栈
 * 撑爆(`RangeError: too many arguments`),而图片正好就在那个量级。
 */
export function bytesToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/** 导出时图片内联的结果统计。 */
export type InlineImagesResult = {
  /** 成功换成 data URL 的张数。 */
  inlined: number;
  /** 读不到 / 太大 / 不是本地图,保持原样的张数。 */
  skipped: number;
};

/**
 * 就地把 `container` 里的本地图片 `src` 换成 data URL。
 *
 * @param noteDir 笔记所在目录。图片链接是相对它的,不是相对 vault 根。
 * @param read 读字节的通道。默认走后端(仍然过 vault allowlist),测试注入假的。
 */
export async function inlineLocalImages(
  container: HTMLElement,
  noteDir: string,
  read: (path: string) => Promise<ArrayBuffer> = readAttachment,
): Promise<InlineImagesResult> {
  const images = Array.from(container.querySelectorAll<HTMLImageElement>("img[src]"));
  let inlined = 0;
  let skipped = 0;
  // 同一张图在一篇笔记里出现多次时只读一次。
  const cache = new Map<string, string | null>();

  for (const img of images) {
    const src = img.getAttribute("src") ?? "";
    if (!needsVaultResolve(src)) {
      // 远端图、data URL、blob URL 都走这里。blob 在导出物里是死的,但把它换成什么
      // 都需要先知道它的来源 —— 而渲染管线给出来的 img 是相对路径,阅读态那份 blob
      // DOM 不该被拿来导出(见模块注释)。
      skipped += 1;
      continue;
    }
    const absolute = joinNotePath(noteDir, safeDecode(src.trim()));
    let dataUrl = cache.get(absolute);
    if (dataUrl === undefined) {
      dataUrl = await readAsDataUrl(absolute, read);
      cache.set(absolute, dataUrl);
    }
    if (dataUrl === null) {
      skipped += 1;
      continue;
    }
    img.setAttribute("src", dataUrl);
    inlined += 1;
  }
  return { inlined, skipped };
}

async function readAsDataUrl(
  absolute: string,
  read: (path: string) => Promise<ArrayBuffer>,
): Promise<string | null> {
  try {
    const bytes = await read(absolute);
    if (bytes.byteLength > MAX_INLINE_BYTES) return null;
    return `data:${mimeFromPath(absolute)};base64,${bytesToBase64(bytes)}`;
  } catch {
    // 读不到就留原路径:导出一份图坏掉的文档,比整个导出失败有用。
    return null;
  }
}

/** `decodeURIComponent` 遇到孤立的 `%` 会抛。文件名里带 `%` 的图不该因此丢掉。 */
function safeDecode(input: string): string {
  try {
    return decodeURIComponent(input);
  } catch {
    return input;
  }
}
