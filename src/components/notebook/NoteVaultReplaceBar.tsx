/* 全库替换条 + 预览列表(⌘⇧F 面板下半部分)。
 *
 * 只画界面。预览与落笔都由面板发起 —— 它持有 vault 路径、笔记列表和 `settleSave`,
 * 而后两者是这条链上的关键:后端读的是磁盘,内存里没落盘的编辑必须先等落完。
 *
 * **不给「直接全部替换」的入口。** 全库替换会一次改动几十个文件,而且不进撤销栈
 * (改的是磁盘,不是编辑器文档)。必须先预览、确认过命中,按钮才亮。
 */

import { Loader2, Replace } from "lucide-react";

import {
  previewCounts,
  type VaultReplacePreview,
  type VaultReplaceSummary,
} from "./noteVaultReplace";

export type NoteVaultReplaceBarProps = {
  /** 替换成什么。空串合法 —— 那是「删掉命中」。 */
  value: string;
  onValueChange: (value: string) => void;
  /** null = 还没预览过。 */
  preview: VaultReplacePreview | null;
  /** 被取消勾选的文件(预览给的路径口径)。 */
  excluded: ReadonlySet<string>;
  onToggleFile: (path: string) => void;
  /** 预览或落笔在飞。 */
  busy: boolean;
  /** 上一次落笔的结果。 */
  summary: VaultReplaceSummary | null;
  /** 查询为空时预览按钮不可用 —— 空查询后端会直接返回零命中。 */
  canPreview: boolean;
  onPreview: () => void;
  onApply: () => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export function NoteVaultReplaceBar({
  value,
  onValueChange,
  preview,
  excluded,
  onToggleFile,
  busy,
  summary,
  canPreview,
  onPreview,
  onApply,
  t,
}: NoteVaultReplaceBarProps) {
  const counts = preview ? previewCounts(preview, excluded) : { files: 0, matches: 0 };
  const canApply = !busy && counts.matches > 0;

  return (
    <div style={{ borderTop: "1px solid var(--border-dim)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "6px 9px" }}>
        <Replace size={13} color="var(--text-muted)" />
        <input
          aria-label={t("notebook.replaceVault")}
          value={value}
          onChange={(event) => onValueChange(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            onPreview();
          }}
          placeholder={t("notebook.replaceVaultPlaceholder")}
          style={{
            flex: 1,
            minWidth: 0,
            height: 26,
            border: "1px solid var(--border-medium)",
            borderRadius: 6,
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            padding: "0 8px",
            fontSize: 12,
            outline: "none",
          }}
        />
        <button
          type="button"
          disabled={busy || !canPreview}
          onClick={onPreview}
          style={{
            height: 24,
            border: "1px solid var(--border-medium)",
            borderRadius: 5,
            background: "transparent",
            color: busy || !canPreview ? "var(--text-muted)" : "var(--text-secondary)",
            padding: "0 8px",
            cursor: busy || !canPreview ? "default" : "pointer",
            fontSize: 11,
          }}
        >
          {t("notebook.replaceVaultPreview")}
        </button>
        <button
          type="button"
          disabled={!canApply}
          onClick={onApply}
          style={{
            height: 24,
            border: `1px solid ${canApply ? "var(--danger)" : "var(--border-medium)"}`,
            borderRadius: 5,
            background: canApply
              ? "color-mix(in srgb, var(--danger) 14%, transparent)"
              : "transparent",
            color: canApply ? "var(--danger)" : "var(--text-muted)",
            padding: "0 8px",
            cursor: canApply ? "pointer" : "default",
            fontSize: 11,
          }}
        >
          {t("notebook.replaceVaultApply")}
        </button>
      </div>
      {/* `role="status"` 而不是光一个 `aria-live` —— 这个对话框里已经有搜索那条状态行
          了,两条都只是裸 `aria-live` 的话读屏念出来分不出是哪一条在说话。 */}
      <div
        role="status"
        aria-live="polite"
        style={{ padding: "0 10px 5px", fontSize: 11, color: "var(--text-muted)" }}
      >
        {busy ? (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
            <Loader2 size={11} />
            {t("notebook.replaceVaultRunning")}
          </span>
        ) : summary ? (
          /* 跳过数要说出来:跳过的意思是「预览之后那个文件被改过」,而用户看到的是
             「点了全部替换,结果只改了一部分」—— 不说的话没人能知道为什么。 */
          <span>
            {t("notebook.replaceVaultDone", {
              applied: summary.replacementsApplied,
              files: summary.filesChanged,
            })}
            {summary.replacementsSkipped > 0
              ? ` · ${t("notebook.replaceVaultSkipped", { skipped: summary.replacementsSkipped })}`
              : ""}
          </span>
        ) : preview ? (
          <span>
            {counts.matches > 0
              ? t("notebook.replaceVaultSummary", {
                  matches: counts.matches,
                  files: counts.files,
                })
              : t("notebook.replaceVaultEmpty")}
            {preview.truncated ? ` · ${t("notebook.replaceVaultTruncated")}` : ""}
          </span>
        ) : (
          <span>{t("notebook.replaceVaultHint")}</span>
        )}
      </div>
      {preview && preview.files.length > 0 && (
        <div style={{ maxHeight: 180, overflow: "auto", padding: "0 6px 8px" }}>
          {preview.files.map((file) => {
            const on = !excluded.has(file.path);
            /* 分组名字不能光是文件名:搜索结果那半边已经按文件名分了组,两边同时在场时
               读屏会念出两个同名分区,而它们一个是"搜到了"一个是"要改成"。 */
            return (
              <section
                key={file.path}
                aria-label={t("notebook.replaceVaultFileGroup", { name: file.name })}
                style={{ marginBottom: 6 }}
              >
                <label
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 5,
                    padding: "3px 4px",
                    fontSize: 11,
                    fontWeight: 600,
                    color: "var(--text-secondary)",
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={on}
                    onChange={() => onToggleFile(file.path)}
                    /* 可及名字带上文件名:一屏十几个复选框,只念「复选框」分不出是哪篇。 */
                    aria-label={t("notebook.replaceVaultFileToggle", { name: file.name })}
                  />
                  {file.name}
                  <span style={{ fontWeight: 400, color: "var(--text-muted)" }}>
                    {file.matches.length}
                  </span>
                </label>
                {file.matches.map((match) => (
                  <div
                    key={`${match.start}:${match.end}`}
                    style={{
                      display: "flex",
                      gap: 7,
                      padding: "2px 5px 2px 22px",
                      fontSize: 11,
                      lineHeight: 1.5,
                      color: on ? "var(--text-secondary)" : "var(--text-muted)",
                      fontFamily: "var(--font-mono, monospace)",
                    }}
                  >
                    <span style={{ flex: "0 0 auto", color: "var(--text-muted)" }}>
                      {match.line}
                    </span>
                    {/* 旧 → 新 两段并排。不画整行:整行里真正要看的就是这两段,而行文本
                        常常是表格或代码,铺开会把列表撑破。 */}
                    <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
                      <del style={{ color: "var(--danger)", textDecoration: "line-through" }}>
                        {match.matchText}
                      </del>
                      <span style={{ margin: "0 5px", color: "var(--text-muted)" }}>→</span>
                      <ins
                        style={{ color: "var(--success, var(--accent))", textDecoration: "none" }}
                      >
                        {match.replacementText || t("notebook.replaceVaultEmptyText")}
                      </ins>
                    </span>
                  </div>
                ))}
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
