/* 随手记面板测试用的内存 vault。
 *
 * 面板落盘后每次新建 / 保存 / 删除都要过一次 Tauri 命令。这里实现一个内存版
 * 后端,让面板测试仍然验证**行为**而不是验证 mock 被调用过 —— 写进去的内容
 * 真的能读回来,冲突检测真的会触发。
 *
 * 刻意与 Rust 侧保持同样的语义:
 * - 保存要比对基线指纹,不一致就报 conflict
 * - 新建不覆盖,重名自动加序号
 * - 标题存 frontmatter,文件名只在新建时定一次
 */

import { deriveTitle } from "../components/notebook/noteFrontmatter";
import { scanWikiLinks } from "../components/notebook/noteLinks";

export type HarnessSig = { mtimeMs: number; hash: string };

type HarnessFile = {
  content: string;
  mtimeMs: number;
};

const VAULT = "/vault";

/** 与 Rust 侧同一套 FNV-1a,保证测试里的冲突判定和真实行为一致。 */
function hash64(text: string): string {
  let hash = 0xcbf29ce484222325n;
  const bytes = new TextEncoder().encode(text);
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash.toString();
}

/* 行内 `#标签` 的扫描。链接那一路能直接借前端的 `scanWikiLinks`(词法器就在前端),
   标签的词法器只在 Rust 里 —— 前端那边只做聚合。所以这里写一个够用的复刻:跳
   frontmatter 与代码块、`#` 前只允许行首或空白、纯数字不算。

   真正的等价性由 `tags.rs` 自己的用例守;这里只需要让面板测试能拿到像样的输入。 */
function harnessTagHits(content: string): { raw: string; line: number; preview: string }[] {
  const lines = content.split("\n");
  const hits: { raw: string; line: number; preview: string }[] = [];
  let start = 0;
  // 未闭合的 `---` 不算 frontmatter,和 Rust 侧一致。
  if (lines[0]?.trim() === "---") {
    const end = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
    if (end > 0) start = end + 1;
  }
  let fenced = false;
  for (let index = start; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) continue;
    // 行内代码整段跳过。
    const bare = line.replace(/`[^`]*`/g, (span) => " ".repeat(span.length));
    for (const match of bare.matchAll(/(^|\s)#([\p{L}\p{N}_/-]+)/gu)) {
      const raw = match[2].replace(/[/-]+$/, "");
      if (!raw || /^\d+$/.test(raw)) continue;
      hits.push({ raw, line: index + 1, preview: line.trim() });
    }
  }
  return hits;
}

/* `- [ ]` 任务行的扫描。和 `harnessTagHits` 同一个性质:真词法器在 Rust
   (`tasks.rs`),前端只做标记解析与分组,所以这里写一个够用的复刻 —— 跳 frontmatter
   与围栏、认有序列表与 blockquote 前缀、空壳 `- [ ]` 不算。

   真正的等价性由 `tasks.rs` 自己那 15 条用例守;这里只需要让面板测试拿到像样的输入。 */
function harnessTasks(content: string): { line: number; checked: boolean; text: string }[] {
  const lines = content.split("\n");
  const out: { line: number; checked: boolean; text: string }[] = [];
  let start = 0;
  // 未闭合的 `---` 不算 frontmatter,和 Rust 侧一致。
  if (lines[0]?.trim() === "---") {
    const end = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
    if (end > 0) start = end + 1;
  }
  let fenced = false;
  for (let index = start; index < lines.length; index += 1) {
    const line = lines[index].replace(/\r$/, "");
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) continue;
    const match = /^(?:\s*>)*\s*(?:[-*+]|\d+[.)])[ \t]+\[([ xX])\]\s*(.*)$/.exec(line);
    if (!match) continue;
    const text = match[2].trim();
    // 空壳 `- [ ]` 不算任务,和 Rust 侧、marked 一致。
    if (!text) continue;
    out.push({ line: index + 1, checked: match[1].toLowerCase() === "x", text });
  }
  return out;
}

/** harness 里的一处未链接提及。字段和 Rust 侧 `MentionHit` 一致。 */
type HarnessMention = {
  needle: string;
  text: string;
  line: number;
  start: number;
  end: number;
  preview: string;
  confidence: "confident" | "ambiguous";
};

/* 未链接提及的扫描。和 `harnessTagHits` 同一个性质:真词法器在 Rust
   (`mentions.rs`),所以这里写一个够用的复刻 —— 跳 frontmatter / 围栏 / 行内代码 /
   已有 `[[链接]]` / ATX 标题,ASCII 词边界拦掉子串,中日韩邻字判 ambiguous。

   偏移按**字节**算,和 Rust 侧一个坐标系:面板测试要验的正是"传下去的区间"和"报告里
   的处数",而处数只有在偏移口径一致时才对得上。真正的等价性由 `mentions.rs` 自己那
   30 条用例守。 */
function harnessMentions(content: string, names: readonly string[]): HarnessMention[] {
  const lines = content.split("\n");
  let startLine = 0;
  if (lines[0]?.trim() === "---") {
    const end = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
    if (end > 0) startLine = end + 1;
  }
  const out: HarnessMention[] = [];
  let fenced = false;
  // 每行的起始字节偏移(含行尾的 `\n`),和 Rust 侧 `line_spans` 一致。
  let base = 0;
  for (let index = 0; index < lines.length; index += 1) {
    const raw = lines[index];
    const line = raw.replace(/\r$/, "");
    const lineBase = base;
    base += byteLength(raw) + 1;
    if (index < startLine) continue;
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) continue;
    // ATX 标题整行跳过,和 Rust 侧一致。
    if (/^\s*#{1,6}(\s|$)/.test(line)) continue;
    /* 不算提及的区间整段抹成空格:行内代码、已有 wikilink、markdown 链接、裸 URL。
       抹成等长空格而不是删掉,这样后面的偏移不用再换算。 */
    const bare = line
      .replace(/`[^`]*`/g, (span) => " ".repeat(span.length))
      .replace(/!?\[\[[^\]\n]*\]\]/g, (span) => " ".repeat(span.length))
      .replace(/!?\[[^\]\n]*\]\([^)\n]*\)/g, (span) => " ".repeat(span.length))
      .replace(/https?:\/\/\S+/g, (span) => " ".repeat(span.length));
    const taken: [number, number][] = [];
    for (const name of names) {
      if (!name) continue;
      const lower = bare.toLowerCase();
      const needle = name.toLowerCase();
      let from = 0;
      for (;;) {
        const at = lower.indexOf(needle, from);
        if (at < 0) break;
        const to = at + needle.length;
        from = to;
        const before = at > 0 ? line[at - 1] : "";
        const after = to < line.length ? line[to] : "";
        const edge = (outer: string, inner: string): "clean" | "ambiguous" | "blocked" => {
          if (!outer || !/[\p{L}\p{N}_]/u.test(outer)) return "clean";
          const cjk = /[぀-ヿ㐀-䶿一-鿿가-힯]/;
          return cjk.test(outer) || cjk.test(inner) ? "ambiguous" : "blocked";
        };
        const left = edge(before, line[at] ?? "");
        const right = edge(after, line[to - 1] ?? "");
        if (left === "blocked" || right === "blocked") continue;
        // 同一处被两个候选命中只留一条,和 Rust 侧一致。
        if (taken.some(([kept, keptEnd]) => at < keptEnd && to > kept)) continue;
        taken.push([at, to]);
        out.push({
          needle: name,
          text: line.slice(at, to),
          line: index + 1,
          start: lineBase + byteLength(line.slice(0, at)),
          end: lineBase + byteLength(line.slice(0, to)),
          preview: line.trim(),
          confidence: left === "clean" && right === "clean" ? "confident" : "ambiguous",
        });
      }
    }
  }
  return out.sort((a, b) => a.line - b.line || a.start - b.start);
}

