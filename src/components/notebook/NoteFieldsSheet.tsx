/* frontmatter 字段浏览器。左侧是全库的 key,右侧是选中 key 的取值,点开一个取值
 * 看它命中哪几篇笔记。
 *
 * 为什么是铺满面板的 sheet 而不是侧栏第四档:侧栏那一列只有 190px,现在三档
 * (大纲 / 反链 / 标签)的按钮已经把它占满 —— 多两个字就会把另外两档挤成省略号。
 * 而这一档要同时显示 key、取值、命中笔记三层,一列宽根本铺不开。历史 / 回收站是
 * 同样的取舍。
 *
 * 铺在面板内部(`position:absolute; inset:0`)而不是整个窗口,和历史 / 回收站一致:
 * 随手记面板可以只占项目视图的一半,盖住整个窗口会把用户正在参照的另一半也遮掉。
 */

import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Braces, X } from "lucide-react";

import { filterFields, type FieldEntry, type FieldNoteHit } from "./noteFields";

export type NoteFieldsSheetProps = {
  entries: FieldEntry[];
  loading: boolean;
  error: string | null;
  onOpenNote: (path: string) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

/** 展开中的那一行。`kind: "empty"` 是"有 key 没值"那一行 —— 见 `emptyNotes`。 */
type OpenRow = { kind: "value"; value: string } | { kind: "empty" };

const overlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  zIndex: 30,
  display: "flex",
  background: "var(--bg-panel)",
};

const keyListStyle: CSSProperties = {
  width: 190,
  flexShrink: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderRight: "1px solid var(--border-dim)",
};

const hintStyle: CSSProperties = {
  padding: 10,
  color: "var(--text-hint)",
  fontSize: 11.5,
};

const filterStyle: CSSProperties = {
  margin: 5,
  height: 22,
  padding: "0 6px",
  border: "1px solid var(--border)",
  borderRadius: 4,
  background: "var(--bg-input, transparent)",
  color: "var(--text-primary)",
  fontSize: 11.5,
};

const headerStyle: CSSProperties = {
  minHeight: 32,
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "0 8px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11.5,
};

const rowStyle: CSSProperties = {
  width: "100%",
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "4px 8px",
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "var(--text-secondary)",
  fontSize: 11.5,
  textAlign: "left",
  cursor: "pointer",
};

/** 一条笔记行。点一下打开那篇。 */
function NoteRow({ note, onOpen }: { note: FieldNoteHit; onOpen: (path: string) => void }) {
  return (
    <button
      type="button"
      onClick={() => onOpen(note.path)}
      style={{
        ...rowStyle,
        paddingLeft: 24,
        color: "var(--text-muted)",
      }}
      title={note.path}
    >
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
        {note.title}
      </span>
    </button>
  );
}

