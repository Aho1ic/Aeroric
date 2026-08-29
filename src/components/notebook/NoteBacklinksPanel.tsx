/* 侧栏的「反链」分区:谁引用了当前这篇笔记。
 *
 * 从 Markio 的 `Outline.tsx`(links tab 那一半)移植,但换了骨架:
 *
 * - Markio 按文件名 stem grep `[[stem`,一个文件只出一条。这里按解析规则算,
 *   一个文件里的每一处都列出来 —— 随手记的链接可以按 frontmatter 标题写,而
 *   标题和文件名可以差得很远。
 * - Markio 的 links tab 还带「未链接提及」和「一键包成 `[[..]]`」。那部分要改写
 *   别人的文件,和这里的只读视图风险完全不同一个量级,单独做。
 *
 * 扫描由父面板发起(它知道 vault 和什么时候该重扫),这个组件只负责画。
 */

import { CornerDownRight, Image as ImageIcon, RefreshCw } from "lucide-react";
import type { BacklinkGroup } from "./noteBacklinks";

export type NoteBacklinksPanelProps = {
  groups: BacklinkGroup[];
  /** 总条数(不是来源篇数)。 */
  count: number;
  loading: boolean;
  /** 扫描失败时的文案。反链是只读视图,失败就地显示,不占用面板那条错误条。 */
  error: string | null;
  /** 跳到某一篇的某一行。 */
  onJump: (path: string, line: number) => void;
  onRefresh: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function NoteBacklinksPanel({
  groups,
  count,
  loading,
  error,
  onJump,
  onRefresh,
  t,
}: NoteBacklinksPanelProps) {
  return (
    <aside
      aria-label={t("notebook.backlinks")}
      style={{ minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          padding: "5px 6px 5px 8px",
          fontSize: 10,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 0.4,
        }}
      >
        <span style={{ flex: 1, minWidth: 0 }}>
          {t("notebook.backlinkCount", {
            count: String(count),
            notes: String(groups.length),
          })}
        </span>
        <button
          type="button"
          aria-label={t("notebook.backlinkRefresh")}
          title={t("notebook.backlinkRefresh")}
          onClick={onRefresh}
          disabled={loading}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: loading ? "progress" : "pointer",
            opacity: loading ? 0.45 : 1,
            padding: 2,
            display: "flex",
            flexShrink: 0,
          }}
        >
          <RefreshCw size={11} />
        </button>
      </div>

      {error ? (
        <div
          role="alert"
          style={{
            margin: "0 8px 8px",
            padding: "6px 8px",
            borderRadius: 6,
            background: "var(--danger-subtle, var(--bg-card))",
            color: "var(--danger, var(--text-primary))",
            fontSize: 11,
            lineHeight: 1.4,
            wordBreak: "break-word",
          }}
        >
          {error}
        </div>
      ) : null}

      {/* 加载中仍然显示上一次的结果 —— 清空会让「刷新」变成一次闪烁,而反链列表
          在两次扫描之间通常几乎不变。 */}
      {groups.length === 0 && !loading && !error ? (
        <div style={{ padding: "4px 10px 12px", fontSize: 11, color: "var(--text-hint)" }}>
          {t("notebook.backlinkEmpty")}
        </div>
      ) : (
        <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 6px 10px" }}>
          {groups.map((group) => (
            <div key={group.path} style={{ marginBottom: 8 }}>
              <div
                title={group.path}
                style={{
                  padding: "0 2px 2px",
                  fontSize: 11,
                  fontWeight: 600,
                  color: "var(--text-secondary)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {group.title}
              </div>
              {group.hits.map((hit) => (
                <button
                  key={hit.line}
                  type="button"
                  /* 可及名里带上来源标题和行号:列表是按来源分组的,而屏读逐个念
                     按钮时听不到上面那个分组标题。 */
                  aria-label={t("notebook.backlinkJump", {
                    title: group.title,
                    line: String(hit.line),
                  })}
                  title={hit.preview}
                  onClick={() => onJump(group.path, hit.line)}
                  style={{
                    width: "100%",
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 3,
                    border: "none",
                    borderRadius: 4,
                    background: "transparent",
                    color: "var(--text-primary)",
                    cursor: "pointer",
                    textAlign: "left",
                    padding: "2px 4px",
                    fontSize: 11,
                    lineHeight: 1.35,
                  }}
                >
                  {hit.embed ? (
                    /* 嵌入和普通引用换一个图标:`![[..]]` 会把整篇内容搬过去,
                       改标题之类的动作影响面比一条链接大。 */
                    <ImageIcon
                      size={10}
                      aria-hidden="true"
                      style={{ marginTop: 2, flexShrink: 0, color: "var(--text-muted)" }}
                    />
                  ) : (
                    <CornerDownRight
                      size={10}
                      aria-hidden="true"
                      style={{ marginTop: 2, flexShrink: 0, color: "var(--text-muted)" }}
                    />
                  )}
                  <span
                    style={{
                      minWidth: 0,
                      overflow: "hidden",
                      display: "-webkit-box",
                      WebkitBoxOrient: "vertical",
                      WebkitLineClamp: 2,
                    }}
                  >
                    {hit.preview}
                  </span>
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}
