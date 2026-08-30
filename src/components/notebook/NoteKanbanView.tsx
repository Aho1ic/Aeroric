/* 看板视图。frontmatter 写 `view: kanban` 时,阅读态渲染这个而不是 Markdown 预览。
 *
 * 只做展示和两种写回(勾选、加卡片),两者都走面板里已有的那条路:
 * - 勾选 → `toggleTaskAtLine`,自带乐观锁和"在 updater 里算"的防丢改动
 * - 加卡片 → `appendCardToColumn`,列头原文当乐观锁
 *
 * 拖拽换列**没做**:Markio 那份也没有,而它意味着删一行 + 插一行的跨列移动 —— 在一个
 * 会被自动保存和外部编辑同时改的文件上,那是另一个量级的正确性问题,值得单独一步。
 */

import { useMemo, useState } from "react";
import { parseNoteKanban, type KanbanCard, type KanbanColumn } from "./noteKanban";

export type NoteKanbanViewProps = {
  /** 正文(不含 frontmatter)。列偏移和卡片行号都按它算。 */
  body: string;
  /** 勾选某一行。`expectChecked` 是乐观锁,传渲染那一刻看到的状态。 */
  onToggleLine: (line: number, expectChecked: boolean) => void;
  /** 往某列末尾加一条。只读时不传 —— 那时候不显示添加入口。 */
  onAppend?: (column: KanbanColumn, text: string) => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export function NoteKanbanView({ body, onToggleLine, onAppend, t }: NoteKanbanViewProps) {
  const board = useMemo(() => parseNoteKanban(body), [body]);
  /* 正在输入的那一列,用**偏移**标识而不是标题 —— 同名两列用标题会同时打开两个输入框。 */
  const [addingAt, setAddingAt] = useState<number | null>(null);
  const [draft, setDraft] = useState("");

  const closeDraft = () => {
    setAddingAt(null);
    setDraft("");
  };

  const commit = (column: KanbanColumn) => {
    const text = draft.trim();
    closeDraft();
    if (text && onAppend) onAppend(column, text);
  };

  if (board.columns.length === 0) {
    /* 外层仍然带 `notebook-kanban`:没有列的看板还是看板。少了它,"这篇是看板但空着"
       和"这篇根本没进看板"在 DOM 上分不出来 —— 前者该给写法说明,后者是接线坏了。 */
    return (
      <div className="notebook-kanban notebook-kanban-empty-state">
        <p>{t("notebook.kanbanNoColumns")}</p>
        <p className="hint">{t("notebook.kanbanHowTo")}</p>
      </div>
    );
  }

  return (
    <div className="notebook-kanban">
      <div className="notebook-kanban-bar">
        <span className="count">
          {t("notebook.kanbanProgress", {
            done: board.done,
            total: board.total,
            percent: board.percent,
          })}
        </span>
        <span className="track" aria-hidden="true">
          <span className="fill" style={{ width: `${board.percent}%` }} />
        </span>
        {/* 未归属的如实报出来:看板上看不见它们,但它们在文件里。 */}
        {board.unplaced > 0 && (
          <span className="unplaced">
            {t("notebook.kanbanUnplaced", { count: board.unplaced })}
          </span>
        )}
      </div>

      <div className="notebook-kanban-board">
        {board.columns.map((column) => {
          const open = column.cards.reduce((sum, card) => sum + (card.checked ? 0 : 1), 0);
          const adding = addingAt === column.offset;
          return (
            /* `aria-label` 不能省:没有可及名字的 `<section>` 根本不是 landmark,
               读屏用户列不出「这块板有哪几列」,只能一张一张卡片往下读。 */
            <section key={column.offset} className="notebook-kanban-col" aria-label={column.title}>
              <header className="notebook-kanban-col-head">
                {column.emoji && <span aria-hidden="true">{column.emoji}</span>}
                <h3>{column.title}</h3>
                <span className="n" title={t("notebook.kanbanOpenCount", { count: open })}>
                  {open}
                </span>
              </header>
              <div className="notebook-kanban-col-list">
                {column.cards.map((card) => (
                  <CardRow
                    key={card.line}
                    card={card}
                    onToggle={() => onToggleLine(card.line, card.checked)}
                    t={t}
                  />
                ))}
                {adding && onAppend && (
                  <input
                    className="notebook-kanban-draft"
                    autoFocus
                    type="text"
                    value={draft}
                    aria-label={t("notebook.kanbanAddTo", { column: column.title })}
                    placeholder={t("notebook.kanbanAddPlaceholder")}
                    onChange={(event) => setDraft(event.target.value)}
                    onBlur={() => commit(column)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        commit(column);
                      } else if (event.key === "Escape") {
                        event.preventDefault();
                        closeDraft();
                      }
                    }}
                  />
                )}
                {!adding && onAppend && (
                  <button
                    type="button"
                    className="notebook-kanban-add"
                    onClick={() => {
                      setAddingAt(column.offset);
                      setDraft("");
                    }}
                  >
                    {t("notebook.kanbanAdd")}
                  </button>
                )}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function CardRow({
  card,
  onToggle,
  t,
}: {
  card: KanbanCard;
  onToggle: () => void;
  t: NoteKanbanViewProps["t"];
}) {
  const hasMeta = card.tags.length > 0 || card.due || card.priority;
  return (
    <article className={"notebook-kanban-card" + (card.checked ? " done" : "")}>
      <div className="row">
        <input
          type="checkbox"
          checked={card.checked}
          onChange={onToggle}
          /* 空文本的卡片(整行只有标记)也要有可读的名字,否则读屏念出来是空的。 */
          aria-label={card.text || card.raw}
        />
        {/* 卡片文本按纯文本显示,不渲染 Markdown:一张卡里出现表格或代码块会把列撑破,
            而看板的用处是"一眼看完一列"。要看排版就切回阅读态。 */}
        <span className="text">{card.text || card.raw}</span>
        {/* 优先级复用收集箱那三条文案(`notebook.taskPriority.*`),不另起一套 ——
            同一个概念两处文案迟早会说成两个词。 */}
        {card.priority && (
          <span
            className={`prio ${card.priority}`}
            title={t(`notebook.taskPriority.${card.priority}`)}
          />
        )}
      </div>
      {hasMeta && (
        <div className="meta">
          {card.tags.map((tag) => (
            <span key={tag} className="tag">
              #{tag}
            </span>
          ))}
          {card.due && <span className="due">{card.due}</span>}
        </div>
      )}
      {typeof card.progress === "number" && (
        <div className="progress" title={`${card.progress}%`}>
          <span style={{ width: `${card.progress}%` }} />
        </div>
      )}
    </article>
  );
}