/** UTF-8 字节长度。Rust 侧的偏移是字节,JS 的字符串下标是 UTF-16 code unit。 */
function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/** 按**字节**区间切一段出来。对不上(切在字符中间)返回 null。 */
function sliceByBytes(content: string, start: number, end: number): string | null {
  const bytes = new TextEncoder().encode(content);
  if (start >= end || end > bytes.length) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes.slice(start, end));
  } catch {
    return null;
  }
}

/** 按**字节**区间替换。 */
function replaceByBytes(content: string, start: number, end: number, insert: string): string {
  const bytes = new TextEncoder().encode(content);
  const decode = (slice: Uint8Array) => new TextDecoder().decode(slice);
  return decode(bytes.slice(0, start)) + insert + decode(bytes.slice(end));
}

/** 这个字节区间是不是落在某条已有 wikilink 里。 */
function harnessAlreadyLinked(content: string, start: number, end: number): boolean {
  const bytes = new TextEncoder().encode(content);
  const before = new TextDecoder().decode(bytes.slice(0, start));
  const lineStart = before.lastIndexOf("\n") + 1;
  const after = new TextDecoder().decode(bytes.slice(end));
  const lineEnd = after.indexOf("\n");
  const line = before.slice(lineStart) + (lineEnd < 0 ? after : after.slice(0, lineEnd));
  const from = before.length - lineStart;
  const to = from + (sliceByBytes(content, start, end)?.length ?? 0);
  for (const match of line.matchAll(/!?\[\[[^\]\n]*\]\]/g)) {
    const at = match.index ?? 0;
    if (from < at + match[0].length && to > at) return true;
  }
  return false;
}

/* frontmatter 字段的解析。和 `harnessTagHits` 同一个性质:真词法器只在 Rust 里
   (`fields.rs`),前端只做聚合,所以这里写一个够用的复刻 —— 顶层 `key: value`、
   行内 `[a, b]`、缩进的 `- item`,不摊平嵌套映射。

   真正的等价性由 `fields.rs` 自己那 17 条用例守;这里只需要让面板测试拿到像样的
   输入。刻意**不**去掉值里的行内 `#`:Rust 侧留着它,harness 砍掉的话面板测试会
   验出一个真后端不存在的行为。 */
function harnessFields(content: string): { key: string; values: string[] }[] {
  if (!content.startsWith("---\n")) return [];
  const lines = content.split("\n");
  const end = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
  // 未闭合的 `---` 不算 frontmatter,和 Rust 侧一致。
  if (end < 1) return [];
  const order: string[] = [];
  const map = new Map<string, string[]>();
  let current: string | null = null;
  const unquote = (value: string): string => {
    if (value.startsWith('"') && value.endsWith('"') && value.length >= 2) {
      return value.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, "\\");
    }
    if (value.startsWith("'") && value.endsWith("'") && value.length >= 2) {
      return value.slice(1, -1).replace(/''/g, "'");
    }
    return value;
  };
  const push = (key: string, value: string): void => {
    const values = map.get(key);
    if (!values || !value || values.includes(value)) return;
    values.push(value);
  };
  for (let index = 1; index < end; index += 1) {
    const line = lines[index];
    if (!line.trim()) continue;
    const indented = /^[ \t]/.test(line);
    const trimmed = line.trim();
    if (indented || trimmed.startsWith("- ") || trimmed === "-") {
      if (trimmed.startsWith("-") && current) {
        const item = trimmed.slice(1).trim();
        if (item) push(current, unquote(item));
      }
      continue;
    }
    if (trimmed.startsWith("#")) continue;
    const colon = trimmed.indexOf(":");
    if (colon < 0) continue;
    const key = trimmed.slice(0, colon).trim();
    if (!key || key.startsWith("-") || key.startsWith("#") || /\s/.test(key)) continue;
    if (!map.has(key)) {
      order.push(key);
      map.set(key, []);
    }
    const value = trimmed.slice(colon + 1).trim();
    const inline = value.startsWith("[") && value.endsWith("]");
    if (inline) {
      for (const item of value.slice(1, -1).split(",")) {
        const text = item.trim();
        if (text) push(key, unquote(text));
      }
    } else if (value) {
      push(key, unquote(value));
    }
    current = key;
  }
  return order.map((key) => ({ key, values: map.get(key) ?? [] }));
}

