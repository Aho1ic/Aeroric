import { useMemo, useState } from "react";
import { Save, X } from "lucide-react";
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
  onClose: () => void;
  onSave: (connection: SshConnection) => void;
}

const FIELD_ORDER: Array<keyof SshConnectionDraft> = [
  "name",
  "host",
  "port",
  "username",
  "identityFile",
  "password",
  "remotePath",
];

export function SshConnectionDialog({ connection, onClose, onSave }: Props) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<SshConnectionDraft>(() => draftFromConnection(connection));
  const [errors, setErrors] = useState<SshConnectionDraftErrors>({});
  const isEditing = Boolean(connection);

  const labels = useMemo<Record<keyof SshConnectionDraft, string>>(
    () => ({
      name: t("ssh.field.name"),
      host: t("ssh.field.host"),
      port: t("ssh.field.port"),
      username: t("ssh.field.username"),
      identityFile: t("ssh.field.identityFile"),
      password: t("ssh.field.password"),
      remotePath: t("ssh.field.remotePath"),
    }),
    [t],
  );

  const placeholders = useMemo<Record<keyof SshConnectionDraft, string>>(
    () => ({
      name: "prod",
      host: "prod.example.com",
      port: "22",
      username: "deploy",
      identityFile: "~/.ssh/id_ed25519",
      password: "",
      remotePath: "/srv/app",
    }),
    [],
  );

  function updateField(field: keyof SshConnectionDraft, value: string) {
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

  return (
    <div style={s.sshDialogOverlay} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form style={s.sshDialog} onSubmit={handleSubmit}>
        <div style={s.sshDialogHeader}>
          <div style={s.sshDialogTitle}>
            {isEditing ? t("ssh.editConnection") : t("ssh.newConnection")}
          </div>
          <button type="button" style={s.modalCloseBtn} onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div style={s.sshDialogBody}>
          {FIELD_ORDER.map((field) => (
            <label key={field} style={s.sshField}>
              <span style={s.sshLabel}>{labels[field]}</span>
              <input
                value={draft[field]}
                onChange={(event) => updateField(field, event.target.value)}
                placeholder={placeholders[field]}
                style={errors[field] ? s.sshInputInvalid : s.sshInput}
                type={field === "password" ? "password" : "text"}
                autoFocus={field === "name"}
              />
              {errors[field] && <span style={s.sshErrorText}>{errors[field]}</span>}
            </label>
          ))}
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
}
