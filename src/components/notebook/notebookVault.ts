/* 磁盘 vault 的读写编排。
 *
 * 这一层把「磁盘上的 .md 文件」翻译成随手记面板认识的笔记列表,并且是面板与
 * 后端之间唯一的通道。把它独立出来是为了 P1/P2 重写编辑器 UI 时不用碰持久化
 * 逻辑 —— 面板换掉,这个模块留着。
 *
 * 三条不变式:
 * - 面板持有的是**工作副本**,不是磁盘内容。保存时带上打开时的基线指纹。
 * - 标题存 frontmatter,不存文件名。文件名只在新建时定一次(见后端
 *   `allocate_note_path`)。
 * - 冲突不静默:后端报冲突时原样往上传,由调用方决定弹窗还是丢弃。
 */

import {
  createNoteInVault,
  deleteNote,
  openNote,
  readOrder,
  readTree,
  renameNote,
  renameToTitle,
  saveNote,
  writeOrder,
  type NoteEntry,
  type NoteSig,
} from "./notebookApi";
import { deriveTitle, joinNote, splitNote, type NoteFrontmatter } from "./noteFrontmatter";

/** 面板里的一条笔记。`body` 是不含 frontmatter 的正文。 */
export type VaultNote = {
  /** 绝对路径。同时充当身份标识 —— 磁盘上不会有两个同路径文件。 */
  path: string;
  title: string;
  /** Markdown 源码。 */
  body: string;
  /** 保留 frontmatter 里我们不认识的字段,保存时原样写回。 */
  frontmatter: NoteFrontmatter;
  /** 打开时拿到的磁盘指纹。保存时作为冲突检测基线;未打开过的笔记为 null。 */
  sig: NoteSig | null;
  modifiedMs: number;
  /** 正文是否已从磁盘读入。列表初次加载只拿路径,正文按需读。 */
  loaded: boolean;
};

export type SaveResult =
  | { status: "saved"; note: VaultNote }
  | { status: "conflict"; diskSig: NoteSig };

/** 把树打平成笔记列表。随手记面板是平铺列表,目录层级留给 P2 的树视图。 */
export function flattenTree(entries: NoteEntry[]): NoteEntry[] {
  const out: NoteEntry[] = [];
  const walk = (list: NoteEntry[]) => {
    for (const entry of list) {
      if (entry.isDir) {
        if (entry.children) walk(entry.children);
        continue;
      }
      out.push(entry);
    }
  };
  walk(entries);
  return out;
}

/** 从绝对路径取文件名。 */
function baseName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

/**
 * 列出 vault 里的笔记(只拿元数据,不读正文)。
 *
 * 排序:先按用户的手工排序(`.notebook/order.json`),不在其中的排前面并按
 * 修改时间倒序 —— 「最近写的在最上面」,与原来 localStorage 版把新建笔记
 * unshift 到队首的行为一致。
 */
export async function listNotes(vault: string): Promise<VaultNote[]> {
  const [tree, order] = await Promise.all([
    readTree(vault),
    // 排序读不出来(文件缺失 / JSON 损坏)只是回落到按修改时间排,不该让
    // 整个面板打不开。
    readOrder(vault).catch(() => [] as string[]),
  ]);
  // order.json 里存文件名而不是绝对路径:vault 整个目录被搬走后排序还在。
  const rank = new Map((order ?? []).map((name, index) => [name, index]));
  return flattenTree(tree)
    .sort((a, b) => {
      const left = rank.get(baseName(a.path));
      const right = rank.get(baseName(b.path));
      if (left !== undefined && right !== undefined) return left - right;
      // 没排过的笔记(新建 / 外部拖进来的)排在最前,让用户第一眼看到。
      if (left !== undefined) return 1;
      if (right !== undefined) return -1;
      return b.modifiedMs - a.modifiedMs;
    })
    .map((entry) => ({
      path: entry.path,
      // 正文还没读,先用文件名当标题。读入后 `loadNote` 会用 frontmatter 里的
      // 真实标题覆盖它。
      title: stemOf(entry.name),
      body: "",
      frontmatter: { title: null, extra: [] },
      sig: null,
      modifiedMs: entry.modifiedMs,
      loaded: false,
    }));
}

function stemOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

/** 读入某条笔记的正文,返回补全后的笔记。 */
export async function loadNote(note: VaultNote): Promise<VaultNote> {
  const opened = await openNote(note.path);
  const { frontmatter, body } = splitNote(opened.content);
  return {
    ...note,
    title: deriveTitle(opened.content, note.path),
    body,
    frontmatter,
    sig: opened.sig,
    loaded: true,
  };
}

/**
 * 这条笔记落盘会长什么样。
 *
 * 抽出来是因为版本历史要拿它和快照比:快照存的是**整个文件**,只比 `body` 会把
 * frontmatter 的每一行都报成删除。两边共用这一个函数,拼法不会各走一套。
 */
export function noteFileContent(note: VaultNote): string {
  // 标题写回 frontmatter —— 它是标题的唯一权威来源。
  const frontmatter: NoteFrontmatter = { ...note.frontmatter, title: note.title };
  return joinNote(frontmatter, note.body);
}

/**
 * 保存。冲突时不写盘,把磁盘指纹交回调用方。
 *
 * `force` 只应在用户明确选择「覆盖」后为 true。
 */
export async function persistNote(note: VaultNote, force = false): Promise<SaveResult> {
  // 内容由 `noteFileContent` 拼;这里再算一遍 frontmatter 是为了放进返回的笔记里,
  // 让调用方手上那份和磁盘一致。
  const frontmatter: NoteFrontmatter = {
    ...note.frontmatter,
    title: note.title,
  };
  const content = noteFileContent(note);
  const outcome = await saveNote(note.path, content, note.sig, force);
  if (outcome.status === "conflict") {
    return { status: "conflict", diskSig: outcome.disk };
  }
  return {
    status: "saved",
    note: { ...note, frontmatter, sig: outcome.sig, modifiedMs: outcome.sig.mtimeMs },
  };
}

/** 新建笔记。文件名由后端按标题分配。 */
export async function createNote(vault: string, title: string): Promise<VaultNote> {
  const trimmed = title.trim();
  const frontmatter: NoteFrontmatter = { title: trimmed || null, extra: [] };
  const content = joinNote(frontmatter, "");
  // 空标题时传个占位名给后端定文件名,否则 slug 全落到 `untitled-N.md`。
  const created = await createNoteInVault(vault, trimmed || "untitled", content);
  return {
    path: created.path,
    title: trimmed,
    body: "",
    frontmatter,
    sig: created.sig,
    modifiedMs: created.sig.mtimeMs,
    loaded: true,
  };
}

/** 删除(进系统回收站,可恢复)。 */
export async function removeNote(note: VaultNote): Promise<void> {
  await deleteNote(note.path);
}

/** 重命名文件本身。标题改动不走这里 —— 标题在 frontmatter 里。 */
export async function renameNoteFile(note: VaultNote, nextPath: string): Promise<VaultNote> {
  await renameNote(note.path, nextPath);
  return { ...note, path: nextPath };
}

/** 把当前列表顺序落盘,让手工排序在重开面板后仍然生效。 */
export async function persistOrder(vault: string, paths: string[]): Promise<void> {
  await writeOrder(vault, paths.map(baseName));
}

/**
 * 按新标题给文件改名,返回改名后的笔记。
 *
 * 刻意**不**自动调用:文件名在新建时定一次就不动了。理由有三 ——
 * 用户可能已经在别的编辑器里打开了这个文件,改名会让它扑空;改名会和自动
 * 保存抢同一个路径;P4 的 `[[wikilink]]` 按文件名互链,自动改名会静默断链。
 * 显式的「重命名」入口在 P2 的文件树右键菜单里。
 */
export async function renameNoteToTitle(vault: string, note: VaultNote): Promise<VaultNote> {
  const nextPath = await renameToTitle(vault, note.path, note.title);
  return nextPath === note.path ? note : { ...note, path: nextPath };
}
