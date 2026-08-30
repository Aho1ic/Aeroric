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
import type { NoteLinkSource } from "./noteBacklinks";
import type { NoteFieldSource } from "./noteFields";
import type { NoteTagSource } from "./noteTags";

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

/**
 * 只读地取一篇笔记的内容。给嵌入(`![[note]]`)用。
 *
 * **不要**用 `openNote` 做这件事:它会把这篇笔记登记成"编辑器里打开着",于是某次
 * 没带基线的保存会拿"嵌入渲染那一刻的指纹"当基线,把盲写放过去。配 `closeNote`
 * 更糟 —— 被嵌入的那篇可能同时开在另一个 tab 里,嵌入侧那次关闭会清掉那个 tab 的
 * 基线,它下一次保存就退回宽松模式静默覆盖。
 */
export function peekNote(path: string): Promise<OpenedNote> {
  return invoke<OpenedNote>("notebook_peek_note", { path });
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

/** 回收站里的一条。`id` 是删除时刻的毫秒时间戳(同毫秒带 `-N` 后缀)。 */
export type TrashItem = {
  /** 恢复 / 彻底删除只认它。前端不回传路径 —— 那个入口就没法被拿去动 vault 外的东西。 */
  id: string;
  /** 删除前的文件名。 */
  name: string;
  /** 删除前相对 vault 根的路径。UI 用它告诉用户"这条原来在哪"。 */
  relativePath: string;
  deletedAtMs: number;
  size: number;
  isDir: boolean;
};

export type RestoredItem = {
  path: string;
  isDir: boolean;
};

/** 删除笔记。软删到 `<vault>/.notebook/trash/`,可从回收站恢复。
 *
 * 不进系统回收站:那里记不住"这条原来在 vault 的哪个子目录",恢复一个
 * `untitled.md` 时用户根本不知道该往哪放。系统回收站是「彻底删除」的落点。 */
export function deleteNote(path: string): Promise<TrashItem> {
  return invoke<TrashItem>("notebook_delete_note", { path });
}

/** 列出 vault 回收站,新删的在前。 */
export function listTrash(vault: string): Promise<TrashItem[]> {
  return invoke<TrashItem[]>("notebook_trash_list", { vault });
}

/** 恢复回原路径。原路径已被占用时抛 `ALREADY_EXISTS:<path>`。 */
export function restoreTrashItem(vault: string, id: string): Promise<RestoredItem> {
  return invoke<RestoredItem>("notebook_trash_restore", { vault, id });
}

/** 彻底删除一条:载荷进系统回收站,清单和历史快照删掉。 */
export function purgeTrashItem(vault: string, id: string): Promise<void> {
  return invoke<void>("notebook_trash_purge", { vault, id });
}

/** 清空回收站,返回清掉的条数。 */
export function purgeAllTrash(vault: string): Promise<number> {
  return invoke<number>("notebook_trash_purge_all", { vault });
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

/** 一条历史快照的元信息。`id` 是毫秒时间戳(同毫秒会带 `-N` 后缀)。 */
export type NoteSnapshotEntry = {
  id: string;
  filePath: string;
  relativePath: string;
  createdAtMs: number;
  size: number;
};

export type NoteSnapshot = {
  entry: NoteSnapshotEntry;
  content: string;
};

export type RestoredNote = {
  content: string;
  /** 回滚后的新基线。不换掉的话下一次保存会撞上一个我们自己造出来的冲突。 */
  sig: NoteSig;
  entry: NoteSnapshotEntry;
};

/** 列出一条笔记的历史快照,新的在前。每个笔记最多 30 条。 */
export function listNoteSnapshots(path: string): Promise<NoteSnapshotEntry[]> {
  return invoke<NoteSnapshotEntry[]>("notebook_list_snapshots", { path });
}

export function readNoteSnapshot(path: string, entryId: string): Promise<NoteSnapshot> {
  return invoke<NoteSnapshot>("notebook_read_snapshot", { path, entryId });
}

/** 回滚到某条快照。回滚前后端会把当前内容也存成一条快照,所以这一步可撤销。 */
export function restoreNoteSnapshot(path: string, entryId: string): Promise<RestoredNote> {
  return invoke<RestoredNote>("notebook_restore_snapshot", { path, entryId });
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

/** vault 里的一个附件。`relativePath` 相对 vault 根,用来告诉用户它在哪。 */
export type Attachment = {
  path: string;
  name: string;
  relativePath: string;
  size: number;
  modifiedMs: number;
  /** image / svg / pdf / video / audio / word / sheet / slides / archive */
  kind: string;
};

/** 刚存下来的附件。`markdown` 是可以直接插进正文的那一段。 */
export type SavedAttachment = {
  path: string;
  name: string;
  /** 相对**笔记所在目录**的链接,子目录里的笔记会带 `../`。 */
  link: string;
  markdown: string;
  size: number;
};

/**
 * 把一段字节存成附件(粘贴走这条)。
 *
 * `bytes` 是 base64。剪贴板里的图本来就在内存里,没有磁盘路径可给,只能编码穿过
 * IPC;从磁盘拖入的走 `saveAttachmentFromPath`,那条不编码。
 */
export function saveAttachment(
  note: string,
  dataBase64: string,
  mime: string,
  fileName?: string,
): Promise<SavedAttachment> {
  return invoke<SavedAttachment>("notebook_attachment_save", {
    note,
    dataBase64,
    mime,
    fileName: fileName ?? null,
  });
}

/** 把磁盘上的一个文件复制进附件目录(从文件管理器拖入走这条)。 */
export function saveAttachmentFromPath(note: string, src: string): Promise<SavedAttachment> {
  return invoke<SavedAttachment>("notebook_attachment_save_from_path", { note, src });
}

/** 列出 vault 里的附件,新的在前。 */
export function listAttachments(vault: string, max?: number): Promise<Attachment[]> {
  return invoke<Attachment[]>("notebook_attachment_list", { vault, max: max ?? null });
}

/**
 * 读一个附件的原始字节。
 *
 * 前端拿它做 blob URL。不走 asset 协议是因为那要给 WebView 开一整棵目录的读权限
 * 并放宽 CSP;这条路上读取仍然过后端那道 vault allowlist。
 *
 * 后端返回的是 `ipc::Response`(原始 body),所以这里拿到的是 ArrayBuffer,而不是
 * 一个几百万元素的数字数组。
 */
export function readAttachment(path: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("notebook_attachment_read", { path });
}

/** 一条笔记在磁盘上的元数据。`createdMs` 为 null 表示文件系统不记创建时间。 */
export type NoteStat = {
  size: number;
  modifiedMs: number;
  createdMs: number | null;
};

/**
 * 读一条笔记的磁盘元数据。
 *
 * 面板里那份笔记对象的 `updatedAt` 是**打开时**的时间戳,而属性面板要回答的是
 * "这个文件现在多大、什么时候改的"。所以直接看盘,不用内存里那份。
 */
export function statNote(path: string): Promise<NoteStat> {
  return invoke<NoteStat>("notebook_note_stat", { path });
}

/**
 * 读自定义图标表(vault 相对路径 → 图标名)。
 *
 * 存在 vault 的 `.notebook/icons.json` 里,不是浏览器存储 —— 图标跟着笔记走,
 * 用户同步或搬走整个 vault 时不会留在原来那台机器上。
 */
export function readNoteIcons(vault: string): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("notebook_read_icons", { vault });
}

/** 写自定义图标表。整张表一起写,不做增量合并。 */
export function writeNoteIcons(vault: string, icons: Record<string, string>): Promise<void> {
  return invoke<void>("notebook_write_icons", { vault, icons });
}

/**
 * 扫全库的 `[[wikilink]]` 出现,给反链面板用。
 *
 * 返回的是**未解析**的原始 body + 行号 + 行预览 —— 解析规则在 `noteLinks.ts`。
 * 扫描必须在后端:反链要读全文(链接可能在最后一行),几百篇笔记意味着几百次
 * IPC 往返加上把整个 vault 的正文搬进 JS 堆。
 */
export function vaultLinks(vault: string): Promise<NoteLinkSource[]> {
  return invoke<NoteLinkSource[]>("notebook_vault_links", { vault });
}

/**
 * 扫全库的行内 `#标签` 出现,给标签面板用。
 *
 * 和 `vaultLinks` 一样只做词法提取:聚合(折大小写、数处数篇数)在 `noteTags.ts`。
 * 后端和前端共用一份归一化规则,靠两边同样的用例守住 —— 漂移的表现是"标签云里
 * 有它、重命名却改不动",而那种偏差没人会往归一化上想。
 *
 * 只覆盖正文里的行内标签;frontmatter 的 `tags:` 归属性面板。
 */
export function vaultTags(vault: string): Promise<NoteTagSource[]> {
  return invoke<NoteTagSource[]>("notebook_vault_tags", { vault });
}

/**
 * 扫全库的 frontmatter 字段,给字段浏览器用。
 *
 * 只做词法提取:聚合(折 key 大小写、数篇数、按值分组)在 `noteFields.ts`。
 * frontmatter 的边界与标题索引共用同一份解析(Rust 侧 `split_frontmatter`),否则
 * 同一篇笔记会在字段浏览器里有 `title`、在笔记列表里显示文件名。
 */
export function vaultFields(vault: string): Promise<NoteFieldSource[]> {
  return invoke<NoteFieldSource[]>("notebook_vault_fields", { vault });
}

/** 一篇被改过的笔记,`count` 是这篇里改掉的处数。 */
export type TagRenameChange = { path: string; count: number };

/** 跳过一篇的理由。文案键是 `notebook.tagSkip.<reason>`。 */
export type TagSkipReason = "notATag" | "vanished" | "tooManyFiles";

export type TagRenameSkip = { path: string; reason: TagSkipReason };

export type TagRenameFailure = { path: string; message: string };

/** 一次跨文件重命名的完整报告。 */
export type TagRenameReport = {
  changed: TagRenameChange[];
  skipped: TagRenameSkip[];
  failed: TagRenameFailure[];
};

/**
 * 跨文件把 `#old` 改成 `#new`。
 *
 * `old` 传归一化 key(大小写不敏感,和面板里那一行的聚合口径一致),`new` 是要写进
 * 文件的字面文本。
 *
 * 改写在后端按扫描器给的**字节区间**做,不是再跑一条正则 —— 面板上数得出来的处数
 * 一定改得动,而代码块 / frontmatter / `##heading` 里的字样一定不动。整次失败(新名字
 * 非法、vault 读不动)抛错;单篇失败进报告的 `failed`,不中断其余的处理。
 */
export function renameVaultTag(vault: string, old: string, next: string): Promise<TagRenameReport> {
  // Rust 侧参数名是 `new` —— 那是 TS 的保留字,所以这里的形参叫 `next`。
  return invoke<TagRenameReport>("notebook_rename_tag", { vault, old, new: next });
}

/** 索引里一篇笔记的路径与真实标题。`path` 与笔记列表里的 `id` 是同一个值。 */
export type VaultIndexEntry = {
  path: string;
  title: string;
};

/**
 * 扫全库,拿每篇笔记的**真实标题**。
 *
 * `listNotes` 只读目录项,给未读入的笔记填的是文件名 stem。而 `[[链接]]` 写的是
 * 标题、标题存在 frontmatter 里 —— 少了这份索引,指向"还没打开过的笔记"的链接
 * 全是死链,而「先写链接、之后才点开那篇笔记」恰恰是双链最常见的用法。
 */
export function vaultIndex(vault: string): Promise<VaultIndexEntry[]> {
  return invoke<VaultIndexEntry[]>("notebook_vault_index", { vault });
}
