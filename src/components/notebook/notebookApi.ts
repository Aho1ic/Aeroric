/* 随手记后端命令的 typed wrapper。
 *
 * 随手记原本把笔记存在 localStorage 里。现在它们是磁盘上真正的 `.md` 文件,
 * 所有读写都要过后端的 vault allowlist —— 前端不再直接持有数据,只持有
 * 「工作副本 + 基线指纹」。
 *
 * 保存路径上的关键约定:`saveNote` 必须带上打开时拿到的 `sig` 作为基线。
 * 不带基线时后端无法判断磁盘内容的来历,会一律报冲突(而不是覆盖)。
 */

import { invoke } from "@tauri-apps/api/core";

/** 文件指纹。`hash` 是字符串:u64 超出 JS 安全整数范围,走 number 会丢精度。 */
export type NoteSig = {
  mtimeMs: number;
  hash: string;
};

export type NoteEntry = {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modifiedMs: number;
  children: NoteEntry[] | null;
  /** 因为触到深度/数量上限而没继续扫。UI 要如实告诉用户,而不是显示成空目录。 */
  truncated: boolean;
};

export type OpenedNote = {
  content: string;
  sig: NoteSig;
};

/** 保存结果。冲突不是异常,是需要用户决策的正常分支。 */
export type SaveOutcome = { status: "saved"; sig: NoteSig } | { status: "conflict"; disk: NoteSig };

export type MigratedNote = {
  legacyId: string;
  path: string;
  title: string;
  sourceFormat: string;
  converted: boolean;
};

export type MigrationReport = {
  vault: string;
  backupPath: string;
  migrated: MigratedNote[];
  /** 因 legacyId 已存在而跳过(重复运行时走这里)。 */
  skipped: string[];
  totalInput: number;
};

/** 新建时撞名,后端返回的错误前缀。 */
export const ALREADY_EXISTS_PREFIX = "ALREADY_EXISTS:";

export function isAlreadyExistsError(error: unknown): boolean {
  return typeof error === "string" && error.startsWith(ALREADY_EXISTS_PREFIX);
}

/** 确保全局默认 vault(`~/.aeroric/notebook`)存在并注册,返回其绝对路径。
 *
 * 启动时必须先调一次:后端的 allowlist 是进程内状态,不调的话所有文件命令
 * 都会因为「没有已注册的 vault」被拒。 */
export function ensureDefaultVault(): Promise<string> {
  return invoke<string>("notebook_ensure_default_vault");
}

/** 启用项目级 vault(`<project>/.aeroric/notes`)。默认不开启。 */
export function ensureProjectVault(projectPath: string): Promise<string> {
  return invoke<string>("notebook_ensure_project_vault", { projectPath });
}

/** 注册一个用户自选目录为 vault(挂载已有的 Markdown 库)。 */
export function registerVault(path: string): Promise<string> {
  return invoke<string>("notebook_register_vault", { path });
}

export function unregisterVault(path: string): Promise<void> {
  return invoke<void>("notebook_unregister_vault", { path });
}

export function listVaults(): Promise<string[]> {
  return invoke<string[]>("notebook_list_vaults");
}

export function readTree(root: string): Promise<NoteEntry[]> {
  return invoke<NoteEntry[]>("notebook_read_tree", { root });
}

/** 读手工排序(文件名列表)。 */
export function readOrder(vault: string): Promise<string[]> {
  return invoke<string[]>("notebook_read_order", { vault });
}

/** 写手工排序。拖拽结束后调用。 */
export function writeOrder(vault: string, names: string[]): Promise<void> {
  return invoke<void>("notebook_write_order", { vault, names });
}

export function openNote(path: string): Promise<OpenedNote> {
  return invoke<OpenedNote>("notebook_open_note", { path });
}

/** 释放该文件的进程内指纹。关闭 tab 时调用。 */
export function closeNote(path: string): Promise<void> {
  return invoke<void>("notebook_close_note", { path });
}

/** 保存。`expected` 传打开时拿到的 sig;`force` 只在用户明确选择覆盖后为 true。 */
export function saveNote(
  path: string,
  content: string,
  expected: NoteSig | null,
  force = false,
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("notebook_save_note", {
    path,
    content,
    expected,
    force,
  });
}

/** 新建。已存在时抛 `ALREADY_EXISTS:<path>`,不覆盖。 */
export function createNote(path: string, content: string): Promise<NoteSig> {
  return invoke<NoteSig>("notebook_create_note", { path, content });
}

export type CreatedNote = {
  path: string;
  sig: NoteSig;
};

/** 按标题在 vault 里新建,文件名由后端分配(slug + 去重)。
 *
 * 前端不参与命名:slug 规则含 Windows 保留设备名、UTF-8 边界截断这些平台
 * 细节,两份实现迟早会漂。 */
export function createNoteInVault(
  vault: string,
  title: string,
  content: string,
): Promise<CreatedNote> {
  return invoke<CreatedNote>("notebook_create_note_in_vault", { vault, title, content });
}

export function createFolder(path: string): Promise<void> {
  return invoke<void>("notebook_create_folder", { path });
}

/** 删除笔记。走系统回收站,可恢复。 */
export function deleteNote(path: string): Promise<void> {
  return invoke<void>("notebook_delete_note", { path });
}

export function renameNote(from: string, to: string): Promise<void> {
  return invoke<void>("notebook_rename_note", { from, to });
}

/** 按新标题重新分配文件名并改名,返回新路径。
 *
 * 新建时标题是空的,文件只能先叫 `untitled.md`;用户敲完标题后靠这个把文件名
 * 对上,免得 vault 里全是 `untitled-N.md`。 */
export function renameToTitle(vault: string, path: string, title: string): Promise<string> {
  return invoke<string>("notebook_rename_to_title", { vault, path, title });
}

/** 把 localStorage 里的随手记迁到磁盘。传原始 JSON 字符串,不是解析后的对象
 *  —— 备份要存下真正的原文,包括后端不认识的字段。 */
export function migrateLegacyNotes(rawJson: string): Promise<MigrationReport> {
  return invoke<MigrationReport>("notebook_migrate_legacy", { rawJson });
}

/**
 * 在系统文件管理器里揭示笔记文件。
 *
 * 借的是通用的 `open_in_system_file_manager`(不是 `notebook_*` 命令),所以放在
 * 这里只为了让面板不出现裸 invoke。`vault` 传给后端当 allowlist 根:它会用
 * `validate_path_within` 拒掉根之外的路径,免得这个入口退化成任意路径揭示器。
 */
export function revealNoteInFileManager(path: string, vault: string): Promise<void> {
  return invoke<void>("open_in_system_file_manager", { path, projectPath: vault });
}

export function htmlToMarkdown(html: string): Promise<string> {
  return invoke<string>("notebook_html_to_markdown", { html });
}

export type ConvertedNote = {
  path: string;
  title: string;
};

export type RichtextConversionReport = {
  vault: string;
  backupDir: string;
  converted: ConvertedNote[];
  skipped: number;
};

/** 把 vault 里 `editor: richtext` 的笔记转成 Markdown(P1 收尾迁移)。
 *
 * P0 为了无损把富文本的 HTML 原样落进了 `.md`;WYSIWYG 到位后这些笔记该变成真正
 * 的 Markdown,否则参与不了双链、RAG 分块、导出。幂等 —— 转完的笔记没有标记了。 */
export function convertRichtextNotes(vault: string): Promise<RichtextConversionReport> {
  return invoke<RichtextConversionReport>("notebook_convert_richtext", { vault });
}
