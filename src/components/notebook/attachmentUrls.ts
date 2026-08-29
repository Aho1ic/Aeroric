/**
 * 把笔记里的相对图片路径变成 `<img src>` 能渲染的 URL。
 *
 * 为什么需要这一层:笔记里写的是 `attachments/x.png`(相对**笔记所在目录**),而
 * WebView 的 document base 是 `tauri://localhost`,相对路径会解析到应用包里去 ——
 * 图永远是坏的。而 CSP 里 `img-src` 只有 `'self' data: blob:`,没开 asset 协议,
 * 所以也不能简单换成 `asset://` 前缀:那要给 WebView 开一整棵目录的读权限并放宽
 * CSP,而这里只需要显示 vault 里的几张图。
 *
 * 于是:后端读字节(仍然过 vault allowlist)→ 前端做 blob URL(CSP 已允许)。
 *
 * blob URL 是要手动回收的。一个 scope 记住自己造过的所有 URL,scope 失效时全部
 * revoke;失效之后才落地的那次读取自己 revoke 掉,不把悬空 URL 交给已经卸载的
 * 组件 —— 这套和 `useDshImageLoader` 是同一个形状。
 */

import { readAttachment } from "./notebookApi";

/**
 * 任何 `scheme:` 开头的地址都不当成 vault 路径。
 *
 * 这里故意不是"排除已知的安全 scheme",而是排除**所有** scheme。反过来写的话
 * `javascript:alert(1)` 不在安全名单里,于是被判定成"相对路径"送进解析层 —— 那
 * 等于把一段脚本当文件名。宁可让没见过的 scheme 走原样输出(由 CSP 兜)。
 *
 * Windows 盘符(`C:\x.png`)也会命中这条。它本来就是绝对路径,不需要解析。
 */
const SCHEME_RE = /^[a-z][a-z0-9+.-]*:/i;

/** 判断一个图片地址是否需要走 vault 解析。 */
export function needsVaultResolve(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;
  if (SCHEME_RE.test(trimmed)) return false;
  // 协议相对地址(`//host/x.png`)是网络地址,不是 vault 里的文件。
  if (trimmed.startsWith("//")) return false;
  // 纯锚点(`#foo`)不是图片地址。
  if (trimmed.startsWith("#")) return false;
  return true;
}

/** 已经是绝对路径的(POSIX `/x` 或 Windows `C:\x`)不需要拼接。 */
function isAbsolutePath(path: string): boolean {
  return path.startsWith("/") || path.startsWith("\\") || /^[a-z]:[\\/]/i.test(path);
}

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

/**
 * 从扩展名猜 MIME。
 *
 * blob 的 type 决定浏览器怎么解码。留空时 Chromium 会按内容嗅探,大多数位图能
 * 猜对,但 SVG 不行 —— 不带 `image/svg+xml` 的 blob 会被当成下载而不是图片。
 */
