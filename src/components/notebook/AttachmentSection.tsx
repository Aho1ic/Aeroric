/* 笔记列表底部的「附件」分区。
 *
 * 移植自 Markio 的 `AttachmentSection.tsx`。它那版把整个 workspace 里的非 markdown
 * 文件平铺出来,点一下在 Finder 里显示 —— 这里保留同样的形状,但加了「插入到当前
 * 笔记」:随手记里附件的主要用途是被引用,而不是被在文件管理器里找到。
 *
 * 默认折叠,展开才扫盘。扫的是整个 vault(见 `attachments::list`),图多的仓库这
 * 一下不便宜,不该在每次打开面板时都付。
 */

import { useCallback, useEffect, useState } from "react";
import {
  Archive,
  File as FileIcon,
  FileSpreadsheet,
  FileText,
  Image as ImageIcon,
  Music,
  Paperclip,
  RefreshCw,
  Video,
} from "lucide-react";
import { listAttachments, type Attachment } from "./notebookApi";

/** 后端 `kind_of` 的取值 → 图标。认不出的类型退回通用文件图标。 */
const KIND_ICONS: Record<string, typeof FileIcon> = {
  image: ImageIcon,
  svg: ImageIcon,
  pdf: FileText,
  video: Video,
  audio: Music,
  word: FileText,
  sheet: FileSpreadsheet,
  slides: ImageIcon,
  archive: Archive,
};

/** 人类可读的大小。附件面板里主要用来看"这张图是不是太大了"。 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export type AttachmentSectionProps = {
  /** 当前 vault。还没准备好时为 null / 空串 —— 这时候整个分区不渲染。 */
  vault: string | null;
  /** 把一个附件插入当前笔记。没有打开的笔记时为 undefined,插入按钮随之消失。 */
  onInsert?: (attachment: Attachment) => void;
  /** 在系统文件管理器里显示。 */
  onReveal: (attachment: Attachment) => void;
  /**
   * 外部动作导致附件集合变了(刚粘了一张图)。变了就重扫一遍。
   *
   * 用一个计数器而不是让面板直接调 `refresh`:分区可能是折叠的,那时候不该扫盘,
   * 而"折叠期间攒下的变更要在展开时体现"这件事用计数器天然成立。
   */
  refreshToken: number;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function AttachmentSection({
  vault,
  onInsert,
  onReveal,
  refreshToken,
  t,
}: AttachmentSectionProps) {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<Attachment[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** 手动刷新用。和 `refreshToken` 分开是为了不去改父组件的状态。 */
  const [localToken, setLocalToken] = useState(0);

  const refresh = useCallback(() => setLocalToken((value) => value + 1), []);

  useEffect(() => {
    if (!open || !vault) return;
    let cancelled = false;
    setLoading(true);
    listAttachments(vault)
      .then((listed) => {
        if (cancelled) return;
        setItems(listed);
        setError(null);
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        // 扫不出来要说出来。静默显示成空列表的话用户会以为附件都没了。
        setItems([]);
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, vault, refreshToken, localToken]);

  if (!vault) return null;

  const hint = (text: string) => (
    <div style={{ padding: "6px 10px", fontSize: 11, color: "var(--text-hint)", lineHeight: 1.4 }}>
      {text}
    </div>
  );

  return (
    <div style={{ borderTop: "1px solid var(--border-dim)", flexShrink: 0 }}>
      <div style={{ display: "flex", alignItems: "center" }}>
        <button
          type="button"
          aria-expanded={open}
          aria-label={t("notebook.attachmentsToggle")}
          onClick={() => setOpen((value) => !value)}
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            alignItems: "center",
            gap: 6,
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: "7px 8px",
            fontSize: 11,
            fontWeight: 700,
            textAlign: "left",
          }}
        >
          <Paperclip size={13} />
          <span style={{ flex: 1, minWidth: 0 }}>{t("notebook.attachmentsTitle")}</span>
          {open && items.length > 0 && (
            <span style={{ color: "var(--text-hint)", fontWeight: 400 }}>{items.length}</span>
          )}
          <span
            aria-hidden="true"
            style={{
              display: "inline-block",
              width: 10,
              transform: open ? "rotate(90deg)" : "none",
              transition: "transform 160ms",
            }}
          >
            ▸
          </span>
        </button>
        {open && (
          <button
            type="button"
            aria-label={t("notebook.attachmentsRefresh")}
            title={t("notebook.attachmentsRefresh")}
            onClick={refresh}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: "7px 8px",
            }}
          >
            <RefreshCw size={12} />
          </button>
        )}
      </div>
      {open && (
        <div style={{ maxHeight: 200, overflowY: "auto", paddingBottom: 4 }}>
          {error ? (
            <div
              role="alert"
              style={{
                margin: "0 8px 6px",
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
          ) : loading ? (
            hint(t("notebook.attachmentsLoading"))
          ) : items.length === 0 ? (
            hint(t("notebook.attachmentsEmpty"))
          ) : (
            items.map((item) => {
              const Icon = KIND_ICONS[item.kind] ?? FileIcon;
              return (
                <div
                  key={item.path}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 5,
                    padding: "3px 8px 3px 14px",
                    fontSize: 11,
                  }}
                >
                  <Icon size={12} style={{ flexShrink: 0, color: "var(--text-hint)" }} />
                  <button
                    type="button"
                    aria-label={t("notebook.attachmentReveal", { name: item.name })}
                    title={`${item.relativePath}\n${formatSize(item.size)}`}
                    onClick={() => onReveal(item)}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      border: "none",
                      background: "transparent",
                      color: "var(--text-primary)",
                      textAlign: "left",
                      padding: 0,
                      cursor: "pointer",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      fontSize: 11,
                      fontFamily: "var(--font-ui)",
                    }}
                  >
                    {item.name}
                  </button>
                  {onInsert && (
                    <button
                      type="button"
                      aria-label={t("notebook.attachmentInsert", { name: item.name })}
                      title={t("notebook.attachmentInsert", { name: item.name })}
                      onClick={() => onInsert(item)}
                      style={{
                        border: "none",
                        background: "transparent",
                        color: "var(--text-muted)",
                        cursor: "pointer",
                        padding: "0 2px",
                        flexShrink: 0,
                        fontSize: 12,
                        lineHeight: 1,
                      }}
                    >
                      +
                    </button>
                  )}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