export class NotebookVaultHarness {
  private files = new Map<string, HarnessFile>();
  /**
   * 单调递增的假时钟。真实 mtime 精度在某些文件系统上只到秒,测试里不能靠
   * Date.now() —— 同一个 tick 内的两次写会拿到同样的 mtime。
   *
   * 基准取一个真实的 epoch 毫秒(2026-01-01)而不是 0 附近:属性面板会把 mtime
   * 格式化出来给人看,从 1000 起算的话面板上是一个 1970-01-01,而那正是这个面板
   * 用来表示"时间戳没读到"的样子。
   */
  private clock = 1_767_225_600_000;
  /** 迁移过的原始 JSON,供断言检查备份行为。 */
  migratedRaw: string | null = null;
  /** 手工排序(文件名列表),对应 vault 里的 `.notebook/order.json`。 */
  order: string[] = [];
  /** 收尾迁移被调用过几次。用来确认它真的接进了启动流程。 */
  richtextConversions = 0;
  /** 「在文件管理器里揭示」收到的参数。要断言的不只是被调用过,还有 allowlist
   *  根传的是 vault —— 传错了这个入口就变成任意路径揭示器。 */
  revealCalls: { path: string; projectPath: string }[] = [];
  /** 落盘次数。「⌘S 在没有改动时不写盘」只能靠它看出来 —— 内容不变的写从
   *  `read()` 上看不出区别。 */
  saveCalls = 0;
  /** 让下一次保存直接失败(磁盘满、权限、IPC 断)。保存失败态是「关 tab 要确认」
   *  的唯一入口,没有它那条分支进不去。冲突**不**走这里 —— 冲突是正常分支。 */
  failNextSave = false;
  /** 存下来的附件,旧的在前。 */
  attachments: {
    path: string;
    name: string;
    relativePath: string;
    size: number;
    modifiedMs: number;
    kind: string;
  }[] = [];
  /** 被读过字节的附件路径。图片解析层"只读一次"靠它验。 */
  attachmentReads: string[] = [];
  /** 让接下来几次附件保存失败。多张图里只有一张失败时的降级路径要用。 */
  private failingAttachmentSaves = 0;
  /** 让附件列表失败。分区的错误态只能从这里进。 */
  failAttachmentList = false;
  /** 让读附件字节失败(文件正被写、权限变了)。图片的坏图标记只能从这里进。 */
  failAttachmentReads = false;
  /** 让读文件元数据失败。属性面板的错误态只能从这里进。 */
  failNoteStat = false;
  /** 自定义图标表(vault 相对路径 → 图标名)。 */
  private icons: Record<string, string> = {};
  /** 让图标的读或写失败。乐观更新的回滚路径只能从这里进。 */
  failIconWrite: "read" | "write" | null = null;
  /** 让全库链接扫描失败,用来验反链面板的错误态。 */
  failLinkScan = false;
  /** 全库链接扫描被调用了几次。验"反链档和图谱共用同一次扫描"用。 */
  linkScanCalls = 0;
  /** 让全库标签扫描失败,用来验标签面板的错误态。 */
  failTagScan = false;
  /** 全库标签扫描被调用了几次。验"只在标签那一档可见时扫"用。 */
  tagScanCalls = 0;
  /** 让全库字段扫描失败,用来验字段浏览器的错误态。 */
  failFieldScan = false;

  /** 让嵌入取数失败,验"填不进来时留下原始语法 + 提示"。 */
  failPeek = false;
  /** 全库字段扫描被调用了几次。验"只在 sheet 开着时扫"用。 */
  fieldScanCalls = 0;
  /** 让全库任务扫描失败,用来验收集箱的错误态。 */
  failTaskScan = false;
  /** 全库任务扫描被调用了几次。验"只在收集箱开着时扫"用。 */
  taskScanCalls = 0;
  /**
   * 挂起中的 `notebook_vault_tags`,按调用顺序排。`holdTagScans()` 之后每次扫描都
   * 停在这里,要测试手工放行。
   *
   * 和 `heldSnapshotReads` 同一个理由:属性面板"回来的不是当前那条就丢掉"这条守卫
   * 只在两次扫描**乱序**返回时才看得出来。默认 harness 同步返回,两次请求永远按
   * 发起顺序完成 —— 那条分支进不去,守卫在测试里等于不存在。
   */
  private heldTagScans: (() => void)[] | null = null;
  /** 每次跨文件重命名的入参。验"传下去的是归一化 key 而不是显示名"用。 */
  tagRenameCalls: { old: string; next: string }[] = [];
  /** 让整次重命名失败(不是单篇失败),用来验小窗里的错误态。 */
  failTagRename = false;
  /**
   * 预置的「单篇失败」列表,原样回到报告的 `failed` 里。
   *
   * 为什么要能造:单篇失败在真后端是权限 / 冲突这类外部条件,harness 里没法自然
   * 触发,而报告里那一段(哪些没成功)恰恰是最该有人看的一段。
   */
  tagRenameFailures: { path: string; message: string }[] = [];
  /** 全库提及扫描被调用了几次。验"只在反链档可见时扫""换笔记会重扫"用。 */
  mentionScanCalls = 0;
  /** 每次提及扫描传下去的候选名字。验"标题和 stem 都在里面"用。 */
  mentionScanNames: string[][] = [];
  /** 让全库提及扫描失败,用来验这一档的错误态。 */
  failMentionScan = false;
  /** 每次「链接提及」的入参。验"只提交用户看见过的那几处"用。 */
  mentionLinkCalls: { path: string; start: number; end: number; text: string }[][] = [];
  /** 让整次链接失败(不是单篇失败),用来验就地错误态。 */
  failMentionLink = false;
  /** 预置的「单篇失败」列表,原样回到报告的 `failed` 里。理由同 `tagRenameFailures`。 */
  mentionLinkFailures: { path: string; message: string }[] = [];

  /** 当前的图标表。断言"真的写进去了"用。 */
  iconTable(): Record<string, string> {
    return { ...this.icons };
  }

  /** 预置一张图标表(模拟上次会话留下的图标)。 */
  seedIcons(table: Record<string, string>): void {
    this.icons = { ...table };
  }