function mimeFromPath(path: string): string {
  const ext = path.split(/[?#]/, 1)[0]!.split(".").pop()?.toLowerCase() ?? "";
  return EXTENSION_MIME[ext] ?? "application/octet-stream";
}

/**
 * 把 `dir` 和一个相对路径拼成绝对路径,并把 `.` / `..` 折叠掉。
 *
 * 折叠必须在这里做:笔记里的链接是 `../attachments/x.png`,把它原样拼进绝对路径
 * 交给后端,`resolve_in_vaults` 那道闸门看到 `..` 会直接拒 —— 而这条路径其实是
 * 合法的,它只是需要先被规范化。
 */
export function joinNotePath(noteDir: string, relative: string): string {
  // 绝对路径(手写的、或者从别的工具导入的笔记里带的)不拼进 `noteDir`:拼上去
  // 会得到 `<noteDir>/Users/...` 这种东西。它在不在 vault 里由后端的 allowlist
  // 判 —— 这一层只负责规范化。
  const base = isAbsolutePath(relative) ? "" : noteDir;
  // 分隔符从**输入**判,不能从拼好的串判:拼接用的那个 `/` 会自己投一票,于是
  // Windows 上永远得到 `/`,拼出来的路径后端认不出。
  const platformSource = base || relative;
  const separator = platformSource.includes("\\") && !platformSource.includes("/") ? "\\" : "/";
  const source = base ? `${base}/${relative}` : relative;
  // 根前缀单独留着:它不参与 `..` 的回退,否则 `/a/../../b` 会把根吃掉变成 `b`。
  const root = source.startsWith("/")
    ? "/"
    : source.startsWith("\\")
      ? "\\"
      : (/^[a-z]:[\\/]/i.exec(source)?.[0] ?? "");
  const segments: string[] = [];
  for (const part of source.slice(root.length).split(/[\\/]/)) {
    if (!part || part === ".") continue;
    if (part === "..") {
      // 退不动了就丢掉这一级。留着 `..` 的话后端那道闸门会直接拒掉整条路径,而
      // 这时候的路径本来就已经越界了,拒是对的 —— 只是不该由这里假装成功。
      segments.pop();
      continue;
    }
    segments.push(part);
  }
  return root + segments.join(separator);
}

/**
 * 从笔记看向一个 vault 内文件的相对链接。
 *
 * 和后端 `attachments::finish` 算的是同一件事,只是这里的输入是"已经存在的附件"
 * (从附件面板插入),而后端那边是"刚写下来的附件"。相对的是**笔记所在目录**而不是
 * vault 根 —— 这样产出的 markdown 在别的编辑器 / SSG 里也是对的。
 *
 * @param vault vault 根的绝对路径。
 * @param notePath 笔记的绝对路径。
 * @param relativePath 附件相对 vault 根的路径(后端列表里给的那个)。
 */
export function linkFromNote(vault: string, notePath: string, relativePath: string): string {
  const noteDir = noteDirOf(notePath);
  const withinVault = noteDir.startsWith(vault) ? noteDir.slice(vault.length) : "";
  const depth = withinVault.split(/[\\/]/).filter(Boolean).length;
  // 链接里一律用 `/`:markdown 是跨平台的,Windows 的反斜杠在别的工具里读不出来。
  return `${"../".repeat(depth)}${relativePath.split(/[\\/]/).filter(Boolean).join("/")}`;
}

/** 图片类的附件插成 `![]()`,其余插成普通链接。 */
export function attachmentMarkdown(name: string, kind: string, link: string): string {
  // alt 里的 `[` `]` 会提前闭合,和后端 `sanitize_alt` 是同一个理由。
  const alt = name.replace(/[[\]]/g, "-");
  return kind === "image" || kind === "svg" ? `![${alt}](${link})` : `[${alt}](${link})`;
}

/**
 * vault 内文件的绝对路径 → 相对 vault 根的路径,用来给人看。
 *
 * vault 根往往埋在 `~/Library/Application Support/...` 底下,完整路径在一个 400px
 * 宽的面板里占三行还是看不出笔记在哪个子目录。算不出来(不在这个 vault 下、或者
 * 还没有 vault)就返回 null,由调用方退回完整路径 —— 编一个相对路径出来会指错地方。
 */
export function vaultRelativePath(vault: string | null, path: string): string | null {
  if (!vault) return null;
  // 结尾的分隔符要先去掉:vault 是 `/a/b/` 而路径是 `/a/b/n.md` 时,不去掉会切出
  // 一个开头带分隔符的残段。
  const root = vault.replace(/[\\/]+$/, "");
  if (!path.startsWith(root)) return null;
  const rest = path.slice(root.length);
  // 前缀相同不等于在里面:vault 是 `/notes` 而路径是 `/notes-old/a.md` 时,
  // `startsWith` 会放行,切出来的 `-old/a.md` 是一条指向别处的假路径。剩下的部分
  // 必须以分隔符开头,或者干脆为空(路径正好就是 vault 自己)。
  if (rest && !/^[\\/]/.test(rest)) return null;
  // 路径正好就是 vault 自己时是空串,而空串显示出来是一片空白。
  return rest.replace(/^[\\/]+/, "") || null;
}

/** 笔记文件路径 → 它所在的目录。 */
export function noteDirOf(notePath: string): string {
  const index = Math.max(notePath.lastIndexOf("/"), notePath.lastIndexOf("\\"));
  return index > 0 ? notePath.slice(0, index) : notePath;
}

type Scope = {
  /** scope 失效时 +1。晚到的读取拿它和自己出发时的值比。 */
  generation: number;
  /** 绝对路径 → URL 的 promise。同一张图在一篇笔记里出现十次也只读一次。 */
  cache: Map<string, Promise<string>>;
  /** 这个 scope 造出来的 blob URL,失效时要全部 revoke。 */
  urls: Set<string>;
};

function revoke(url: string): void {
  if (!url.startsWith("blob:")) return;
  if (typeof URL.revokeObjectURL === "function") URL.revokeObjectURL(url);
}

async function mintUrl(absolutePath: string): Promise<string> {
  const bytes = await readAttachment(absolutePath);
  const blob = new Blob([bytes], { type: mimeFromPath(absolutePath) });
  if (typeof URL.createObjectURL !== "function") {
    throw new Error("URL.createObjectURL is unavailable");
  }
  return URL.createObjectURL(blob);
}

/**
 * 一个附件 URL 的解析器。
 *
 * 生命周期跟着"当前打开的笔记":换笔记就 `release()`,上一篇的 blob 全部回收。
 */
export class AttachmentUrlResolver {
  private scope: Scope = { generation: 0, cache: new Map(), urls: new Set() };

  /**
   * 解析一个图片地址。已经是绝对地址的原样返回,相对地址读字节换 blob URL。
   *
   * @param noteDir 笔记所在目录。链接是相对它的,不是相对 vault 根。
   */
  resolve(url: string, noteDir: string): Promise<string> {
    if (!needsVaultResolve(url)) return Promise.resolve(url);
    // 链接里的空格在 markdown 里是 `%20`,落到文件系统上得还原回来。
    const decoded = safeDecode(url.trim());
    const absolute = joinNotePath(noteDir, decoded);
    const scope = this.scope;
    const cached = scope.cache.get(absolute);
    if (cached) return cached;
    const generation = scope.generation;
    const pending = mintUrl(absolute).then((minted) => {
      if (scope.generation !== generation) {
        // scope 已经失效了:这个 URL 没人会回收,自己 revoke 掉。
        revoke(minted);
        throw new Error("the attachment scope was released before the read completed");
      }
      scope.urls.add(minted);
      return minted;
    });
    // 失败的不留在缓存里,否则一次读失败(比如文件刚好在被写)会让这张图永远
    // 显示不出来,连重新打开笔记都救不回。
    pending.catch(() => {
      if (scope.cache.get(absolute) === pending) scope.cache.delete(absolute);
    });
    scope.cache.set(absolute, pending);
    return pending;
  }

  /** 回收这个 scope 造过的所有 blob URL。换笔记 / 卸载时调。 */
  release(): void {
    const scope = this.scope;
    // generation 变了之后,in-flight 的读取落地时会认出自己已经过期,自己 revoke
    // 掉新造的 URL —— 这才是"释放之后才读完"那些图不漏的原因。这三步都是同步的,
    // 顺序不影响结果,但 generation 放最前面读起来最像那句不变式。
    scope.generation += 1;
    scope.cache.clear();
    for (const url of scope.urls) revoke(url);
    scope.urls.clear();
  }
}

/** `decodeURIComponent` 遇到孤立的 `%` 会抛。文件名里带 `%` 的图不该因此消失。 */
function safeDecode(input: string): string {
  try {
    return decodeURIComponent(input);
  } catch {
    return input;
  }
}
