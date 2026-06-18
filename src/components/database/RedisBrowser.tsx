import { useEffect, useState } from "react";
import { Plus, RefreshCcw, Search, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { useRedisBrowser } from "../../hooks/useRedisBrowser";
import s from "../../styles";

interface Props {
  connectionId: string;
  readOnly: boolean;
}

function redisValueText(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "string") return value;
  return JSON.stringify(value, null, 2);
}

export function RedisBrowser({ connectionId, readOnly }: Props) {
  const { t } = useI18n();
  const redis = useRedisBrowser(connectionId);
  const [activeDb, setActiveDb] = useState(0);
  const [pattern, setPattern] = useState("*");
  const [valueDraft, setValueDraft] = useState("");
  const [ttlDraft, setTtlDraft] = useState("");
  const [commandText, setCommandText] = useState("");
  const [commandResult, setCommandResult] = useState("");
  const [showCreateKey, setShowCreateKey] = useState(false);
  const [createKeyType, setCreateKeyType] = useState<"string" | "hash" | "list" | "set" | "zset" | "stream" | "json">("string");
  const [createKeyName, setCreateKeyName] = useState("");
  const [createKeyValue, setCreateKeyValue] = useState("");
  const [createKeyField, setCreateKeyField] = useState("");
  const [createKeyScore, setCreateKeyScore] = useState("0");
  const [createKeyEntryId, setCreateKeyEntryId] = useState("*");
  const [createKeyTtl, setCreateKeyTtl] = useState("");

  useEffect(() => {
    redis.loadDatabases()
      .then((items) => {
        const firstDb = items[0]?.db ?? 0;
        setActiveDb(firstDb);
        return redis.scanKeys({ db: firstDb, pattern: "*", count: 100 });
      })
      .catch(() => undefined);
    // load once when connection changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId]);

  useEffect(() => {
    setValueDraft(redisValueText(redis.selectedValue?.value ?? ""));
    setTtlDraft(redis.selectedValue?.ttl != null && redis.selectedValue.ttl >= 0 ? String(redis.selectedValue.ttl) : "");
  }, [redis.selectedValue]);

  const refreshKeys = () => redis.scanKeys({ db: activeDb, pattern: pattern.trim() || "*", count: 100 });
  const createKey = async () => {
    if (!createKeyName.trim()) return;
    await redis.createKey({
      db: activeDb,
      keyRaw: createKeyName.trim(),
      keyType: createKeyType,
      value: createKeyValue,
      field: createKeyField.trim() || null,
      score: createKeyScore.trim() ? Number(createKeyScore) : null,
      entryId: createKeyEntryId.trim() || "*",
      ttl: createKeyTtl.trim() ? Number(createKeyTtl) : null,
    });
    setShowCreateKey(false);
    setCreateKeyName("");
    setCreateKeyValue("");
    setCreateKeyField("");
    await refreshKeys();
  };

  const runCommand = async () => {
    if (!commandText.trim()) return;
    const result = await redis.executeCommand({
      db: activeDb,
      command: commandText.trim(),
      skipSafetyCheck: false,
    });
    setCommandResult(JSON.stringify(result.value, null, 2));
  };

  return (
    <div style={s.databaseBrowserRoot}>
      <div style={s.databaseBrowserSidebar}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>Redis</div>
            <div style={s.databaseDialogHint}>{t("database.redisBrowserHint")}</div>
          </div>
          <button type="button" style={s.databaseIconButton} onClick={() => void refreshKeys()} aria-label={t("database.refresh")}>
            <RefreshCcw size={14} />
          </button>
        </div>
        <div style={s.databaseButtonRow}>
          {redis.databases.map((database) => (
            <button
              key={database.db}
              type="button"
              style={{
                ...s.databaseSmallButton,
                ...(activeDb === database.db ? s.databaseListButtonActive : {}),
                padding: "0 8px",
              }}
              onClick={() => {
                setActiveDb(database.db);
                void redis.scanKeys({ db: database.db, pattern: pattern.trim() || "*", count: 100 });
              }}
            >
              db{database.db}
            </button>
          ))}
        </div>
        <label style={s.databaseSearchBox}>
          <Search size={14} />
          <input
            aria-label={t("database.redisKeyPattern")}
            style={s.databaseSearchInput}
            value={pattern}
            onChange={(event) => setPattern(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void refreshKeys();
            }}
          />
        </label>
        <div style={s.databaseScroll}>
          {redis.keys.map((key) => (
            <button
              key={key.key_raw}
              type="button"
              style={s.databaseListButton}
              onClick={() => void redis.loadValue(activeDb, key.key_raw)}
            >
              <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {key.key_display || key.key_raw}
              </span>
              <span style={s.databasePill}>{key.key_type}</span>
            </button>
          ))}
          {redis.keys.length === 0 && <div style={s.databaseEmptyCompact}>{redis.loading ? t("database.loading") : t("database.empty")}</div>}
        </div>
      </div>
      <div style={s.databaseBrowserMain}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>{redis.selectedValue?.key_display ?? t("database.redisValue")}</div>
            <div style={s.databaseDialogHint}>
              {redis.selectedValue ? `${redis.selectedValue.key_type} · TTL ${redis.selectedValue.ttl}` : t("database.redisSelectKey")}
            </div>
          </div>
          <div style={s.databaseButtonRow}>
            <button type="button" style={s.databaseSmallButton} disabled={readOnly} onClick={() => setShowCreateKey((value) => !value)}>
              <Plus size={13} />
              <span>{t("database.redisCreateKey")}</span>
            </button>
            <button
              type="button"
              style={s.databaseSmallButton}
              disabled={readOnly || !redis.selectedValue}
              onClick={() => redis.selectedValue && void redis.deleteKey(activeDb, redis.selectedValue.key_raw)}
            >
              <Trash2 size={13} />
              <span>{t("database.redisDeleteKey")}</span>
            </button>
          </div>
        </div>
        {showCreateKey && (
          <div style={s.databaseDialogFormGrid}>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisKeyName")}</span>
              <input
                aria-label="Redis key name"
                style={s.databaseDialogInput}
                value={createKeyName}
                onChange={(event) => setCreateKeyName(event.target.value)}
              />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisKeyType")}</span>
              <select
                aria-label="Redis key type"
                style={s.databaseDialogInput}
                value={createKeyType}
                onChange={(event) => setCreateKeyType(event.target.value as typeof createKeyType)}
              >
                <option value="string">String</option>
                <option value="hash">Hash</option>
                <option value="list">List</option>
                <option value="set">Set</option>
                <option value="zset">Sorted Set</option>
                <option value="stream">Stream</option>
                <option value="json">JSON</option>
              </select>
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisCreateValue")}</span>
              <input
                aria-label="Redis create value"
                style={s.databaseDialogInput}
                value={createKeyValue}
                onChange={(event) => setCreateKeyValue(event.target.value)}
              />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisCreateField")}</span>
              <input style={s.databaseDialogInput} value={createKeyField} onChange={(event) => setCreateKeyField(event.target.value)} />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisCreateScore")}</span>
              <input style={s.databaseDialogInput} value={createKeyScore} onChange={(event) => setCreateKeyScore(event.target.value)} />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisCreateEntryId")}</span>
              <input style={s.databaseDialogInput} value={createKeyEntryId} onChange={(event) => setCreateKeyEntryId(event.target.value)} />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisTtl")}</span>
              <input style={s.databaseDialogInput} value={createKeyTtl} onChange={(event) => setCreateKeyTtl(event.target.value)} />
            </label>
            <button type="button" style={s.databaseSmallButton} onClick={() => void createKey()} disabled={!createKeyName.trim()}>
              {t("database.saveKey")}
            </button>
          </div>
        )}
        <textarea
          style={s.databaseLargeTextArea}
          value={valueDraft}
          onChange={(event) => setValueDraft(event.target.value)}
          readOnly={readOnly || !redis.selectedValue}
        />
        <div style={s.databaseToolbar}>
          <input
            aria-label={t("database.redisTtl")}
            style={{ ...s.databaseDialogInput, width: 120 }}
            value={ttlDraft}
            onChange={(event) => setTtlDraft(event.target.value)}
            placeholder="TTL"
          />
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={readOnly || !redis.selectedValue}
            onClick={() =>
              redis.selectedValue &&
              void redis.setValue({
                db: activeDb,
                keyRaw: redis.selectedValue.key_raw,
                value: valueDraft,
                ttl: ttlDraft ? Number(ttlDraft) : null,
              })
            }
          >
            {t("database.saveValue")}
          </button>
          <input
            aria-label={t("database.redisCommand")}
            style={{ ...s.databaseDialogInput, flex: 1 }}
            value={commandText}
            onChange={(event) => setCommandText(event.target.value)}
            placeholder="GET user:1"
          />
          <button type="button" style={s.databaseSmallButton} disabled={!commandText.trim()} onClick={() => void runCommand()}>
            {t("database.redisRunCommand")}
          </button>
        </div>
        {commandResult && <pre style={s.databaseSqlPreview}>{commandResult}</pre>}
        {redis.error && <div style={s.databaseError}>{redis.error}</div>}
      </div>
    </div>
  );
}