  /** 让接下来 `count` 次附件保存抛错。 */
  failAttachmentSaves(count = 1): void {
    this.failingAttachmentSaves = count;
  }
  /**
   * 版本历史快照,按路径分组,新的在前。
   *
   * 和 Rust 侧的差别:这里**不限流**,每次成功保存都留一条。真实后端两条快照之间
   * 至少隔三分钟,但那个窗口按真实时钟算,而这里的时钟是假的 —— 照搬会让所有
   * 保存都落在同一个窗口里,历史永远只有一条,面板测试也就没东西可看了。限流本身
   * 由 Rust 侧的 `rapid_autosaves_share_one_snapshot` 覆盖。
   */
  private snapshots = new Map<string, { id: string; content: string; createdAtMs: number }[]>();
  /** 让下一次快照相关的调用失败,用来验历史面板的错误态。 */
  failNextSnapshotCall = false;
  /**
   * 挂起中的 `notebook_read_snapshot`,按调用顺序排。`holdSnapshotReads()` 之后
   * 每次读快照都停在这里,要测试手工放行。
   *
   * 为什么需要它:面板里"回来的不是当前选中的那条就丢掉"这条守卫,只有在两个
   * 请求**乱序**返回时才看得出来。默认 harness 是同步返回的,两个请求永远按发起
   * 顺序完成,那条分支进不去 —— 于是守卫在测试里等于不存在。
   */
  private heldSnapshotReads: (() => void)[] | null = null;
  /**
   * 软删的笔记,按回收站 id 存 —— 对应 vault 的 `.notebook/trash/`。
   *
   * 删除**不是**从这个 map 里消失就完了:笔记先离开 `files` 进这里,恢复时再回去。
   * 让删除直接丢掉内容的话「恢复」在测试里就永远拿不到正文,回收站那一整套行为
   * 也就无从验证。
   */
  private trashed = new Map<
    string,
    { name: string; relativePath: string; deletedAtMs: number; content: string }
  >();
  /**
   * 还要让接下来几次回收站调用失败。用来验回收站面板的错误态。
   *
   * 是计数而不是布尔:面板在「清空失败」之后会自己再拉一次列表来纠正清单,那条
   * 补救路径本身也可能失败。只能注入一次失败的话,这两级降级里的第二级就没法验。
   */
  private failingTrashCalls = 0;

  /** 让接下来 `count` 次回收站调用抛错。 */
  failTrashCalls(count = 1): void {
    this.failingTrashCalls = count;
  }

  /** 这一次回收站调用该不该失败。为真时顺带把额度扣掉。 */
  private shouldFailTrashCall(): boolean {
    if (this.failingTrashCalls <= 0) return false;
    this.failingTrashCalls -= 1;
    return true;
  }

  /** 直接往 vault 里放一个文件,模拟「磁盘上已经有笔记」。 */
  seed(fileName: string, content: string): string {
    const path = `${VAULT}/${fileName}`;
    this.files.set(path, { content, mtimeMs: (this.clock += 10) });
    return path;
  }

  /** 绕过面板直接改磁盘,模拟外部编辑器。用来触发冲突。 */
  externalWrite(path: string, content: string): void {
    this.files.set(path, { content, mtimeMs: (this.clock += 10) });
  }

  read(path: string): string | undefined {
    return this.files.get(path)?.content;
  }

  paths(): string[] {
    return [...this.files.keys()].sort();
  }

  private sigOf(path: string): HarnessSig {
    const file = this.files.get(path);
    if (!file) throw new Error(`no such file: ${path}`);
    return { mtimeMs: file.mtimeMs, hash: hash64(file.content) };
  }

  /** 每个命令被调了几次。用来钉住"不该重复扫盘"这类节流行为。 */
  private callCounts = new Map<string, number>();

  callCount(command: string): number {
    return this.callCounts.get(command) ?? 0;
  }

