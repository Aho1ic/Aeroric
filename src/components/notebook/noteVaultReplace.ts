/* 随手记的全库替换(⌘⇧F 面板里的「替换」)。
 *
 * 后端复用 Aeroric 已有的 `replace_text_preview` / `apply_text_replacements`,不新写
 * 替换逻辑 —— 那边已经做对了四件难做对的事:按**倒序**逐文件落笔(正序会让后面的偏移
 * 失效)、用命中处原文当乐观锁(对不上就跳过而不是照旧偏移写坏)、校验 UTF-8 字符边界、
 * 把路径锁在项目根里。
 *
 * 这个模块只负责三件后端不管、而前端弄错就会出事的事:
 *
 * 1. **必须传排除模式挡住 vault 私有目录。** 回收站(`.notebook/trash/`)和历史快照
 *    (`.notebook/history/`)里放的也是 `.md`,而后端遍历时只跳 `.git` / `node_modules` /
 *    `dist` / `target`(`search.rs` 的 `is_ignored_dir`)。不挡的话「全库替换」会把已删除
 *    的笔记和历史版本一起改写 —— 而历史版本被改写之后,回滚就再也拿不回替换前的正文。
 *    两条 Rust 测试守着这一点(`replace_preview_exclude_glob_fences_off_a_dot_directory`
 *    和它的对照)。
 *
 * 2. **偏移原样回传,不在 JS 里重算。** 预览给的 `start` / `end` 是**整个文件**的字节
 *    偏移(含 frontmatter),不是行列、也不是 JS 字符串下标。JS 里 `String.length` 数的是
 *    UTF-16 码元,中文笔记里两者对不上,重算一次就是写到错的位置去。
 *
 * 3. **只提交预览里出现过的命中。** 排除模式只在**预览**那一步生效 ——
 *    `apply_text_replacements` 自己不看 glob,只校验路径在根内。所以「不碰私有目录」这条
 *    保障完全依赖「提交的每一条都来自预览」,`buildReplacements` 因此只从预览构造。
 */

import { resolveHitNoteId, type NoteSearchFlags } from "./noteGlobalSearch";

/** vault 私有目录:历史快照、回收站、索引。里面的 `.md` 不是用户的笔记。 */
const VAULT_PRIVATE_GLOB = ".notebook/**";

/** 预览里的一处命中。字段名与 Rust 的 `ReplacePreviewMatch` 对齐(serde camelCase)。 */
export type VaultReplaceMatch = {
  path: string;
  name: string;
  /** 1 起的文件行号,frontmatter 也算。 */
  line: number;
  /** 1 起的**字节**列。只用来显示,定位一律用 `start` / `end`。 */
  column: number;
  lineText: string;
  matchText: string;
  /** 后端算好的替换结果。正则模式下捕获组已经展开,前端不要自己拼。 */
  replacementText: string;
  /** 整个文件的字节偏移。原样回传给 `apply_text_replacements`。 */
  start: number;
  end: number;
};

export type VaultReplaceFile = {
  path: string;
  name: string;
  matches: VaultReplaceMatch[];
};

export type VaultReplacePreview = {
  query: string;
  replacement: string;
  files: VaultReplaceFile[];
  totalMatches: number;
  /** 命中数触顶,预览不是全部。 */
  truncated: boolean;
};

/** 提交给 `apply_text_replacements` 的一条。与 Rust 的 `TextReplacement` 对齐。 */
export type VaultTextReplacement = {
  path: string;
  start: number;
  end: number;
  matchText: string;
  replacementText: string;
};

export type VaultReplaceSummary = {
  filesChanged: number;
  replacementsApplied: number;
  /** 被跳过的条数。乐观锁不符(预览之后文件被改过)会落在这里。 */
  replacementsSkipped: number;
};

/** 传给预览的选项。比搜索多一个排除模式 —— 理由见文件头第 1 条。 */
export type VaultReplaceOptions = {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
  includeGlob: string;
  excludeGlob: string;
  limit: number;
};

export function vaultReplaceOptions(flags: NoteSearchFlags, limit: number): VaultReplaceOptions {
  return {
    caseSensitive: flags.caseSensitive,
    wholeWord: flags.wholeWord,
    regex: flags.regex,
    includeGlob: "*.md",
    excludeGlob: VAULT_PRIVATE_GLOB,
    limit,
  };
}

/**
 * 把预览摊平成待提交的替换列表,跳过用户取消勾选的文件。
 *
 * 只从预览构造 —— 见文件头第 3 条:排除模式只在预览那一步生效,提交路径上没有第二道
 * glob 闸门,所以「不碰私有目录」这条保障靠的就是这里不凭空造条目。
 */
export function buildReplacements(
  preview: VaultReplacePreview,
  excludedPaths: ReadonlySet<string> = new Set(),
): VaultTextReplacement[] {
  const out: VaultTextReplacement[] = [];
  for (const file of preview.files) {
    if (excludedPaths.has(file.path)) continue;
    for (const match of file.matches) {
      out.push({
        path: file.path,
        start: match.start,
        end: match.end,
        matchText: match.matchText,
        replacementText: match.replacementText,
      });
    }
  }
  return out;
}

/** 勾选之后实际会改动的文件数与命中数,用于「替换 N 处 / M 个文件」那行字。 */
export function previewCounts(
  preview: VaultReplacePreview,
  excludedPaths: ReadonlySet<string> = new Set(),
): { files: number; matches: number } {
  let files = 0;
  let matches = 0;
  for (const file of preview.files) {
    if (excludedPaths.has(file.path)) continue;
    files += 1;
    matches += file.matches.length;
  }
  return { files, matches };
}

/**
 * 预览涉及的文件对回内存里的笔记 id。对不上的给 `null`。
 *
 * **不能用字符串直接比。** 后端 `validate_project_root` 会 canonicalize 根目录,macOS 上
 * `/tmp` 会变成 `/private/tmp`,而面板里的笔记 id 是 `listNotes` 给的原始路径 —— 直接比
 * 一条都对不上,而且是静默的。这套换算已经在 `resolveHitNoteId` 里做过一遍(全等优先,
 * 再退回按「相对 vault 的尾段」比),这里复用它,不重写第二份。
 */
export function resolvePreviewNoteIds(
  preview: VaultReplacePreview,
  notePaths: readonly string[],
  vault: string,
): Map<string, string | null> {
  const out = new Map<string, string | null>();
  for (const file of preview.files) {
    out.set(file.path, resolveHitNoteId(file.path, notePaths, vault));
  }
  return out;
}

/* 这里**没有**「哪些文件在内存里还有未落盘编辑」这种查询,是刻意的。
 *
 * 未落盘的编辑确实是这条链上最危险的一环:后端的乐观锁比对的是**磁盘**上那一段原文,
 * 内存里改了没落盘时磁盘仍是旧内容 —— 锁通过、替换写进磁盘,随后那条挂起的自动保存
 * 到期,把替换前的整篇正文写回去,替换静默消失。
 *
 * 但唯一能按笔记分辨脏与不脏的前端信号是 `useNoteAutosave` 的 `saveStates`,而那个
 * 是 state、被明确标注为**只用来显示**(判定全在 `savingRef` / `resaveRef` / 定时器表
 * 那三个 ref 里,写完立刻生效;state 要等下一次渲染)。拿它当闸门会引入一整类时序 bug。
 *
 * 所以正确做法是无条件 `settleSave` 掉每一篇再读盘 —— 那条路径走的就是那三个 ref。
 * 既然如此,再加一道基于 `saveStates` 的检查只是永不触发的第二道闸门。
 */
