export type CreateKind = "file" | "folder";

/**
 * `read_dir_entries`(本地)与 `wsl_read_dir_entries` / `remote_read_dir_entries`
 * 返回的目录项。字段名逐字对齐 Rust 侧 `fs::FsEntry` / `posix_fs::PosixFsEntry`
 * —— 那两个结构体没有 `rename_all`,发出来的就是下划线名字。别在这里写 camelCase:
 * `modifiedAtMs` 曾经这么写过,于是"按修改时间排序"读到的恒为 undefined,
 * 静默退化成按名字排序。Rust 侧 `fs_entry_wire_field_names_match_the_frontend_type`
 * 锁住键集。
 */
export interface FsEntry {
  name: string;
  path: string;
  is_dir: boolean;
  extension?: string;
  modified_at_ms?: number | null;
  is_gitignored: boolean;
  /** 条目自身是符号链接。`is_dir` 跟随链接,所以指向目录的链接两者同时为真。 */
  is_symlink?: boolean;
}

export interface TreeNode extends FsEntry {
  children: TreeNode[] | null; // null = not loaded yet
  expanded: boolean;
}

export interface ProjectFileSearchResult {
  path: string;
  name: string;
  dir: string;
  extension?: string;
}

export type FlatRow =
  | { kind: "node"; node: TreeNode; depth: number }
  | { kind: "input"; parentPath: string; depth: number; createKind: CreateKind };

export interface ContextMenuState {
  x: number;
  y: number;
  path: string;
  isDir: boolean;
  isRoot: boolean;
}

export const ROW_HEIGHT = 22;
export const AUTO_REFRESH_MS = 2500;
export const GITIGNORED_COLOR = "var(--icon-file-ignored)";
export const FILE_TREE_HOVER_BG = "color-mix(in srgb, var(--accent) 7%, transparent)";
