import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { FolderTree, Pencil, Plus, Trash2, X } from "lucide-react";
import type { Project } from "../types";
import { projectGroupForProject, UNGROUPED_PROJECT_GROUP } from "../projectGroups";
import { useI18n } from "../i18n";
import s from "../styles";

type Props = {
  projects: Project[];
  groupNames: string[];
  onAssignProjectGroup: (projectId: string, groupName: string | null) => void;
  onCreateGroup: (groupName: string) => void;
  onRenameGroup: (oldName: string, nextName: string) => void;
  onDeleteGroup: (groupName: string) => void;
  onClose: () => void;
};

export function ProjectGroupDialog({
  projects,
  groupNames,
  onAssignProjectGroup,
  onCreateGroup,
  onRenameGroup,
  onDeleteGroup,
  onClose,
}: Props) {
  const { t } = useI18n();
  const [newGroupName, setNewGroupName] = useState("");
  const [editingGroup, setEditingGroup] = useState<string | null>(null);
  const [editingGroupName, setEditingGroupName] = useState("");

  const groups = useMemo(
    () => [
      ...groupNames.map((name) => ({ name, isUngrouped: false })),
      { name: UNGROUPED_PROJECT_GROUP, isUngrouped: true },
    ],
    [groupNames],
  );

  const submitNewGroup = () => {
    const normalized = newGroupName.trim();
    if (!normalized || groupNames.includes(normalized)) return;
    onCreateGroup(normalized);
    setNewGroupName("");
  };

  const startRename = (name: string) => {
    setEditingGroup(name);
    setEditingGroupName(name);
  };

  const submitRename = () => {
    if (!editingGroup) return;
    const normalized = editingGroupName.trim();
    if (normalized && normalized !== editingGroup && !groupNames.includes(normalized)) {
      onRenameGroup(editingGroup, normalized);
    }
    setEditingGroup(null);
    setEditingGroupName("");
  };

  const handleOverlayClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };

  const dialog = (
    <div style={s.modalOverlay} onClick={handleOverlayClick}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("projectGroups.title")}
        style={{
          width: "min(720px, calc(100vw - 40px))",
          maxHeight: "min(720px, calc(100vh - 40px))",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          background: "var(--bg-card)",
          border: "1px solid var(--border-medium)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-popover)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 12,
            padding: "18px 20px 14px",
            borderBottom: "1px solid var(--border-dim)",
          }}
        >
          <FolderTree size={18} color="var(--accent)" strokeWidth={2} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 15, fontWeight: 720, color: "var(--text-primary)" }}>
              {t("projectGroups.title")}
            </div>
            <div style={{ marginTop: 4, fontSize: 12, color: "var(--text-muted)" }}>
              {t("projectGroups.subtitle")}
            </div>
          </div>
          <button
            type="button"
            style={s.modalCloseBtn}
            onClick={onClose}
            title={t("common.close")}
            aria-label={t("common.close")}
          >
            <X size={16} strokeWidth={2} />
          </button>
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "12px 20px",
            borderBottom: "1px solid var(--border-dim)",
            background: "color-mix(in srgb, var(--bg-sidebar) 72%, transparent)",
          }}
        >
          <input
            aria-label={t("projectGroups.newGroup")}
            value={newGroupName}
            onChange={(event) => setNewGroupName(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submitNewGroup();
              }
            }}
            placeholder={t("projectGroups.groupName")}
            style={{
              ...s.modalInput,
              flex: 1,
              minWidth: 0,
              height: 34,
              boxSizing: "border-box",
            }}
          />
          <button
            type="button"
            style={{
              ...s.modalSaveBtn,
              height: 34,
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              opacity: newGroupName.trim() && !groupNames.includes(newGroupName.trim()) ? 1 : 0.5,
            }}
            disabled={!newGroupName.trim() || groupNames.includes(newGroupName.trim())}
            onClick={submitNewGroup}
          >
            <Plus size={14} strokeWidth={2.2} />
            <span>{t("projectGroups.create")}</span>
          </button>
        </div>

        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "8px 20px 18px" }}>
          {groups.map((group) => {
            const groupedProjects = projects.filter(
              (project) => projectGroupForProject(project) === group.name,
            );
            const isEditing = !group.isUngrouped && editingGroup === group.name;
            return (
              <div key={group.name} style={{ marginTop: 8 }}>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    height: 30,
                    padding: "0 8px",
                    borderBottom: "1px solid color-mix(in srgb, #16a34a 45%, var(--border-dim))",
                    color: "var(--text-muted)",
                    fontSize: 10.5,
                    fontWeight: 700,
                  }}
                >
                  {isEditing ? (
                    <input
                      autoFocus
                      aria-label={t("projectGroups.rename")}
                      value={editingGroupName}
                      onChange={(event) => setEditingGroupName(event.currentTarget.value)}
                      onBlur={submitRename}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          submitRename();
                        }
                        if (event.key === "Escape") {
                          setEditingGroup(null);
                          setEditingGroupName("");
                        }
                      }}
                      style={{
                        ...s.modalInput,
                        height: 26,
                        minWidth: 0,
                        flex: 1,
                        boxSizing: "border-box",
                      }}
                    />
                  ) : (
                    <span style={{ flex: 1, minWidth: 0 }}>
                      {group.isUngrouped ? t("projectGroups.ungrouped") : group.name}
                    </span>
                  )}
                  <span>{groupedProjects.length}</span>
                  {!group.isUngrouped && !isEditing && (
                    <>
                      <button
                        type="button"
                        aria-label={t("projectGroups.rename")}
                        title={t("projectGroups.rename")}
                        onClick={() => startRename(group.name)}
                        style={s.modalIconBtn}
                      >
                        <Pencil size={13} strokeWidth={1.9} />
                      </button>
                      <button
                        type="button"
                        aria-label={t("projectGroups.delete")}
                        title={t("projectGroups.delete")}
                        onClick={() => onDeleteGroup(group.name)}
                        style={{ ...s.modalIconBtn, color: "var(--danger)" }}
                      >
                        <Trash2 size={13} strokeWidth={1.9} />
                      </button>
                    </>
                  )}
                </div>

                {groupedProjects.length === 0 ? (
                  <div style={{ padding: "12px 8px", color: "var(--text-hint)", fontSize: 12 }}>
                    {t("projectGroups.empty")}
                  </div>
                ) : (
                  groupedProjects.map((project) => (
                    <div
                      key={project.id}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 12,
                        minHeight: 42,
                        padding: "5px 8px",
                        borderBottom: "1px solid var(--border-dim)",
                      }}
                    >
                      <span
                        style={{
                          flex: 1,
                          minWidth: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          color: "var(--text-primary)",
                          fontSize: 12.5,
                        }}
                        title={project.name}
                      >
                        {project.name}
                      </span>
                      <select
                        aria-label={t("projectGroups.assign", { name: project.name })}
                        value={project.group?.trim() ?? ""}
                        onChange={(event) =>
                          onAssignProjectGroup(project.id, event.currentTarget.value || null)
                        }
                        style={{
                          ...s.modalInput,
                          width: 170,
                          height: 30,
                          padding: "0 8px",
                          color: "var(--text-secondary)",
                          background: "var(--bg-input)",
                        }}
                      >
                        <option value="">{t("projectGroups.ungrouped")}</option>
                        {groupNames.map((name) => (
                          <option key={name} value={name}>
                            {name}
                          </option>
                        ))}
                      </select>
                    </div>
                  ))
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );

  return createPortal(dialog, document.body);
}
