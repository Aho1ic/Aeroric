/* 随手记的状态栏。
 *
 * 对着 Markio 的 StatusBar 挑过一遍:它那十来项里绝大多数是它自己的架构
 * (同步状态、git 轮询、番茄钟、后台诊断、文件监听健康度、工作区名),
 * Aeroric 这边要么不存在,要么已经有专门的视图在显示,搬过来只是重复。
 * 真正缺的只有两项:
 *
 * 1. **保存状态** —— 自动保存是静默的,用户切走之前没有任何办法确认那几个字
 *    到底落盘了没有。这是整条状态栏存在的理由。
 * 2. **笔记在 vault 里的相对路径** —— 让用户知道自己在改哪个 .md 文件。
 *
 * 字数 / 阅读时长 / 「Markdown」标签不在这里,它们已经在标题栏上了。
 */

import type { NoteSaveState } from "./useNoteAutosave";

export type NoteStatusBarProps = {
  /** 当前笔记的绝对路径。 */
  notePath: string;
  /** vault 根目录的绝对路径。null = 还没初始化完。 */
  vault: string | null;
  saveState: NoteSaveState;
  t: (key: string, vars?: Record<string, string>) => string;
};

const STATE_KEY: Record<NoteSaveState, string> = {
  pending: "notebook.saveStatePending",
  saving: "notebook.saveStateSaving",
  saved: "notebook.saveStateSaved",
  error: "notebook.saveStateError",
};

const STATE_COLOR: Record<NoteSaveState, string> = {
  // 未保存用暖色而不是红色 —— 它是正常的中间态(防抖还没到期),不是故障。
  pending: "var(--warning, #ff9500)",
  saving: "var(--accent)",
  saved: "var(--text-muted)",
  error: "var(--danger, #ff453a)",
};

/** 反斜杠归一化并去掉结尾的斜杠,好让 Windows 路径也能比。 */
function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

/**
 * 笔记相对 vault 的路径。不在 vault 里(或还不知道 vault)就退回文件名。
 *
 * 前缀比对要带上那个 `/`,否则 `<vault>-backup/a.md` 会被当成 vault 内的
 * `-backup/a.md`。
 */
export function vaultRelativePath(notePath: string, vault: string | null): string {
  const note = normalizePath(notePath);
  const fileName = note.slice(note.lastIndexOf("/") + 1);
  if (!vault) return fileName;
  const root = normalizePath(vault);
  if (!note.startsWith(`${root}/`)) return fileName;
  return note.slice(root.length + 1);
}

export function NoteStatusBar({ notePath, vault, saveState, t }: NoteStatusBarProps) {
  const relative = vaultRelativePath(notePath, vault);
  const label = t(STATE_KEY[saveState]);
  const color = STATE_COLOR[saveState];
  return (
    <div
      aria-label={t("notebook.noteStatus")}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        flexShrink: 0,
        height: 22,
        padding: "0 8px",
        borderTop: "1px solid var(--border-dim)",
        background: "var(--bg-sidebar)",
        fontSize: 10,
        color: "var(--text-muted)",
      }}
    >
      {/* role=status 让屏幕阅读器在保存状态变化时播报,不用用户主动去查。 */}
      <span
        role="status"
        style={{ display: "flex", alignItems: "center", gap: 4, color, flexShrink: 0 }}
      >
        <span
          aria-hidden="true"
          style={{ width: 6, height: 6, borderRadius: "50%", background: color }}
        />
        {label}
      </span>
      <span
        title={notePath}
        style={{
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          // 路径从**左边**截断:尾部的文件名比 vault 前缀更有信息量。
          direction: "rtl",
          textAlign: "left",
        }}
      >
        {/* direction: rtl 会把结尾的标点甩到行首,用 LRM 把整段钉成从左到右。 */}
        {"‎"}
        {relative}
      </span>
    </div>
  );
}