export function NoteFieldsSheet({
  entries,
  loading,
  error,
  onOpenNote,
  onClose,
  t,
}: NoteFieldsSheetProps) {
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const [query, setQuery] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [openRow, setOpenRow] = useState<OpenRow | null>(null);

  // 打开时把焦点挪进来。不挪的话焦点还在编辑器上,下面那个 onKeyDown 根本收不到
  // Esc(事件在编辑器那棵子树里冒泡,不经过这个 div),而 Tab 会从 sheet 背后的
  // 元素开始走。历史 / 回收站同样处理。
  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  const shown = useMemo(() => filterFields(entries, query), [entries, query]);
  const selected = useMemo(
    () => entries.find((entry) => entry.key === selectedKey) ?? null,
    [entries, selectedKey],
  );

  return (
    <div
      style={overlayStyle}
      role="dialog"
      aria-modal="true"
      aria-label={t("notebook.fieldsTitle")}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        /* 不往上冒。历史 / 回收站那两个 sheet 也这么写。**目前**没有可碰撞的对象:
           面板自己没有 Esc 处理,宿主那个 window 监听要按住修饰键才进,所以去掉
           这一句测试照样绿(验过)。留着是因为它是"浮层拦掉自己的 Esc"的正确写法,
           而给面板加一层 Esc 是随时会发生的改动 —— 那时候少了它就变成一次按键关
           两层。测不出来,所以在这里写清。 */
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <div style={keyListStyle}>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("notebook.fieldsFilter")}
          aria-label={t("notebook.fieldsFilter")}
          style={filterStyle}
        />
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "0 5px 5px" }}>
          {loading ? (
            <div style={hintStyle}>{t("notebook.fieldsLoading")}</div>
          ) : error ? (
            <div style={{ ...hintStyle, color: "var(--warning)" }}>{error}</div>
          ) : entries.length === 0 ? (
            <div style={hintStyle}>{t("notebook.fieldsEmpty")}</div>
          ) : shown.length === 0 ? (
            <div style={hintStyle}>{t("notebook.fieldsNoMatch")}</div>
          ) : (
            shown.map((entry) => (
              <button
                key={entry.key}
                type="button"
                aria-pressed={entry.key === selectedKey}
                onClick={() => {
                  setSelectedKey(entry.key);
                  /* 换 key 时收起展开的那一行:留着的话新 key 里恰好有同名取值会
                     显示成已展开,而那不是用户点开的。 */
                  setOpenRow(null);
                }}
                style={{
                  ...rowStyle,
                  background: entry.key === selectedKey ? "var(--bg-hover)" : "transparent",
                }}
              >
                <span
                  style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}
                >
                  {entry.label}
                </span>
                <span style={{ color: "var(--text-hint)" }}>{entry.notes}</span>
              </button>
            ))
          )}
        </div>
      </div>
      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <div style={headerStyle}>
          <Braces size={12} aria-hidden />
          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
            {selected
              ? t("notebook.fieldsValuesOf", { key: selected.label })
              : t("notebook.fieldsTitle")}
          </span>
          <span style={{ color: "var(--text-hint)" }}>
            {selected
              ? t("notebook.fieldsNoteCount", { count: String(selected.notes) })
              : t("notebook.fieldsKeyCount", { count: String(entries.length) })}
          </span>
          <button
            ref={closeRef}
            type="button"
            aria-label={t("notebook.fieldsClose")}
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
        <div
          style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 5 }}
          data-testid="note-fields-values"
        >
          {!selected ? (
            <div style={hintStyle}>{t("notebook.fieldsPickOne")}</div>
          ) : (
            <>
              {selected.values.map((value) => {
                const open = openRow?.kind === "value" && openRow.value === value.value;
                return (
                  <div key={value.value}>
                    <button
                      type="button"
                      aria-expanded={open}
                      onClick={() =>
                        setOpenRow(open ? null : { kind: "value", value: value.value })
                      }
                      style={{
                        ...rowStyle,
                        background: open ? "var(--bg-hover)" : "transparent",
                      }}
                    >
                      <span
                        style={{
                          flex: 1,
                          minWidth: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {value.value}
                      </span>
                      <span style={{ color: "var(--text-hint)" }}>
                        {t("notebook.fieldsNoteCount", { count: String(value.notes.length) })}
                      </span>
                    </button>
                    {open
                      ? value.notes.map((note) => (
                          <NoteRow key={note.path} note={note} onOpen={onOpenNote} />
                        ))
                      : null}
                  </div>
                );
              })}
              {selected.emptyNotes.length > 0 ? (
                <div>
                  <button
                    type="button"
                    aria-expanded={openRow?.kind === "empty"}
                    onClick={() => setOpenRow(openRow?.kind === "empty" ? null : { kind: "empty" })}
                    style={{
                      ...rowStyle,
                      background: openRow?.kind === "empty" ? "var(--bg-hover)" : "transparent",
                      fontStyle: "italic",
                    }}
                  >
                    <span style={{ flex: 1, minWidth: 0 }}>{t("notebook.fieldsNoValue")}</span>
                    <span style={{ color: "var(--text-hint)" }}>
                      {t("notebook.fieldsNoteCount", {
                        count: String(selected.emptyNotes.length),
                      })}
                    </span>
                  </button>
                  {openRow?.kind === "empty"
                    ? selected.emptyNotes.map((note) => (
                        <NoteRow key={note.path} note={note} onOpen={onOpenNote} />
                      ))
                    : null}
                </div>
              ) : null}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
