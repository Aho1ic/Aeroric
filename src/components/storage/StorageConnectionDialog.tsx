import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import * as Select from "@radix-ui/react-select";
import { AlertTriangle, Check, ChevronDown, Eye, EyeOff, Save, ShieldCheck, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { storageApi } from "../../lib/storageApi";
import type {
  StorageConnection,
  StorageCredentialOptions,
  StorageCredentialSource,
  StorageProtocol,
  StorageProtocolDescriptor,
} from "../../types/storage";
import s from "../../styles";
import {
  groupStorageProtocols,
  normalizeStorageDraft,
  storageDraftFromConnection,
  storageFieldLabelKey,
  storageFieldsForProtocol,
  switchStorageDraftProtocol,
  validateStorageDraft,
  type StorageConnectionDraft,
  type StorageDraftErrors,
} from "./storageProtocolForm";

const DEFAULT_GROUP_VALUE = "__default__";

interface Props {
  connection?: StorageConnection | null;
  descriptors: StorageProtocolDescriptor[];
  groups?: string[];
  initialGroup?: string;
  /** 该连接已落盘的凭据键名,决定必填凭据能否留空。 */
  savedSecretKeys?: string[];
  onClose: () => void;
  onSave: (connection: StorageConnection) => void | Promise<void>;
}

export function StorageConnectionDialog({
  connection,
  descriptors,
  groups = [],
  initialGroup = "",
  savedSecretKeys = [],
  onClose,
  onSave,
}: Props) {
  const { t } = useI18n();
  const isEditing = Boolean(connection);
  const [draft, setDraft] = useState<StorageConnectionDraft>(() =>
    storageDraftFromConnection(connection, descriptors[0]?.protocol ?? "s3", initialGroup),
  );
  const [errors, setErrors] = useState<StorageDraftErrors>({});
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const [credentialOptions, setCredentialOptions] = useState<StorageCredentialOptions | null>(null);
  const [credentialSource, setCredentialSource] = useState<StorageCredentialSource>("userProvided");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [authorizing, setAuthorizing] = useState(false);
  const [authorized, setAuthorized] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

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

  const descriptor = useMemo(
    () => descriptors.find((item) => item.protocol === draft.protocol),
    [descriptors, draft.protocol],
  );
  const fields = useMemo(
    () => storageFieldsForProtocol(draft.protocol, descriptor),
    [descriptor, draft.protocol],
  );
  const effectiveSavedSecretKeys = useMemo(
    () => (connection?.protocol === draft.protocol ? savedSecretKeys : []),
    [connection?.protocol, draft.protocol, savedSecretKeys],
  );
  const hasSavedOAuthCredentials = useMemo(() => {
    if (!credentialOptions) return false;
    const saved = new Set(effectiveSavedSecretKeys);
    return (
      saved.has("refreshToken") &&
      saved.has("clientId") &&
      (!credentialOptions.requiresClientSecret || saved.has("clientSecret"))
    );
  }, [credentialOptions, effectiveSavedSecretKeys]);
  const protocolGroups = useMemo(() => groupStorageProtocols(descriptors), [descriptors]);

  useEffect(() => {
    if (!descriptor?.oauth) {
      setCredentialOptions(null);
      return;
    }
    let cancelled = false;
    void storageApi
      .oauthCredentialOptions(draft.protocol)
      .then((options) => {
        if (cancelled) return;
        setCredentialOptions(options);
        setCredentialSource(options?.builtinAvailable ? "builtin" : "userProvided");
      })
      .catch((cause) => {
        if (!cancelled) console.warn("Failed to read OAuth credential options", cause);
      });
    return () => {
      cancelled = true;
    };
  }, [descriptor?.oauth, draft.protocol]);

  const setConfigValue = useCallback((key: string, value: string) => {
    setDraft((prev) => ({ ...prev, config: { ...prev.config, [key]: value } }));
    setErrors((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  const setSecretValue = useCallback((key: string, value: string) => {
    setDraft((prev) => ({ ...prev, secrets: { ...prev.secrets, [key]: value } }));
    setErrors((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  const changeProtocol = useCallback((protocol: StorageProtocol) => {
    setDraft((prev) => switchStorageDraftProtocol(prev, protocol));
    setErrors({});
    setAuthorized(false);
    setAuthError(null);
  }, []);

  const authorize = useCallback(async () => {
    setAuthorizing(true);
    setAuthError(null);
    try {
      const result = await storageApi.oauthAuthorize({
        protocol: draft.protocol,
        source: credentialSource,
        clientId: clientId.trim() || undefined,
        clientSecret: clientSecret.trim() || undefined,
      });
      setDraft((prev) => ({ ...prev, secrets: { ...prev.secrets, ...result.secrets } }));
      setAuthorized(true);
    } catch (cause) {
      setAuthError(String(cause));
    } finally {
      setAuthorizing(false);
    }
  }, [clientId, clientSecret, credentialSource, draft.protocol]);

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    const nextErrors = validateStorageDraft(draft, effectiveSavedSecretKeys);
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) return;
    setSaveError(null);
    try {
      await onSave(normalizeStorageDraft(draft));
    } catch (cause) {
      setSaveError(String(cause));
    }
  }

  const dialog = (
    <div
      style={s.sshDialogOverlay}
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-label={isEditing ? t("storage.editConnection") : t("storage.newConnection")}
        style={s.sshDialog}
        onSubmit={handleSubmit}
      >
        <div style={s.sshDialogHeader}>
          <div style={s.sshDialogTitle}>
            {isEditing ? t("storage.editConnection") : t("storage.newConnection")}
          </div>
          <button type="button" style={s.modalCloseBtn} onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div style={s.sshDialogBody}>
          <label style={s.sshField}>
            <span style={s.sshLabel}>{t("storage.field.name")}</span>
            <input
              value={draft.name}
              onChange={(event) => {
                setDraft((prev) => ({ ...prev, name: event.target.value }));
                setErrors((prev) => {
                  if (!("name" in prev)) return prev;
                  const next = { ...prev };
                  delete next.name;
                  return next;
                });
              }}
              aria-label={t("storage.field.name")}
              placeholder={t("storage.placeholder.name")}
              style={errors.name ? s.sshInputInvalid : s.sshInput}
              autoFocus
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
            {errors.name && <span style={s.sshErrorText}>{t(errors.name)}</span>}
          </label>

          <label style={s.sshField}>
            <span style={s.sshLabel}>{t("storage.field.protocol")}</span>
            <Select.Root
              value={draft.protocol}
              onValueChange={(value) => changeProtocol(value as StorageProtocol)}
            >
              <Select.Trigger
                aria-label={t("storage.field.protocol")}
                className="radix-select-trigger"
                style={selectTriggerStyle}
              >
                <Select.Value>
                  {descriptor
                    ? t(`storage.protocol.${descriptor.protocol}`)
                    : t("storage.field.protocol")}
                </Select.Value>
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
                    {protocolGroups.map((entry) => (
                      <Select.Group key={entry.group}>
                        <Select.Label className="radix-select-label">
                          {t(`storage.group.${entry.group}`)}
                        </Select.Label>
                        {entry.descriptors.map((item) => (
                          <Select.Item
                            key={item.protocol}
                            value={item.protocol}
                            className="radix-select-item"
                            style={
                              item.protocol === draft.protocol
                                ? s.settingsSelectOptionSelected
                                : undefined
                            }
                          >
                            <Select.ItemText>
                              {t(`storage.protocol.${item.protocol}`)}
                            </Select.ItemText>
                            <Select.ItemIndicator className="radix-select-item-indicator">
                              <Check size={13} />
                            </Select.ItemIndicator>
                          </Select.Item>
                        ))}
                      </Select.Group>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </label>

          <label style={s.sshField}>
            <span style={s.sshLabel}>{t("storage.field.group")}</span>
            {groupOptions.length > 0 ? (
              <Select.Root
                value={draft.group || DEFAULT_GROUP_VALUE}
                onValueChange={(value) =>
                  setDraft((prev) => ({
                    ...prev,
                    group: value === DEFAULT_GROUP_VALUE ? "" : value,
                  }))
                }
              >
                <Select.Trigger
                  aria-label={t("storage.field.group")}
                  className="radix-select-trigger"
                  style={selectTriggerStyle}
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
                      {[DEFAULT_GROUP_VALUE, ...groupOptions].map((value) => {
                        const selected =
                          (value === DEFAULT_GROUP_VALUE ? "" : value) === draft.group;
                        return (
                          <Select.Item
                            key={value}
                            value={value}
                            className="radix-select-item"
                            style={selected ? s.settingsSelectOptionSelected : undefined}
                          >
                            <Select.ItemText>
                              {value === DEFAULT_GROUP_VALUE ? t("ssh.defaultGroup") : value}
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
            ) : (
              <input
                value={draft.group}
                onChange={(event) => setDraft((prev) => ({ ...prev, group: event.target.value }))}
                placeholder={t("ssh.defaultGroup")}
                style={s.sshInput}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
              />
            )}
          </label>

          {descriptor?.deprecated && (
            <div style={deprecationNoticeStyle}>
              <AlertTriangle size={13} />
              <span>{t(`storage.deprecated.${draft.protocol}`)}</span>
            </div>
          )}

          {descriptor?.systemMount && (
            <div style={s.sshSecretNote}>{t("storage.hint.systemMount")}</div>
          )}

          {descriptor?.oauth && (
            <OAuthSection
              protocol={draft.protocol}
              options={credentialOptions}
              source={credentialSource}
              onSourceChange={setCredentialSource}
              clientId={clientId}
              onClientIdChange={setClientId}
              clientSecret={clientSecret}
              onClientSecretChange={setClientSecret}
              authorizing={authorizing}
              authorized={authorized || hasSavedOAuthCredentials}
              error={authError}
              onAuthorize={authorize}
            />
          )}

          {fields.map((spec) => {
            const value =
              spec.kind === "config"
                ? (draft.config[spec.key] ?? "")
                : (draft.secrets[spec.key] ?? "");
            const saved = spec.kind === "secret" && effectiveSavedSecretKeys.includes(spec.key);
            const isRevealed = revealed.has(spec.key);
            return (
              <label key={`${spec.kind}:${spec.key}`} style={s.sshField}>
                <span style={s.sshLabel}>
                  {t(storageFieldLabelKey(spec))}
                  {spec.required ? " *" : ""}
                </span>
                {spec.masked ? (
                  <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
                    <input
                      value={value}
                      onChange={(event) => setSecretValue(spec.key, event.target.value)}
                      aria-label={t(storageFieldLabelKey(spec))}
                      placeholder={saved ? t("storage.placeholder.savedSecret") : spec.placeholder}
                      style={{
                        ...(errors[spec.key] ? s.sshInputInvalid : s.sshInput),
                        paddingRight: 36,
                      }}
                      type={isRevealed ? "text" : "password"}
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      onClick={() =>
                        setRevealed((prev) => {
                          const next = new Set(prev);
                          if (next.has(spec.key)) next.delete(spec.key);
                          else next.add(spec.key);
                          return next;
                        })
                      }
                      style={revealButtonStyle}
                      aria-label={isRevealed ? t("ssh.hidePassword") : t("ssh.showPassword")}
                    >
                      {isRevealed ? <EyeOff size={14} /> : <Eye size={14} />}
                    </button>
                  </div>
                ) : (
                  <input
                    value={value}
                    onChange={(event) =>
                      spec.kind === "config"
                        ? setConfigValue(spec.key, event.target.value)
                        : setSecretValue(spec.key, event.target.value)
                    }
                    aria-label={t(storageFieldLabelKey(spec))}
                    placeholder={saved ? t("storage.placeholder.savedSecret") : spec.placeholder}
                    style={errors[spec.key] ? s.sshInputInvalid : s.sshInput}
                    type="text"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                  />
                )}
                {spec.hintKey && <span style={s.sshSecretNote}>{t(spec.hintKey)}</span>}
                {errors[spec.key] && <span style={s.sshErrorText}>{t(errors[spec.key])}</span>}
              </label>
            );
          })}

          <div style={s.sshSecretNote}>{t("storage.secretStorageHint")}</div>
          {saveError && <span style={s.sshErrorText}>{saveError}</span>}
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

function OAuthSection({
  protocol,
  options,
  source,
  onSourceChange,
  clientId,
  onClientIdChange,
  clientSecret,
  onClientSecretChange,
  authorizing,
  authorized,
  error,
  onAuthorize,
}: {
  protocol: StorageProtocol;
  options: StorageCredentialOptions | null;
  source: StorageCredentialSource;
  onSourceChange: (source: StorageCredentialSource) => void;
  clientId: string;
  onClientIdChange: (value: string) => void;
  clientSecret: string;
  onClientSecretChange: (value: string) => void;
  authorizing: boolean;
  authorized: boolean;
  error: string | null;
  onAuthorize: () => void;
}) {
  const { t } = useI18n();
  const builtinAvailable = options?.builtinAvailable ?? false;
  const requiresClientSecret = options?.requiresClientSecret ?? false;
  const effectiveSource = builtinAvailable ? source : "userProvided";
  const canAuthorize =
    effectiveSource === "builtin" ||
    (clientId.trim().length > 0 && (!requiresClientSecret || clientSecret.trim().length > 0));

  return (
    <section style={oauthSectionStyle} aria-label={t("storage.oauth.title")}>
      <div style={s.sshLabel}>{t("storage.oauth.title")}</div>
      {builtinAvailable ? (
        <div style={{ display: "flex", gap: 12 }}>
          <label style={radioLabelStyle}>
            <input
              type="radio"
              name="storage-credential-source"
              aria-label={t("storage.oauth.builtin")}
              checked={effectiveSource === "builtin"}
              onChange={() => onSourceChange("builtin")}
            />
            <span>{t("storage.oauth.builtin")}</span>
          </label>
          <label style={radioLabelStyle}>
            <input
              type="radio"
              name="storage-credential-source"
              aria-label={t("storage.oauth.userProvided")}
              checked={effectiveSource === "userProvided"}
              onChange={() => onSourceChange("userProvided")}
            />
            <span>{t("storage.oauth.userProvided")}</span>
          </label>
        </div>
      ) : (
        <div style={s.sshSecretNote}>
          {requiresClientSecret
            ? t("storage.oauth.secretRequiredHint")
            : t("storage.oauth.noBuiltinHint")}
        </div>
      )}

      {effectiveSource === "userProvided" && (
        <>
          <label style={s.sshField}>
            <span style={s.sshLabel}>{t("storage.field.clientId")} *</span>
            <input
              value={clientId}
              onChange={(event) => onClientIdChange(event.target.value)}
              aria-label={t("storage.field.clientId")}
              style={s.sshInput}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
          </label>
          {requiresClientSecret && (
            <label style={s.sshField}>
              <span style={s.sshLabel}>{t("storage.field.clientSecret")} *</span>
              <input
                value={clientSecret}
                onChange={(event) => onClientSecretChange(event.target.value)}
                aria-label={t("storage.field.clientSecret")}
                style={s.sshInput}
                type="password"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
              />
            </label>
          )}
        </>
      )}

      {options?.scope && (
        <div style={s.sshSecretNote}>{t("storage.oauth.scope", { scope: options.scope })}</div>
      )}
      <div style={s.sshSecretNote}>{t("storage.oauth.browserHint")}</div>

      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <button
          type="button"
          style={canAuthorize && !authorizing ? s.sshSecondaryButton : s.sshPrimaryButtonDisabled}
          disabled={!canAuthorize || authorizing}
          onClick={onAuthorize}
        >
          <ShieldCheck size={14} />
          {authorizing
            ? t("storage.oauth.authorizing")
            : t("storage.oauth.authorize", { service: t(`storage.protocol.${protocol}`) })}
        </button>
        {authorized && (
          <span style={{ ...s.sshSecretNote, color: "var(--success)" }}>
            {t("storage.oauth.authorized")}
          </span>
        )}
      </div>
      {error && <span style={s.sshErrorText}>{error}</span>}
    </section>
  );
}

const selectTriggerStyle = {
  ...s.sshInput,
  minHeight: 34,
  height: 34,
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 8,
  padding: "0 9px",
} as const;

const revealButtonStyle = {
  position: "absolute",
  right: 8,
  display: "flex",
  alignItems: "center",
  padding: 2,
  border: "none",
  background: "transparent",
  color: "var(--text-hint)",
  cursor: "pointer",
} as const;

const oauthSectionStyle = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
  padding: 10,
  border: "1px solid var(--border-dim)",
  borderRadius: "var(--radius-md)",
  background: "color-mix(in srgb, var(--bg-card) 70%, var(--bg-hover))",
} as const;

const radioLabelStyle = {
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  fontSize: 12,
  color: "var(--text-primary)",
  cursor: "pointer",
} as const;

const deprecationNoticeStyle = {
  display: "flex",
  alignItems: "flex-start",
  gap: 6,
  padding: "8px 10px",
  border: "1px solid var(--warning)",
  borderRadius: "var(--radius-sm)",
  color: "var(--warning)",
  fontSize: 11.5,
  lineHeight: 1.45,
} as const;
