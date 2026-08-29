/* 一条笔记的属性:磁盘事实(大小 / 修改时间 / 创建时间)+ 内容统计(字数 / 标题数 /
 * 预计阅读时间)。
 *
 * 磁盘那半边来自 `notebook_note_stat`,不是面板内存里那份笔记:后者的 `updatedAt`
 * 是**打开时**的时间戳,而这里要回答"这个文件现在多大、什么时候改的"。内容那半边
 * 反过来 —— 用的是编辑器里的当前文本,包括还没保存的编辑,因为用户问"这篇多少字"
 * 问的是眼前这篇。两半边的口径不同,所以分成两组显示。
 *
 * Markio 的原版还有 tags / mentions(来自 vault 索引)和收藏 / 颜色 / 标记(来自
 * 文件元数据 store)。那两个 store 属于 P4,Aeroric 还没有,所以这一版只做现在
 * 真实存在的字段 —— 显示一组永远为空的 tags 比不显示更让人困惑。
 *
 * 铺在面板内部而不是整个窗口,和版本历史 / 回收站一致。
 */

import { useEffect, useRef, type CSSProperties } from "react";
import { X } from "lucide-react";

import type { NoteStat } from "./notebookApi";

/**
 * 属性面板的状态。
 *
 * `noteId` 存在状态里而不是跟着 `activeNote` 走,和历史面板同一个理由:面板开着的
 * 时候别处换掉当前笔记,不能让它悄悄变成另一条笔记的属性。
 */
export type NotePropertiesState = {
  noteId: string;
  stat: NoteStat | null;
  loading: boolean;
  error: string | null;
};

/** 初始状态。`useState` 的初值不能共享同一个对象引用,所以是构造器不是常量。 */
export function freshPropertiesState(noteId: string): NotePropertiesState {
  return { noteId, stat: null, loading: true, error: null };
}

export type NotePropertiesSheetProps = {
  noteTitle: string;
  /** 笔记的绝对路径。 */
  notePath: string;
  /** 相对 vault 根的路径,null 表示算不出来(笔记不在当前 vault 下)。 */
  relativePath: string | null;
  stat: NoteStat | null;
  loading: boolean;
  error: string | null;
  /** 编辑器里的当前文本统计。 */
  words: number;
  headings: number;
  readingMinutes: number;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

const overlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  zIndex: 30,
  display: "flex",
  flexDirection: "column",
  background: "var(--bg-panel)",
};

const headerStyle: CSSProperties = {
  minHeight: 32,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 8px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11.5,
};

const sectionStyle: CSSProperties = {
  padding: "8px 10px 2px",
  color: "var(--text-hint)",
  fontSize: 10.5,
  textTransform: "uppercase",
  letterSpacing: 0.4,
};

const rowStyle: CSSProperties = {
  display: "flex",
  alignItems: "baseline",
  gap: 10,
  padding: "3px 10px",
  fontSize: 11.5,
};

const keyStyle: CSSProperties = {
  flex: "0 0 88px",
  color: "var(--text-hint)",
};

const valueStyle: CSSProperties = {
  flex: 1,
  color: "var(--text-primary)",
  // 路径长起来会顶破面板,而面板可能只有 400px 宽。
  overflowWrap: "anywhere",
};

/** 字节数 → 人读的大小。 */
export function formatNoteSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

/**
 * epoch 毫秒 → `YYYY-MM-DD HH:MM`。
 *
 * 不用 `toLocaleString`:它按系统区域给出五花八门的顺序(`3/7/2026` 到底是 3 月 7
 * 还是 7 月 3 号),而这里只需要一个所有人都读得一样的写法。
 */
