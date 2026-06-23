import { useEffect, useMemo, useState } from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { FileText, Plus, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";

type NotebookNote = {
  id: string;
  title: string;
  body: string;
  updatedAt: number;
};

const STORAGE_KEY = "aeroric:notebook:v1";

function createNote(): NotebookNote {
  const now = Date.now();
  return {
    id: `note-${now}-${Math.random().toString(36).slice(2, 8)}`,
    title: "Untitled memo",
    body: "",
    updatedAt: now,
  };
}

function loadNotes(): NotebookNote[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Array<Partial<NotebookNote>>;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((item): item is NotebookNote => Boolean(item.id && typeof item.title === "string"))
      .map((item) => ({
        id: item.id,
        title: item.title || "Untitled memo",
        body: typeof item.body === "string" ? item.body : "",
        updatedAt: typeof item.updatedAt === "number" ? item.updatedAt : 0,
      }))
      .sort((a, b) => b.updatedAt - a.updatedAt);
  } catch {
    return [];
  }
}

function saveNotes(notes: NotebookNote[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(notes));
}

function renderMarkdown(body: string): string {
  return DOMPurify.sanitize(marked(body || "", { async: false }) as string);
}

export function NotebookPanel({ width = "100%" }: { width?: number | string }) {
  const { t } = useI18n();
  const [notes, setNotes] = useState<NotebookNote[]>(() => loadNotes());
  const [activeId, setActiveId] = useState<string | null>(() => notes[0]?.id ?? null);
  const [mode, setMode] = useState<"edit" | "read">("edit");
  const activeNote = notes.find((note) => note.id === activeId) ?? notes[0] ?? null;
  const markdownHtml = useMemo(() => renderMarkdown(activeNote?.body ?? ""), [activeNote?.body]);

  useEffect(() => {
    saveNotes(notes);
  }, [notes]);

  useEffect(() => {
    if (!activeId && notes[0]) setActiveId(notes[0].id);
    if (activeId && notes.length > 0 && !notes.some((note) => note.id === activeId)) {
      setActiveId(notes[0].id);
    }
  }, [activeId, notes]);

  const updateActiveNote = (patch: Partial<Pick<NotebookNote, "title" | "body">>) => {
    if (!activeNote) return;
    const updatedAt = Date.now();
    setNotes((current) =>
      current
        .map((note) => (note.id === activeNote.id ? { ...note, ...patch, updatedAt } : note))
        .sort((a, b) => b.updatedAt - a.updatedAt),
    );
  };

  const addNote = () => {
    const note = createNote();
    setNotes((current) => [note, ...current]);
    setActiveId(note.id);
    setMode("edit");
  };

  const deleteActiveNote = () => {
    if (!activeNote) return;
    setNotes((current) => current.filter((note) => note.id !== activeNote.id));
  };

  return (
    <section
      aria-label={t("notebook.title")}
      style={{
        width,
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        display: "grid",
        gridTemplateColumns: "150px minmax(0, 1fr)",
        background: "var(--bg-panel)",
        color: "var(--text-primary)",
      }}
    >
      <aside
        style={{
          minWidth: 0,
          borderRight: "1px solid var(--border-dim)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div style={{ height: 38, display: "flex", alignItems: "center", gap: 6, padding: "0 8px", borderBottom: "1px solid var(--border-dim)" }}>
          <FileText size={14} />
          <strong style={{ fontSize: 12, flex: 1 }}>{t("notebook.title")}</strong>
          <button type="button" aria-label={t("notebook.newMemo")} title={t("notebook.newMemo")} onClick={addNote} style={{ border: "none", background: "transparent", color: "var(--text-muted)", cursor: "pointer", padding: 3 }}>
            <Plus size={14} />
          </button>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: 6, display: "flex", flexDirection: "column", gap: 4 }}>
          {notes.length === 0 ? (
            <div style={{ padding: 10, fontSize: 12, color: "var(--text-hint)", lineHeight: 1.4 }}>{t("notebook.empty")}</div>
          ) : (
            notes.map((note) => (
              <button
                type="button"
                key={note.id}
                title={note.title}
                onClick={() => setActiveId(note.id)}
                style={{
                  minHeight: 28,
                  border: "1px solid transparent",
                  borderRadius: 6,
                  background: note.id === activeNote?.id ? "var(--bg-selected)" : "transparent",
                  color: "var(--text-primary)",
                  textAlign: "left",
                  padding: "5px 7px",
                  cursor: "pointer",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  fontSize: 12,
                }}
              >
                {note.title || t("notebook.untitled")}
              </button>
            ))
          )}
        </div>
      </aside>
      <div style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        {activeNote ? (
          <>
            <div style={{ height: 38, display: "flex", alignItems: "center", gap: 8, padding: "0 10px", borderBottom: "1px solid var(--border-dim)" }}>
              <input
                aria-label={t("notebook.memoName")}
                value={activeNote.title}
                onChange={(event) => updateActiveNote({ title: event.currentTarget.value })}
                style={{ flex: 1, minWidth: 0, border: "none", outline: "none", background: "transparent", color: "var(--text-primary)", fontSize: 13, fontWeight: 700 }}
              />
              <button type="button" onClick={() => setMode((current) => (current === "edit" ? "read" : "edit"))} style={{ height: 26, border: "1px solid var(--border-medium)", borderRadius: 6, background: "var(--bg-card)", color: "var(--text-primary)", cursor: "pointer", padding: "0 8px", fontSize: 12 }}>
                {mode === "edit" ? t("notebook.read") : t("notebook.edit")}
              </button>
              <button type="button" aria-label={t("common.delete")} title={t("common.delete")} onClick={deleteActiveNote} style={{ border: "none", background: "transparent", color: "var(--text-muted)", cursor: "pointer", padding: 4 }}>
                <Trash2 size={14} />
              </button>
            </div>
            {mode === "edit" ? (
              <textarea
                aria-label={t("notebook.memoContent")}
                value={activeNote.body}
                onChange={(event) => updateActiveNote({ body: event.currentTarget.value })}
                spellCheck={false}
                style={{ flex: 1, minHeight: 0, resize: "none", border: "none", outline: "none", background: "var(--bg-panel)", color: "var(--text-primary)", padding: 12, fontFamily: "var(--font-mono)", fontSize: 12.5, lineHeight: 1.6 }}
              />
            ) : (
              <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 14 }}>
                <div className="md-preview notebook-markdown-preview" dangerouslySetInnerHTML={{ __html: markdownHtml }} />
              </div>
            )}
          </>
        ) : (
          <div style={{ margin: "auto", color: "var(--text-hint)", fontSize: 12 }}>{t("notebook.empty")}</div>
        )}
      </div>
    </section>
  );
}
