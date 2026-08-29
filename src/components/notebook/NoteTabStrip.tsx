/* 面板内部的笔记 tab 条。
 *
 * 和 Aeroric 主界面的编辑器 tab 无关 —— 这一条完全活在随手记面板里,只管
 * 「我这会儿开着哪几条笔记」。会话内状态,不落盘。
 *
 * 在紧凑档里它是主要的导航方式:那一档笔记列表默认收起,tab 条就是唯一能一眼
 * 看到多条笔记的地方。
 *
 * 脏标记用的是自动保存的状态,不是自己另算一份:
 * - pending / saving:圆点。会在一秒内自己消失,不拦关闭。
 * - error:感叹号 + 危险色。这一档关闭要确认 —— 那条编辑真的会丢。
 */

import { AlertCircle, X } from "lucide-react";
import type { NoteSaveState } from "./useNoteAutosave";

export type NoteTabItem = {
  id: string;
  title: string;
  saveState: NoteSaveState;
};

export type NoteTabStripProps = {
  tabs: NoteTabItem[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteTabStrip({ tabs, activeId, onSelect, onClose, t }: NoteTabStripProps) {
  // 只开着一条时不占那 30px:tab 条在那种情况下没有信息量,而随手记大多数时候
  // 就是开着一条。
  if (tabs.length < 2) return null;

  return (
    <div
      role="tablist"
      aria-label={t("notebook.openNotes")}
      style={{
        display: "flex",
        alignItems: "stretch",
        gap: 2,
        padding: "3px 4px 0",
        borderBottom: "1px solid var(--border-dim)",
        overflowX: "auto",
        flexShrink: 0,
        scrollbarWidth: "none",
      }}
    >
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        const failed = tab.saveState === "error";
        const busy = tab.saveState === "pending" || tab.saveState === "saving";
        const label = tab.title || t("notebook.untitled");
        return (
          <div
            key={tab.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 2,
              maxWidth: 168,
              borderRadius: "6px 6px 0 0",
              background: active ? "var(--bg-panel)" : "transparent",
              border: "1px solid",
              borderColor: active ? "var(--border-dim)" : "transparent",
              borderBottom: "none",
              paddingRight: 2,
            }}
          >
            <button
              type="button"
              role="tab"
              aria-selected={active}
              /* 保存失败要能被读屏听见:状态栏只播报当前那条,非当前的 tab 失败了
                 就没有别的渠道。正常态不给 aria-label,名字由文本本身来 —— 少一层
                 需要跟着标题同步的东西。 */
              aria-label={failed ? t("notebook.tabSaveFailed", { name: label }) : undefined}
              title={failed ? t("notebook.tabSaveFailed", { name: label }) : label}
              onClick={() => onSelect(tab.id)}
              style={{
                minWidth: 0,
                maxWidth: 132,
                height: 26,
                border: "none",
                background: "transparent",
                color: active ? "var(--text-primary)" : "var(--text-muted)",
                cursor: "pointer",
                padding: "0 4px 0 8px",
                display: "flex",
                alignItems: "center",
                gap: 5,
                fontSize: 12,
                fontWeight: active ? 700 : 400,
                fontFamily: "var(--font-ui)",
              }}
            >
              {failed ? (
                /* 图标本身是装饰性的。给它 aria-label 会把那句话拼进 tab 的可访问
                   名字里(变成「Save failed Second」),读屏里念起来别扭,查询也
                   会跟着漂。状态改由 tab 自己的 aria-label 表达,见下面。 */
                <AlertCircle
                  size={11}
                  color="var(--danger, currentColor)"
                  aria-hidden
                  style={{ flexShrink: 0 }}
                />
              ) : (
                busy && (
                  /* 未落盘的圆点。一秒内会自己消失,所以不给它 aria-label ——
                     读屏没必要念一个转瞬即逝的状态,保存状态栏已经在播报了。 */
                  <span
                    aria-hidden
                    style={{
                      width: 5,
                      height: 5,
                      borderRadius: "50%",
                      background: "var(--text-hint)",
                      flexShrink: 0,
                    }}
                  />
                )
              )}
              <span
                style={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {label}
              </span>
            </button>
            <button
              type="button"
              aria-label={t("notebook.closeTab", { name: label })}
              title={t("notebook.closeTab", { name: label })}
              onClick={() => onClose(tab.id)}
              style={{
                width: 18,
                height: 18,
                border: "none",
                borderRadius: 4,
                background: "transparent",
                color: "var(--text-hint)",
                cursor: "pointer",
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                padding: 0,
                flexShrink: 0,
              }}
            >
              <X size={11} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