export function formatNoteTime(ms: number | null): string | null {
  if (!ms) return null;
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return null;
  const pad = (value: number) => String(value).padStart(2, "0");
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    ` ${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}

export function NotePropertiesSheet({
  noteTitle,
  notePath,
  relativePath,
  stat,
  loading,
  error,
  words,
  headings,
  readingMinutes,
  onClose,
  t,
}: NotePropertiesSheetProps) {
  const closeRef = useRef<HTMLButtonElement | null>(null);

  // 打开时把焦点挪进来:不挪的话 Esc 会被编辑器的按键处理先吃掉。
  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  const modified = formatNoteTime(stat?.modifiedMs ?? null);
  const created = formatNoteTime(stat?.createdMs ?? null);

  return (
    <div
      style={overlayStyle}
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.propertiesTitle")}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        // 拦住:面板外面还有 window 级的 Esc 监听(会去关整个视图)。
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <div style={headerStyle}>
        <span>{t("notebook.propertiesTitle")}</span>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("notebook.propertiesClose")}
          onClick={onClose}
          style={{
            marginLeft: "auto",
            display: "flex",
            padding: 3,
            border: "none",
            borderRadius: 4,
            background: "transparent",
            color: "var(--text-hint)",
            cursor: "pointer",
          }}
        >
          <X size={13} aria-hidden />
        </button>
      </div>

      <div style={{ flex: 1, overflow: "auto", paddingBottom: 8 }}>
        <div style={sectionStyle}>{t("notebook.propertiesFile")}</div>
        <div style={rowStyle}>
          <span style={keyStyle}>{t("notebook.propertiesName")}</span>
          <span style={valueStyle}>{noteTitle}</span>
        </div>
        <div style={rowStyle}>
          <span style={keyStyle}>{t("notebook.propertiesLocation")}</span>
          {/* 相对路径更好读,但完整路径要能拿到 —— 所以它进 title。 */}
          <span
            style={{ ...valueStyle, fontFamily: "var(--font-mono, monospace)" }}
            title={notePath}
          >
            {relativePath ?? notePath}
          </span>
        </div>
        {error ? (
          <div style={{ ...rowStyle, color: "var(--warning)" }}>
            <span style={keyStyle}>·</span>
            <span style={valueStyle} role="alert">
              {error}
            </span>
          </div>
        ) : loading ? (
          <div style={rowStyle}>
            <span style={keyStyle}>·</span>
            <span style={{ ...valueStyle, color: "var(--text-hint)" }}>
              {t("notebook.propertiesLoading")}
            </span>
          </div>
        ) : (
          <>
            <div style={rowStyle}>
              <span style={keyStyle}>{t("notebook.propertiesSize")}</span>
              <span style={valueStyle}>{formatNoteSize(stat?.size ?? 0)}</span>
            </div>
            <div style={rowStyle}>
              <span style={keyStyle}>{t("notebook.propertiesModified")}</span>
              <span style={valueStyle}>{modified ?? t("notebook.propertiesUnknown")}</span>
            </div>
            {/* 创建时间在部分 Linux 文件系统上根本不记 —— 那时候整行不显示,
                而不是显示一个 1970。 */}
            {created && (
              <div style={rowStyle}>
                <span style={keyStyle}>{t("notebook.propertiesCreated")}</span>
                <span style={valueStyle}>{created}</span>
              </div>
            )}
          </>
        )}

        <div style={sectionStyle}>{t("notebook.propertiesContent")}</div>
        <div style={rowStyle}>
          <span style={keyStyle}>{t("notebook.propertiesWords")}</span>
          <span style={valueStyle} data-testid="note-properties-words">
            {words}
          </span>
        </div>
        <div style={rowStyle}>
          <span style={keyStyle}>{t("notebook.propertiesHeadings")}</span>
          <span style={valueStyle} data-testid="note-properties-headings">
            {headings}
          </span>
        </div>
        <div style={rowStyle}>
          <span style={keyStyle}>{t("notebook.propertiesReading")}</span>
          <span style={valueStyle}>
            {t("notebook.propertiesReadingValue", { count: String(readingMinutes) })}
          </span>
        </div>
        {/* 内容那一组算的是编辑器里的当前文本,磁盘那一组算的是文件 —— 没保存的
            编辑会让两边对不上,不说清楚的话用户会以为哪个数错了。 */}
        <div style={{ ...rowStyle, paddingTop: 6, color: "var(--text-hint)" }}>
          <span style={keyStyle}>·</span>
          <span style={valueStyle}>{t("notebook.propertiesContentHint")}</span>
        </div>
      </div>
    </div>
  );
}