  /** 接管 `invoke`。未知命令直接抛,避免悄悄吞掉真实调用。 */
  handle = (command: string, args: Record<string, unknown> = {}): unknown => {
    this.callCounts.set(command, (this.callCounts.get(command) ?? 0) + 1);
    switch (command) {
      case "notebook_ensure_default_vault":
        return VAULT;

      case "notebook_read_tree":
        return [...this.files.entries()]
          .filter(([path]) => path.endsWith(".md"))
          .map(([path, file]) => ({
            name: path.slice(VAULT.length + 1),
            path,
            isDir: false,
            size: file.content.length,
            modifiedMs: file.mtimeMs,
            children: null,
            truncated: false,
          }));

      case "notebook_open_note": {
        const path = String(args.path);
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        return { content: file.content, sig: this.sigOf(path) };
      }

      /* 只读取数。和 `notebook_open_note` 的返回一模一样 —— 差别全在后端的指纹表上
         (真后端不 record_open),而 harness 根本没有那张表:它的冲突判定按调用方
         传的 `expected` 走,没传就报冲突,正好等价于"没有登记过基线"。 */
      case "notebook_peek_note": {
        const path = String(args.path);
        if (this.failPeek) throw new Error("reading the note failed");
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        return { content: file.content, sig: this.sigOf(path) };
      }

      case "notebook_close_note":
        return undefined;

      case "notebook_note_stat": {
        const path = String(args.path);
        if (this.failNoteStat) throw new Error("reading file info failed");
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        return {
          // 字节数按 UTF-8 算,和真实后端的 `meta.len()` 一致 —— 用 `length` 的话
          // 一篇中文笔记会报成三分之一大。
          size: new TextEncoder().encode(file.content).length,
          modifiedMs: file.mtimeMs,
          // 假时钟没有"创建时间"的概念。给 null 正好覆盖"文件系统不记它"那条路径。
          createdMs: null,
        };
      }

      case "notebook_read_icons":
        if (this.failIconWrite === "read") throw new Error("reading icons failed");
        // 真后端存 `.notebook/icons.json`。这里用一张内存表 —— 测试关心的是
        // "写进去的能读回来"和"整张表替换",不是 JSON 落在哪。
        return { ...this.icons };

      case "notebook_write_icons": {
        if (this.failIconWrite === "write") throw new Error("writing icons failed");
        this.icons = { ...(args.icons as Record<string, string>) };
        return undefined;
      }

      case "notebook_vault_index":
        // 真后端只读文件头 8KB 再取标题;这里内容都在内存里,直接整篇交给
        // `deriveTitle` —— 用的是**前端同一个函数**,而 Rust 侧的 `derive_title`
        // 刻意复刻它的优先级。两边算出不同标题会让「列表里叫 A、链接解析成 B」。
        return [...this.files.keys()]
          .filter((path) => path.endsWith(".md"))
          .sort()
          .map((path) => ({
            path,
            title: deriveTitle(this.files.get(path)?.content ?? "", path),
          }));

      case "notebook_vault_links": {
        this.linkScanCalls += 1;
        if (this.failLinkScan) throw new Error("scanning links failed");
        /* 真后端在 Rust 里手写了一个和前端正则等价的词法扫描(为了逐行拿行号、
           不把整个 vault 的正文搬进 JS)。这里内容都在内存里,直接用前端的
           `scanWikiLinks` —— 两边等价这件事由 `notebook-backlinks.test.ts` 和
           `links.rs` 共享的那张黄金用例表守着,不靠这个 harness。 */
        const sources: {
          path: string;
          links: { raw: string; line: number; preview: string; embed: boolean }[];
        }[] = [];
        for (const path of [...this.files.keys()].filter((name) => name.endsWith(".md")).sort()) {
          const content = this.files.get(path)?.content ?? "";
          const links: { raw: string; line: number; preview: string; embed: boolean }[] = [];
          content.split("\n").forEach((line, index) => {
            for (const hit of scanWikiLinks(line)) {
              links.push({ raw: hit.raw, line: index + 1, preview: line.trim(), embed: hit.embed });
            }
          });
          // 没有链接的笔记不进结果,和 Rust 侧一致。
          if (links.length) sources.push({ path, links });
        }
        return sources;
      }

      case "notebook_vault_tags": {
        this.tagScanCalls += 1;
        if (this.failTagScan) throw new Error("scanning tags failed");
        const sources: { path: string; tags: { raw: string; line: number; preview: string }[] }[] =
          [];
        for (const path of [...this.files.keys()].filter((name) => name.endsWith(".md")).sort()) {
          const tags = harnessTagHits(this.files.get(path)?.content ?? "");
          // 没有标签的笔记不进结果,和 Rust 侧一致。
          if (tags.length) sources.push({ path, tags });
        }
        const held = this.heldTagScans;
        if (!held) return sources;
        // 挂住:`invoke` 的 mock 会 await 这个 promise,于是这次扫描要等测试放行。
        return new Promise((resolve) => {
          held.push(() => resolve(sources));
        });
      }

      case "notebook_vault_fields": {
        this.fieldScanCalls += 1;
        if (this.failFieldScan) throw new Error("scanning fields failed");
        const sources: { path: string; fields: { key: string; values: string[] }[] }[] = [];
        for (const path of [...this.files.keys()].filter((name) => name.endsWith(".md")).sort()) {
          const fields = harnessFields(this.files.get(path)?.content ?? "");
          // 没有字段的笔记不进结果,和 Rust 侧一致。
          if (fields.length) sources.push({ path, fields });
        }
        return sources;
      }

      case "notebook_vault_tasks": {
        this.taskScanCalls += 1;
        if (this.failTaskScan) throw new Error("scanning tasks failed");
        const sources: {
          path: string;
          tasks: { line: number; checked: boolean; text: string }[];
        }[] = [];
        for (const path of [...this.files.keys()].filter((name) => name.endsWith(".md")).sort()) {
          const tasks = harnessTasks(this.files.get(path)?.content ?? "");
          // 没有任务的笔记不进结果,和 Rust 侧一致。
          if (tasks.length) sources.push({ path, tasks });
        }
        return sources;
      }

      case "notebook_vault_mentions": {
        this.mentionScanCalls += 1;
        this.mentionScanNames.push([...(args.names as string[])]);
        if (this.failMentionScan) throw new Error("scanning mentions failed");
        const self = String(args.note);
        const names = (args.names as string[]) ?? [];
        const sources: { path: string; mentions: HarnessMention[] }[] = [];
        for (const path of [...this.files.keys()].filter((name) => name.endsWith(".md")).sort()) {
          // 自己整篇跳过,和 Rust 侧一致。
          if (path === self) continue;
          const mentions = harnessMentions(this.files.get(path)?.content ?? "", names);
          if (mentions.length) sources.push({ path, mentions });
        }
        return sources;
      }

      case "notebook_link_mentions": {
        const targets = args.targets as {
          path: string;
          start: number;
          end: number;
          text: string;
        }[];
        this.mentionLinkCalls.push(targets.map((target) => ({ ...target })));
        if (this.failMentionLink) throw new Error("linking mentions failed");
        /* 复刻 Rust 侧的三个桶,包括**从后往前改**那一条 —— 面板测试要验的正是"报告
           里的处数"和"改完会重扫",而处数只有在偏移没被前一次插入顶偏时才对得上。 */
        const changed: { path: string; count: number }[] = [];
        const skipped: { path: string; start: number; reason: string }[] = [];
        const byPath = new Map<string, typeof targets>();
        for (const target of targets) {
          const list = byPath.get(target.path) ?? [];
          list.push(target);
          byPath.set(target.path, list);
        }
        for (const path of [...byPath.keys()].sort()) {
          let content = this.files.get(path)?.content ?? "";
          const ordered = [...(byPath.get(path) ?? [])].sort((a, b) => b.start - a.start);
          let count = 0;
          for (const target of ordered) {
            const slice = sliceByBytes(content, target.start, target.end);
            if (slice !== target.text) {
              skipped.push({ path, start: target.start, reason: "vanished" });
              continue;
            }
            if (harnessAlreadyLinked(content, target.start, target.end)) {
              skipped.push({ path, start: target.start, reason: "alreadyLinked" });
              continue;
            }
            content = replaceByBytes(content, target.start, target.end, `[[${target.text}]]`);
            count += 1;
          }
          if (count) {
            this.files.set(path, { content, mtimeMs: (this.clock += 10) });
            changed.push({ path, count });
          }
        }
        return {
          changed,
          skipped,
          failed: [...this.mentionLinkFailures],
          // 处数由后端算,前端拿现成的 —— 和 Rust 侧同一个理由,见 `mentions.rs`。
          linked: changed.reduce((sum, entry) => sum + entry.count, 0),
        };
      }

      case "notebook_rename_tag": {
        this.tagRenameCalls.push({ old: String(args.old), next: String(args.new) });
        if (this.failTagRename) throw new Error("renaming the tag failed");
        /* 复刻 Rust 侧的三个桶。改写只认 `harnessTagHits` 认出来的那些行 —— 和真后端
           "索引与重命名共用一个词法器"是同一个性质,所以代码块 / frontmatter /
           `##heading` 里的字样在这里也一样不会被改。 */
        const oldKey = String(args.old)
          .trim()
          .replace(/^#+/, "")
          .replace(/[/-]+$/, "")
          .toLowerCase();
        const nextTag = String(args.new).trim().replace(/^#+/, "");
        const changed: { path: string; count: number }[] = [];
        const skipped: { path: string; reason: string }[] = [];
        for (const path of [...this.files.keys()].filter((name) => name.endsWith(".md")).sort()) {
          const content = this.files.get(path)?.content ?? "";
          const hits = harnessTagHits(content).filter(
            (hit) => hit.raw.toLowerCase() === oldKey && hit.raw !== nextTag,
          );
          if (!hits.length) {
            // 有字样却没有一处算标签 —— 和 Rust 侧一样报出理由。
            if (content.toLowerCase().includes(`#${oldKey}`)) {
              skipped.push({ path, reason: "notATag" });
            }
            continue;
          }
          /* 按行改:harness 的词法器只给行号,不给字节区间。够用 —— 面板测试要验的是
             "报告怎么显示"和"改完会重扫",逐字节的正确性由 `tag_rename.rs` 自己守。 */
          const lines = content.split("\n");
          const touched = new Set(hits.map((hit) => hit.line));
          for (const line of touched) {
            lines[line - 1] = lines[line - 1].replace(
              new RegExp(`#${oldKey}(?![\\w\\u4e00-\\u9fff/-])`, "gi"),
              `#${nextTag}`,
            );
          }
          this.files.set(path, { content: lines.join("\n"), mtimeMs: (this.clock += 10) });
          changed.push({ path, count: hits.length });
        }
        return { changed, skipped, failed: [...this.tagRenameFailures] };
      }

      // 通用 fs 命令,不是 notebook_* 的。列表右键菜单的「在系统文件夹中打开」
      // 借了它,所以这里也要认。
      case "open_in_system_file_manager":
        this.revealCalls.push({
          path: String(args.path),
          projectPath: String(args.projectPath),
        });
        return undefined;

      case "notebook_read_order":
        return [...this.order];

      case "notebook_write_order":
        this.order = (args.names as string[]) ?? [];
        return undefined;

      case "notebook_save_note": {
        this.saveCalls += 1;
        if (this.failNextSave) {
          this.failNextSave = false;
          throw new Error("disk is on fire");
        }
        const path = String(args.path);
        const content = String(args.content);
        const expected = args.expected as HarnessSig | null;
        const force = Boolean(args.force);
        const existing = this.files.get(path);

        if (existing && !force) {
          const disk = this.sigOf(path);
          // 与 Rust 侧同一条判据:两个维度都不一致才算冲突。
          const stale = expected
            ? disk.hash !== expected.hash && disk.mtimeMs !== expected.mtimeMs
            : true;
          if (stale) return { status: "conflict", disk };
        }

        // 快照记在冲突判定之后、写盘之前,和 Rust 侧同序 —— 报冲突的那次保存
        // 不留快照,存的是被覆盖掉的那一版。
        if (existing && existing.content !== content) this.pushSnapshot(path, existing.content);
        this.files.set(path, { content, mtimeMs: (this.clock += 10) });
        return { status: "saved", sig: this.sigOf(path) };
      }

      case "notebook_list_snapshots": {
        if (this.failNextSnapshotCall) {
          this.failNextSnapshotCall = false;
          throw new Error("history is unavailable");
        }
        const path = String(args.path);
        return (this.snapshots.get(path) ?? []).map((snapshot) => ({
          id: snapshot.id,
          filePath: path,
          relativePath: path.slice(VAULT.length + 1),
          createdAtMs: snapshot.createdAtMs,
          size: snapshot.content.length,
        }));
      }

      case "notebook_read_snapshot": {
        if (this.failNextSnapshotCall) {
          this.failNextSnapshotCall = false;
          throw new Error("snapshot is unreadable");
        }
        const path = String(args.path);
        const entryId = String(args.entryId);
        const found = (this.snapshots.get(path) ?? []).find((snapshot) => snapshot.id === entryId);
        if (!found) throw new Error(`no such snapshot: ${entryId}`);
        const payload = {
          entry: {
            id: found.id,
            filePath: path,
            relativePath: path.slice(VAULT.length + 1),
            createdAtMs: found.createdAtMs,
            size: found.content.length,
          },
          content: found.content,
        };
        const held = this.heldSnapshotReads;
        if (!held) return payload;
        // 挂住:`invoke` 的 mock 会 await 这个 promise,于是这次读要等测试放行。
        return new Promise((resolve) => {
          held.push(() => resolve(payload));
        });
      }

      case "notebook_restore_snapshot": {
        if (this.failNextSnapshotCall) {
          this.failNextSnapshotCall = false;
          throw new Error("rollback failed");
        }
        const path = String(args.path);
        const entryId = String(args.entryId);
        const found = (this.snapshots.get(path) ?? []).find((snapshot) => snapshot.id === entryId);
        if (!found) throw new Error(`no such snapshot: ${entryId}`);
        const existing = this.files.get(path);
        // 兜底快照:被回滚覆盖掉的那一版要留下来,否则回滚不可撤销。
        if (existing) this.pushSnapshot(path, existing.content);
        this.files.set(path, { content: found.content, mtimeMs: (this.clock += 10) });
        return {
          content: found.content,
          sig: this.sigOf(path),
          entry: {
            id: found.id,
            filePath: path,
            relativePath: path.slice(VAULT.length + 1),
            createdAtMs: found.createdAtMs,
            size: found.content.length,
          },
        };
      }

      case "notebook_create_note_in_vault": {
        const title = String(args.title ?? "");
        const content = String(args.content ?? "");
        const path = this.allocate(title);
        this.files.set(path, { content, mtimeMs: (this.clock += 10) });
        return { path, sig: this.sigOf(path) };
      }

      // 删除是**软删**:笔记搬进回收站,不是消失。和 Rust 侧同语义。
      case "notebook_delete_note": {
        const path = String(args.path);
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        this.files.delete(path);
        const deletedAtMs = (this.clock += 10);
        const relativePath = path.slice(VAULT.length + 1);
        const name = relativePath.split("/").pop() ?? relativePath;
        this.trashed.set(String(deletedAtMs), {
          name,
          relativePath,
          deletedAtMs,
          content: file.content,
        });
        return {
          id: String(deletedAtMs),
          name,
          relativePath,
          deletedAtMs,
          size: file.content.length,
          isDir: false,
        };
      }

      case "notebook_trash_list": {
        if (this.shouldFailTrashCall()) {
          throw new Error("trash is unavailable");
        }
        return (
          [...this.trashed.entries()]
            .map(([id, item]) => ({
              id,
              name: item.name,
              relativePath: item.relativePath,
              deletedAtMs: item.deletedAtMs,
              size: item.content.length,
              isDir: false,
            }))
            // 新删的在前,和后端一致。
            .sort((left, right) => right.deletedAtMs - left.deletedAtMs)
        );
      }

      case "notebook_trash_restore": {
        if (this.shouldFailTrashCall()) {
          throw new Error("restore failed");
        }
        const id = String(args.id);
        const item = this.trashed.get(id);
        if (!item) throw new Error(`no such trash item: ${id}`);
        const path = `${VAULT}/${item.relativePath}`;
        // 原路径被占用时报 ALREADY_EXISTS，和新建 / 改名同一个前缀。
        if (this.files.has(path)) throw new Error(`ALREADY_EXISTS:${path}`);
        this.files.set(path, { content: item.content, mtimeMs: (this.clock += 10) });
        this.trashed.delete(id);
        return { path, isDir: false };
      }

      case "notebook_trash_purge": {
        if (this.shouldFailTrashCall()) {
          throw new Error("purge failed");
        }
        const id = String(args.id);
        const item = this.trashed.get(id);
        if (!item) throw new Error(`no such trash item: ${id}`);
        this.trashed.delete(id);
        // 和 Rust 侧一致:彻底删除把这条的历史快照一起清掉。
        this.snapshots.delete(`${VAULT}/${item.relativePath}`);
        return undefined;
      }

      case "notebook_trash_purge_all": {
        if (this.shouldFailTrashCall()) {
          throw new Error("emptying the trash failed");
        }
        const count = this.trashed.size;
        for (const item of this.trashed.values()) {
          this.snapshots.delete(`${VAULT}/${item.relativePath}`);
        }
        this.trashed.clear();
        return count;
      }

      case "notebook_rename_to_title": {
        const path = String(args.path);
        const title = String(args.title ?? "");
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        const target = this.allocate(title);
        if (target === path) return path;
        this.files.delete(path);
        this.files.set(target, file);
        return target;
      }

      case "notebook_rename_note": {
        const from = String(args.from);
        const to = String(args.to);
        const file = this.files.get(from);
        if (!file) throw new Error(`no such file: ${from}`);
        if (this.files.has(to)) throw new Error(`ALREADY_EXISTS:${to}`);
        this.files.delete(from);
        this.files.set(to, file);
        return undefined;
      }

      case "notebook_convert_richtext": {
        // P1 收尾迁移。把 `editor: richtext` 的笔记正文换成一个标记,便于断言
        // 「转过了」;真正的 HTML → Markdown 语义由 Rust 侧测试覆盖。
        const converted: { path: string; title: string }[] = [];
        for (const [path, file] of this.files) {
          if (!path.endsWith(".md")) continue;
          if (!/^---\n(?:.*\n)*?editor: richtext\n/m.test(file.content)) continue;
          const next = file.content
            .split("\n")
            .filter((line) => !line.startsWith("editor:"))
            .join("\n");
          this.files.set(path, { content: next, mtimeMs: (this.clock += 10) });
          converted.push({ path, title: path.slice(VAULT.length + 1) });
        }
        this.richtextConversions += 1;
        return {
          vault: VAULT,
          backupDir: `${VAULT}/.notebook/richtext-backup-test`,
          converted,
          skipped: 0,
        };
      }

      case "notebook_attachment_save": {
        const note = String(args.note);
        if (!this.files.has(note)) throw new Error(`no such file: ${note}`);
        if (this.failingAttachmentSaves > 0) {
          this.failingAttachmentSaves -= 1;
          throw new Error("saving the attachment failed");
        }
        const mime = String(args.mime);
        const given = args.fileName == null ? null : String(args.fileName);
        // 和 Rust 侧同一套命名:笔记名 + 毫秒戳 + 扩展名。扩展名优先取文件名。
        const ext =
          given?.includes(".") === true
            ? given.split(".").pop()!.toLowerCase()
            : (mime.split("/")[1]?.replace("+xml", "") ?? "bin");
        const stem = note.slice(VAULT.length + 1).replace(/\.md$/, "");
        const name = `${stem}-${(this.clock += 10)}.${ext}`;
        return this.storeAttachment(note, name, given ?? name, String(args.dataBase64).length);
      }

      case "notebook_attachment_save_from_path": {
        const note = String(args.note);
        if (!this.files.has(note)) throw new Error(`no such file: ${note}`);
        if (this.failingAttachmentSaves > 0) {
          this.failingAttachmentSaves -= 1;
          throw new Error("saving the attachment failed");
        }
        const src = String(args.src);
        const base = src.split(/[\\/]/).pop() ?? "file";
        const ext = base.includes(".") ? base.split(".").pop()!.toLowerCase() : "bin";
        const stem = note.slice(VAULT.length + 1).replace(/\.md$/, "");
        return this.storeAttachment(note, `${stem}-${(this.clock += 10)}.${ext}`, base, 32);
      }

      case "notebook_attachment_list":
        if (this.failAttachmentList) throw new Error("listing attachments failed");
        // 新的在前,和后端一致。
        return [...this.attachments].reverse();

      case "notebook_attachment_read": {
        const path = String(args.path);
        if (this.failAttachmentReads) throw new Error("reading the attachment failed");
        const found = this.attachments.find((item) => item.path === path);
        if (!found) throw new Error(`no such attachment: ${path}`);
        this.attachmentReads.push(path);
        return new Uint8Array([1, 2, 3]).buffer;
      }

      case "notebook_migrate_legacy": {
        // 面板只关心「迁移成功了」,详细的迁移语义由 Rust 侧测试覆盖。
        this.migratedRaw = String(args.rawJson);
        return {
          vault: VAULT,
          backupPath: `${VAULT}/.notebook/legacy-backup-test.json`,
          migrated: [],
          skipped: [],
          totalInput: 0,
        };
      }

      default:
        throw new Error(`unexpected notebook command: ${command}`);
    }
  };

  /**
   * 存一个附件并算出插进正文的 markdown。
   *
   * 链接相对**笔记所在目录**,和 Rust 侧 `finish` 一致 —— 子目录里的笔记要爬回
   * vault 根。写错这一条的话面板测试会对着一条断链断言成功。
   */
  private storeAttachment(note: string, name: string, alt: string, size: number) {
    const noteDir = note.slice(0, note.lastIndexOf("/"));
    const depth = noteDir.slice(VAULT.length).split("/").filter(Boolean).length;
    const relativePath = `attachments/${name}`;
    const link = `${"../".repeat(depth)}${relativePath}`;
    const kind = /\.(png|jpe?g|gif|webp|bmp|avif|ico|tiff?|heic)$/i.test(name)
      ? "image"
      : /\.svg$/i.test(name)
        ? "svg"
        : "pdf";
    const path = `${VAULT}/${relativePath}`;
    const stem = alt.replace(/\.[^.]+$/, "").replace(/[[\]]/g, "-");
    this.attachments.push({
      path,
      name,
      relativePath,
      size,
      modifiedMs: this.clock,
      kind,
    });
    return {
      path,
      name,
      link,
      markdown: kind === "pdf" ? `[${name}](${link})` : `![${stem}](${link})`,
      size,
    };
  }

  /** 记一条快照。新的在前,和后端 `list` 的顺序一致。 */
  private pushSnapshot(path: string, content: string): void {
    const list = this.snapshots.get(path) ?? [];
    const createdAtMs = (this.clock += 10);
    list.unshift({ id: String(createdAtMs), content, createdAtMs });
    this.snapshots.set(path, list);
  }

  /** 直接塞一条快照,免得测试为了造历史先保存好几次。 */
  seedSnapshot(path: string, content: string): string {
    this.pushSnapshot(path, content);
    return this.snapshots.get(path)![0].id;
  }

  snapshotCount(path: string): number {
    return this.snapshots.get(path)?.length ?? 0;
  }

  /** 快照内容,新的在前。 */
  snapshotContents(path: string): string[] {
    return (this.snapshots.get(path) ?? []).map((snapshot) => snapshot.content);
  }

  /** 回收站里的文件名,新删的在前。 */
  trashedNames(): string[] {
    return [...this.trashed.values()]
      .sort((left, right) => right.deletedAtMs - left.deletedAtMs)
      .map((item) => item.name);
  }

  /** 从现在起,全库标签扫描都停住不返回。 */
  holdTagScans(): void {
    this.heldTagScans = [];
  }

  /** 挂起的标签扫描有几个。 */
  heldTagScanCount(): number {
    return this.heldTagScans?.length ?? 0;
  }

  /** 放行第 `index` 个挂起的标签扫描(0 是最早发起的那个)。 */
  releaseTagScan(index: number): void {
    const held = this.heldTagScans;
    if (!held) throw new Error("tag scans are not held");
    const release = held[index];
    if (!release) throw new Error(`no held tag scan at ${index}`);
    held[index] = () => {};
    release();
  }

  /** 从现在起,读快照都停住不返回。 */
  holdSnapshotReads(): void {
    this.heldSnapshotReads = [];
  }

  /** 挂起的读快照有几个。 */
  heldSnapshotReadCount(): number {
    return this.heldSnapshotReads?.length ?? 0;
  }

  /** 放行第 `index` 个挂起的读快照(0 是最早发起的那个)。 */
  releaseSnapshotRead(index: number): void {
    const held = this.heldSnapshotReads;
    if (!held) throw new Error("snapshot reads are not held");
    const release = held[index];
    if (!release) throw new Error(`no held snapshot read at ${index}`);
    held[index] = () => {};
    release();
  }

  /** 与后端 `allocate_note_path` 同样的 slug + 去重规则。 */
  private allocate(title: string): string {
    const stem = slugify(title);
    let name = `${stem}.md`;
    let suffix = 2;
    const taken = new Set([...this.files.keys()].map((path) => path.toLowerCase()));
    while (taken.has(`${VAULT}/${name}`.toLowerCase())) {
      name = `${stem}-${suffix}.md`;
      suffix += 1;
    }
    return `${VAULT}/${name}`;
  }
}

function slugify(title: string): string {
  let out = "";
  let lastDash = false;
  for (const ch of title) {
    const mapped = /[/\\:*?"<>|\s]/.test(ch) || ch.charCodeAt(0) < 0x20 ? "-" : ch;
    if (mapped === "-") {
      if (!lastDash && out.length > 0) {
        out += "-";
        lastDash = true;
      }
      continue;
    }
    out += mapped;
    lastDash = false;
  }
  const trimmed = out.replace(/^[-.\s]+|[-.\s]+$/g, "");
  return trimmed || "untitled";
}

export const HARNESS_VAULT = VAULT;
