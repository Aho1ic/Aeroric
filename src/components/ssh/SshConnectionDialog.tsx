import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import * as Select from "@radix-ui/react-select";
import { Check, ChevronDown, Eye, EyeOff, Save, X } from "lucide-react";
import type { SshConnection } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import {
  draftFromConnection,
  normalizeSshConnectionDraft,
  validateSshConnectionDraft,
  type SshConnectionDraft,
  type SshConnectionDraftErrors,
} from "./validation";

interface Props {
  connection?: SshConnection | null;
  groups?: string[];
  initialGroup?: string;
  onClose: () => void;
  onSave: (connection: SshConnection) => void;
}

const FIELD_ORDER: Array<keyof SshConnectionDraft> = [
  "name",
  "group",
  "host",
  "port",
  "username",
  "identityFile",
  "password",
  "remotePath",
];

type SshTextField = Exclude<keyof SshConnectionDraft, "autoSudoWithPassword">;

const TEXT_FIELD_ORDER: SshTextField[] = FIELD_ORDER as SshTextField[];

export function SshConnectionDialog({
  connection,
  groups = [],
  initialGroup = "",
  onClose,
  onSave,
}: Props) {
  const { t } = useI18n();
  const groupOptions = useMemo(
    () =>
      Array.from(
        new Set(
          [...groups, connection?.group ?? "", initialGroup]
            .map((group) => group.trim())
            .filter(Boolean),
        ),
      ),
    [connection?.group, groups, initialGroup],
  );
  const [draft, setDraft] = useState<SshConnectionDraft>(() => ({
    ...draftFromConnection(connection),
    group: connection?.group ?? initialGroup,
  }));
  const [errors, setErrors] = useState<SshConnectionDraftErrors>({});
  const [showPassword, setShowPassword] = useState(false);
  const isEditing = Boolean(connection);

  const labels = useMemo<Record<SshTextField, string>>(
    () => ({
      name: t("ssh.field.name"),
      group: t("ssh.field.group"),
      host: t("ssh.field.host"),
      port: t("ssh.field.port"),
      username: t("ssh.field.username"),
      identityFile: t("ssh.field.identityFile"),
      password: t("ssh.field.password"),
      remotePath: t("ssh.field.remotePath"),
    }),
    [t],
  );

  const placeholders = useMemo<Record<SshTextField, string>>(
    () => ({
      name: "prod",
      group: t("ssh.defaultGroup"),
      host: "prod.example.com",
      port: "22",
      username: "deploy",
      identityFile: "~/.ssh/id_ed25519",
      password: "",
      remotePath: "/srv/app",
    }),
    [t],
  );

  function updateField(field: SshTextField, value: string) {
    setDraft((prev) => ({ ...prev, [field]: value }));
    setErrors((prev) => {
      if (!(field in prev)) return prev;
      const next = { ...prev };
      delete next[field];
      return next;
    });
  }

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    const nextErrors = validateSshConnectionDraft(draft);
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) return;
    const normalized = normalizeSshConnectionDraft(draft, Date.now(), Date.now(), connection);
    if (!normalized) return;
    onSave(normalized);
  }

  const dialog = (
    <div style={s.sshDialogOverlay} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        role="dialog"
        aria-modal="true"
        aria-label={isEditing ? t("ssh.editConnection") : t("ssh.newConnection")}
        style={s.sshDialog}
        onSubmit={handleSubmit}
      >
        <div style={s.sshDialogHeader}>
          <div style={s.sshDialogTitle}>
            {isEditing ? t("ssh.editConnection") : t("ssh.newConnection")}
          </div>
          <button type="button" style={s.modalCloseBtn} onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div style={s.sshDialogBody}>
          {TEXT_FIELD_ORDER.map((field) => (
            <label key={field} style={s.sshField}>
              <span style={s.sshLabel}>{labels[field]}</span>
              {field === "group" && groupOptions.length > 0 ? (
                <Select.Root
                  value={draft.group || "__default__"}
                  onValueChange={(value) =>
                    updateField("group", value === "__default__" ? "" : value)
                  }
                >
                  <Select.Trigger
                    aria-label={labels[field]}
                    className="radix-select-trigger"
                    style={{
                      ...(errors[field] ? s.sshInputInvalid : s.sshInput),
                      minHeight: 34,
                      height: 34,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 8,
                      padding: "0 9px",
                    }}
                  >
                    <Select.Value>{draft.group || t("ssh.defaultGroup")}</Select.Value>
                    <Select.Icon>
                      <ChevronDown size={13} strokeWidth={2.2} color="var(--text-hint)" />
                    </Select.Icon>
                  </Select.Trigger>
                  <Select.Portal>
                    <Select.Content
                      position="popper"
                      sideOffset={4}
                      className="radix-select-content"
                      style={s.settingsSelectContent}
                    >
                      <Select.Viewport className="radix-select-viewport">
                        {["__default__", ...groupOptions].map((value) => {
                          const selected = (value === "__default__" ? "" : value) === draft.group;
                          return (
                            <Select.Item
                              key={value}
                              value={value}
                              className="radix-select-item"
                              style={selected ? s.settingsSelectOptionSelected : undefined}
                            >
                              <Select.ItemText>
                                {value === "__default__" ? t("ssh.defaultGroup") : value}
                              </Select.ItemText>
                              <Select.ItemIndicator className="radix-select-item-indicator">
                                <Check size={13} />
                              </Select.ItemIndicator>
                            </Select.Item>
                          );
                        })}
                      </Select.Viewport>
                    </Select.Content>
                  </Select.Portal>
                </Select.Root>
              ) : field === "password" ? (
                <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
                  <input
                    value={draft[field]}
                    onChange={(event) => updateField(field, event.target.value)}
                    placeholder={placeholders[field]}
                    style={{
                      ...(errors[field] ? s.sshInputInvalid : s.sshInput),
                      paddingRight: 36,
                    }}
                    type={showPassword ? "text" : "password"}
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword((value) => !value)}
                    style={{
                      position: "absolute",
                      right: 8,
                      display: "flex",
                      alignItems: "center",
                      padding: 2,
                      border: "none",
                      background: "transparent",
                      color: "var(--text-hint)",
                      cursor: "pointer",
                    }}
                    aria-label={showPassword ? t("ssh.hidePassword") : t("ssh.showPassword")}
                  >
                    {showPassword ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
              ) : (
                <input
                  value={draft[field]}
                  onChange={(event) => updateField(field, event.target.value)}
                  placeholder={placeholders[field]}
                  style={errors[field] ? s.sshInputInvalid : s.sshInput}
                  type="text"
                  autoFocus={field === "name"}
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                />
              )}
              {errors[field] && <span style={s.sshErrorText}>{errors[field]}</span>}
            </label>
          ))}
          <label style={{ ...s.sshField, flexDirection: "row", alignItems: "flex-start", gap: 10 }}>
            <input
              type="checkbox"
              checked={draft.autoSudoWithPassword}
              onChange={(event) =>
                setDraft((prev) => ({
                  ...prev,
                  autoSudoWithPassword: event.target.checked,
                }))
              }
              style={{ marginTop: 2 }}
            />
            <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={s.sshLabel}>{t("ssh.field.autoSudoWithPassword")}</span>
              <span style={s.sshSecretNote}>{t("ssh.field.autoSudoWithPasswordHint")}</span>
            </span>
          </label>
          <div style={s.sshSecretNote}>{t("ssh.passwordStorageHint")}</div>
        </div>

        <div style={s.sshDialogFooter}>
          <button type="button" style={s.sshSecondaryButton} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" style={s.sshPrimaryButton}>
            <Save size={14} />
            {t("common.save")}
          </button>
        </div>
      </form>
    </div>
  );

  return createPortal(dialog, document.body);
}
