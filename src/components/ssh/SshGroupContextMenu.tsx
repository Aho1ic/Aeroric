import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Check, FolderPlus, Pencil, Trash2, X } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { zLayers } from "../../styles/zLayers";
import { normalizeSshGroupName } from "./sshGroups";

interface Props {
  group: string;
  x: number;
  y: number;
  /** 已存在的分组名,用于拦重名。不含当前分组自身。 */
  takenNames: string[];
  onClose: () => void;
  onRename: (group: string, nextName: string) => void;
  onDelete: (group: string) => void;
  onCreateConnection?: (group: string) => void;
}

const MENU_WIDTH = 226;
const MENU_HEIGHT = 132;

function menuPosition(x: number, y: number) {
  return {
    left: Math.max(6, Math.min(x, window.innerWidth - MENU_WIDTH - 8)),
    top: Math.max(6, Math.min(y, window.innerHeight - MENU_HEIGHT - 8)),
  };
}

function menuItemStyle(danger = false) {
  return {
    ...s.toolbarMenuItem,
    width: "100%",
    boxSizing: "border-box" as const,
    border: "none",
    background: "transparent",
    textAlign: "left" as const,
    ...(danger ? { color: "var(--danger)" } : null),
  };
}

export function SshGroupContextMenu({
  group,
  x,
  y,
  takenNames,
  onClose,
  onRename,
  onDelete,
  onCreateConnection,
}: Props) {
  const { t } = useI18n();
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(group);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose]);

  const normalized = normalizeSshGroupName(draft);
  // 重名会把两个分组悄悄合并,那不是"重命名"该有的结果,所以直接拦住。
  const canSubmit = Boolean(normalized) && !takenNames.includes(normalized ?? "");

  const submitRename = () => {
    if (!normalized || !canSubmit) return;
    if (normalized !== group) onRename(group, normalized);
    onClose();
  };

  const content = (
    <>
      <div
        aria-hidden="true"
        style={{ position: "fixed", inset: 0, zIndex: zLayers.contextMenuBackdrop }}
        onPointerDown={onClose}
        onContextMenu={(event) => {
          event.preventDefault();
          onClose();
        }}
      />
      <div
        role="menu"
        aria-label={`${group} ${t("ssh.groupMenu")}`}
        style={{
          ...s.toolbarMenuContent,
          ...menuPosition(x, y),
          position: "fixed",
          minWidth: MENU_WIDTH,
          padding: 5,
          zIndex: zLayers.contextMenu,
        }}
        onPointerDown={(event) => event.stopPropagation()}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
        }}
      >
        {renaming ? (
          <div style={{ display: "flex", alignItems: "center", gap: 4, padding: 2 }}>
            <input
              autoFocus
              aria-label={t("ssh.renameGroup")}
              value={draft}
              onChange={(event) => setDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  submitRename();
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  onClose();
                }
              }}
              style={{ ...s.sshInput, flex: 1, minWidth: 0, height: 28 }}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
            <button
              type="button"
              style={{ ...s.sshIconButton, opacity: canSubmit ? 1 : 0.4 }}
              disabled={!canSubmit}
              title={t("common.save")}
              aria-label={t("common.save")}
              onClick={submitRename}
            >
              <Check size={13} />
            </button>
            <button
              type="button"
              style={s.sshIconButton}
              title={t("common.cancel")}
              aria-label={t("common.cancel")}
              onClick={onClose}
            >
              <X size={13} />
            </button>
          </div>
        ) : (
          <>
            <button
              type="button"
              role="menuitem"
              style={menuItemStyle()}
              onClick={() => {
                setDraft(group);
                setRenaming(true);
              }}
            >
              <Pencil style={s.toolbarMenuItemIcon} />
              <span style={{ flex: 1 }}>{t("ssh.renameGroup")}</span>
            </button>
            {onCreateConnection && (
              <button
                type="button"
                role="menuitem"
                style={menuItemStyle()}
                onClick={() => {
                  onCreateConnection(group);
                  onClose();
                }}
              >
                <FolderPlus style={s.toolbarMenuItemIcon} />
                <span style={{ flex: 1 }}>{t("ssh.createInGroup", { group })}</span>
              </button>
            )}
            <button
              type="button"
              role="menuitem"
              style={menuItemStyle(true)}
              onClick={() => {
                onDelete(group);
                onClose();
              }}
            >
              <Trash2 style={s.toolbarMenuItemIcon} />
              <span style={{ flex: 1 }}>{t("ssh.deleteGroup")}</span>
            </button>
            <div style={{ padding: "4px 8px 2px", fontSize: 10.5, color: "var(--text-hint)" }}>
              {t("ssh.deleteGroupHint")}
            </div>
          </>
        )}
      </div>
    </>
  );

  return createPortal(content, document.body);
}
