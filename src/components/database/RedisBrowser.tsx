import { useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent, PointerEvent as ReactPointerEvent } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  Braces,
  Copy,
  Eraser,
  Eye,
  Minimize2,
  Pencil,
  Plus,
  RefreshCcw,
  Save,
  Terminal,
  Trash2,
  WrapText,
  X,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { useRedisBrowser } from "../../hooks/useRedisBrowser";
import {
  clearRedisCommandHistory,
  isRedisClearScreenCommand,
  loadRedisCommandHistory,
  nextRedisCommandDb,
  saveRedisCommandHistory,
} from "../../lib/redisCommandSession";
import { classifyRedisCommandSafety } from "../../lib/redisCommandSafety";
import { redisKeySearchPattern } from "../../lib/redisKeyPattern";
import {
  buildRedisKeyTree,
  collectRedisGroupIds,
  flattenVisibleRedisKeyTree,
} from "../../lib/redisKeyTree";
import s from "../../styles";
import { DbxButton, DbxMenuItem } from "./DbxButton";
import { RedisCommandSessionView, type RedisCommandHistoryEntry } from "./RedisCommandSessionView";
import { RedisJsonTree } from "./RedisJsonTree";
import { RedisKeyTreePane } from "./RedisKeyTreePane";
import {
  clampRedisHashFieldWidth,
  clampRedisMemberDetailWidth,
  clampRedisZsetScoreWidth,
  formatRedisCommandResult,
  loadRedisJsonWordWrap,
  redisInsertStatement,
  redisJsonText,
  redisJsonValue,
  redisKeySizeLabel,
  redisMemberColumnKeys,
  redisMemberRows,
  redisStreamEntryCount,
  redisStreamMemberGroups,
  redisValueMemberKind,
  redisValueText,
  saveRedisJsonWordWrap,
  type RedisMemberRow,
} from "./redisBrowserState";

interface Props {
  connectionId: string;
  readOnly: boolean;
  initialDb?: number;
  initialKey?: string;
  keySeparator?: string;
}

type RedisCommandHistoryDraft = Omit<RedisCommandHistoryEntry, "id">;
export function RedisBrowser({
  connectionId,
  readOnly,
  initialDb,
  initialKey,
  keySeparator = ":",
}: Props) {
  const { t } = useI18n();
  const redis = useRedisBrowser(connectionId);
  const [activeDb, setActiveDb] = useState(0);
  const [commandDb, setCommandDb] = useState(0);
  const [pattern, setPattern] = useState("");
  const [fuzzyKeySearch, setFuzzyKeySearch] = useState(false);
  const [fetchingAllKeys, setFetchingAllKeys] = useState(false);
  const [fetchAllLoadedKeys, setFetchAllLoadedKeys] = useState(0);
  const [fetchAllTotalKeys, setFetchAllTotalKeys] = useState(0);
  const stopFetchAllKeysRef = useRef(false);
  const [expandedKeyGroups, setExpandedKeyGroups] = useState<Set<string>>(new Set());
  const [selectedKeyRaws, setSelectedKeyRaws] = useState<Set<string>>(new Set());
  const [selectedMemberId, setSelectedMemberId] = useState<string | null>(null);
  const [loadingMoreMembers, setLoadingMoreMembers] = useState(false);
  const [editingMemberId, setEditingMemberId] = useState<string | null>(null);
  const [memberDetailView, setMemberDetailView] = useState<"json" | "raw">("json");
  const [memberJsonWordWrap, setMemberJsonWordWrap] = useState(loadRedisJsonWordWrap);
  const [memberDetailPanelWidth, setMemberDetailPanelWidth] = useState(420);
  const [resizingMemberDetail, setResizingMemberDetail] = useState(false);
  const memberDetailResizeStartRef = useRef({ x: 0, width: 420 });
  const [memberHashFieldWidth, setMemberHashFieldWidth] = useState(180);
  const [memberZsetScoreWidth, setMemberZsetScoreWidth] = useState(120);
  const [resizingMemberColumn, setResizingMemberColumn] = useState<"hash" | "zset" | null>(null);
  const memberColumnResizeStartRef = useRef({ x: 0, width: 0 });
  const [memberEditValue, setMemberEditValue] = useState("");
  const [savingMember, setSavingMember] = useState(false);
  const [newMemberField, setNewMemberField] = useState("");
  const [newMemberValue, setNewMemberValue] = useState("");
  const [newMemberScore, setNewMemberScore] = useState("0");
  const [addingMember, setAddingMember] = useState(false);
  const [memberActionError, setMemberActionError] = useState("");
  const [valueDraft, setValueDraft] = useState("");
  const [valueDetailView, setValueDetailView] = useState<"json" | "raw">("raw");
  const [valueFormatError, setValueFormatError] = useState("");
  const [ttlDraft, setTtlDraft] = useState("");
  const [editingTtl, setEditingTtl] = useState(false);
  const [ttlError, setTtlError] = useState("");
  const [commandText, setCommandText] = useState("");
  const [commandHistory, setCommandHistory] = useState<RedisCommandHistoryEntry[]>([]);
  const [commandRunning, setCommandRunning] = useState(false);
  const [showCreateKey, setShowCreateKey] = useState(false);
  const [createKeyType, setCreateKeyType] = useState<
    "string" | "hash" | "list" | "set" | "zset" | "stream" | "json"
  >("string");
  const [createKeyName, setCreateKeyName] = useState("");
  const [createKeyValue, setCreateKeyValue] = useState("");
  const [createKeyField, setCreateKeyField] = useState("");
  const [createKeyScore, setCreateKeyScore] = useState("0");
  const [createKeyEntryId, setCreateKeyEntryId] = useState("*");
  const [createKeyTtl, setCreateKeyTtl] = useState("");
  const [keyContextMenu, setKeyContextMenu] = useState<{
    x: number;
    y: number;
    keyRaw: string;
  } | null>(null);
  const [groupContextMenu, setGroupContextMenu] = useState<{
    x: number;
    y: number;
    groupName: string;
    keyRaws: string[];
  } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setActiveDb(0);
    setCommandDb(0);
    setPattern("");
    setFuzzyKeySearch(false);
    setFetchingAllKeys(false);
    setFetchAllLoadedKeys(0);
    setFetchAllTotalKeys(0);
    stopFetchAllKeysRef.current = false;
    setExpandedKeyGroups(new Set());
    setSelectedKeyRaws(new Set());
    setSelectedMemberId(null);
    setLoadingMoreMembers(false);
    setEditingMemberId(null);
    setMemberDetailView("json");
    setResizingMemberDetail(false);
    setResizingMemberColumn(null);
    setMemberEditValue("");
    setSavingMember(false);
    setNewMemberField("");
    setNewMemberValue("");
    setNewMemberScore("0");
    setAddingMember(false);
    setMemberActionError("");
    setValueDraft("");
    setValueDetailView("raw");
    setValueFormatError("");
    setTtlDraft("");
    setEditingTtl(false);
    setTtlError("");
    setCommandText("");
    setShowCreateKey(false);
    setCreateKeyType("string");
    setCreateKeyName("");
    setCreateKeyValue("");
    setCreateKeyField("");
    setCreateKeyScore("0");
    setCreateKeyEntryId("*");
    setCreateKeyTtl("");
    setKeyContextMenu(null);
    setGroupContextMenu(null);
    redis.resetBrowserState();
    redis
      .loadDatabases()
      .then((items) => {
        if (cancelled) return undefined;
        const targetDb =
          typeof initialDb === "number" && Number.isFinite(initialDb)
            ? initialDb
            : (items[0]?.db ?? 0);
        setActiveDb(targetDb);
        setCommandDb(targetDb);
        return redis.scanKeys({ db: targetDb, pattern: "*", count: 100 }).then(() => {
          if (cancelled) return undefined;
          if (initialKey) return redis.loadValue(targetDb, initialKey);
          return undefined;
        });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
    // sync initial db/key from the sidebar without re-running for hook identity changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId, initialDb, initialKey]);

  useEffect(() => {
    const persisted = loadRedisCommandHistory(connectionId);
    setCommandHistory(persisted.map((entry, index) => ({ ...entry, id: index + 1 })));
  }, [connectionId]);

  useEffect(() => {
    if (!resizingMemberDetail) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => {
      const { x, width } = memberDetailResizeStartRef.current;
      setMemberDetailPanelWidth(clampRedisMemberDetailWidth(width + x - event.clientX));
    };
    const handlePointerUp = () => {
      setResizingMemberDetail(false);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingMemberDetail]);

  useEffect(() => {
    if (!resizingMemberColumn) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => {
      const { x, width } = memberColumnResizeStartRef.current;
      const nextWidth = width + event.clientX - x;
      if (resizingMemberColumn === "hash") {
        setMemberHashFieldWidth(clampRedisHashFieldWidth(nextWidth));
      } else {
        setMemberZsetScoreWidth(clampRedisZsetScoreWidth(nextWidth));
      }
    };
    const handlePointerUp = () => {
      setResizingMemberColumn(null);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingMemberColumn]);

  useEffect(() => {
    const nextValueDraft = redisValueText(redis.selectedValue?.value ?? "");
    const selectedValueIsStringJson =
      redis.selectedValue?.key_type.toLowerCase() === "string" &&
      !redis.selectedValue.value_is_binary &&
      redisJsonValue(nextValueDraft) != null;
    setValueDraft(nextValueDraft);
    setValueDetailView(selectedValueIsStringJson ? "json" : "raw");
    setValueFormatError("");
    setTtlDraft(
      redis.selectedValue?.ttl != null && redis.selectedValue.ttl >= 0
        ? String(redis.selectedValue.ttl)
        : "",
    );
    setEditingTtl(false);
    setTtlError("");
    setSelectedMemberId(null);
    setLoadingMoreMembers(false);
    setEditingMemberId(null);
    setMemberDetailView("json");
    setMemberEditValue("");
    setSavingMember(false);
    setNewMemberField("");
    setNewMemberValue("");
    setNewMemberScore("0");
    setAddingMember(false);
    setMemberActionError("");
  }, [redis.selectedValue]);

  useEffect(() => {
    const availableKeys = new Set(redis.keys.map((key) => key.key_raw));
    setSelectedKeyRaws((current) => {
      const next = new Set<string>();
      current.forEach((keyRaw) => {
        if (availableKeys.has(keyRaw)) next.add(keyRaw);
      });
      return next.size === current.size ? current : next;
    });
  }, [redis.keys]);

  const effectivePattern = (value = pattern) => redisKeySearchPattern(value, fuzzyKeySearch);
  const refreshKeys = (value = pattern) =>
    redis.scanKeys({ db: activeDb, pattern: effectivePattern(value), count: 100 });
  const appendCommandHistory = (entry: RedisCommandHistoryDraft, persist = false) => {
    setCommandHistory((current) => {
      const lastId = current.length > 0 ? current[current.length - 1].id : 0;
      const next = [...current, { ...entry, id: lastId + 1 }];
      if (persist) {
        saveRedisCommandHistory(
          connectionId,
          next.map((row) => ({
            prompt: row.prompt,
            command: row.command,
            output: row.output,
            error: row.error,
          })),
        );
      }
      return next;
    });
  };
  const clearCommandHistory = () => {
    setCommandHistory([]);
    clearRedisCommandHistory(connectionId);
  };
  const loadMoreKeys = () => {
    if (!redis.cursor) return;
    void redis.scanKeys({
      db: activeDb,
      pattern: effectivePattern(),
      cursor: redis.cursor,
      count: 100,
    });
  };
  const fetchAllKeys = async () => {
    if (!redis.cursor || fetchingAllKeys) return;
    stopFetchAllKeysRef.current = false;
    let loadedKeys = redis.keys.length;
    setFetchAllLoadedKeys(loadedKeys);
    setFetchAllTotalKeys(redis.totalKeys);
    setFetchingAllKeys(true);
    const seenCursors = new Set<number>();
    let nextCursor = redis.cursor;
    try {
      while (nextCursor !== 0 && !seenCursors.has(nextCursor) && !stopFetchAllKeysRef.current) {
        seenCursors.add(nextCursor);
        const result = await redis.scanKeys({
          db: activeDb,
          pattern: effectivePattern(),
          cursor: nextCursor,
          count: 100,
        });
        loadedKeys += result.keys.length;
        setFetchAllLoadedKeys(loadedKeys);
        setFetchAllTotalKeys(result.total_keys);
        nextCursor = result.cursor;
      }
    } finally {
      setFetchingAllKeys(false);
    }
  };
  const stopFetchAllKeys = () => {
    stopFetchAllKeysRef.current = true;
    setFetchingAllKeys(false);
  };
  const copyKeyName = (keyRaw: string) => {
    navigator.clipboard?.writeText(keyRaw).catch(() => undefined);
  };
  const copyValue = () => {
    if (!redis.selectedValue) return;
    navigator.clipboard
      ?.writeText(redisValueText(redis.selectedValue.value))
      .catch(() => undefined);
  };
  const selectedValueText = redisValueText(redis.selectedValue?.value ?? "");
  const selectedValueIsStream = redis.selectedValue?.key_type.toLowerCase() === "stream";
  const selectedValueIsBinaryString =
    redis.selectedValue?.key_type.toLowerCase() === "string" &&
    Boolean(redis.selectedValue.value_is_binary);
  const showValueDraftEditor = !selectedValueIsStream;
  const valueDraftDirty = Boolean(redis.selectedValue && valueDraft !== selectedValueText);
  const selectedKeyInfo = useMemo(
    () => redis.keys.find((key) => key.key_raw === redis.selectedValue?.key_raw) ?? null,
    [redis.keys, redis.selectedValue?.key_raw],
  );
  const selectedKeySizeLabel = selectedKeyInfo
    ? redisKeySizeLabel(selectedKeyInfo.key_type, selectedKeyInfo.size)
    : "";
  const selectedValueJson = useMemo(() => {
    if (
      redis.selectedValue?.key_type.toLowerCase() !== "string" ||
      redis.selectedValue.value_is_binary
    )
      return null;
    return redisJsonValue(valueDraft);
  }, [redis.selectedValue, valueDraft]);
  const memberKind = redisValueMemberKind(redis.selectedValue);
  const memberRows = useMemo(() => redisMemberRows(redis.selectedValue), [redis.selectedValue]);
  const streamMemberGroups = useMemo(
    () => (memberKind === "stream" ? redisStreamMemberGroups(memberRows) : []),
    [memberKind, memberRows],
  );
  const selectedMember = useMemo(
    () => memberRows.find((row) => row.id === selectedMemberId) ?? null,
    [memberRows, selectedMemberId],
  );
  const selectedMemberIsEditing = selectedMember != null && selectedMember.id === editingMemberId;
  const selectedMemberJsonText = useMemo(
    () =>
      selectedMember?.format === "json" ? redisJsonText(selectedMember.detailText, true) : null,
    [selectedMember],
  );
  const selectedMemberJsonValue = useMemo(
    () => (selectedMember?.format === "json" ? redisJsonValue(selectedMember.detailText) : null),
    [selectedMember],
  );
  const memberDetailText =
    selectedMemberJsonText && memberDetailView === "json"
      ? selectedMemberJsonText
      : (selectedMember?.detailText ?? "");
  const memberColumnKeys = useMemo(() => redisMemberColumnKeys(memberKind), [memberKind]);
  const canAddMember =
    memberKind === "list" || memberKind === "hash" || memberKind === "set" || memberKind === "zset";
  const canSubmitNewMember =
    canAddMember &&
    !readOnly &&
    !addingMember &&
    (memberKind === "hash" ? newMemberField.trim().length > 0 : newMemberValue.trim().length > 0);
  const canLoadMoreMembers =
    Boolean(redis.selectedValue?.scan_cursor && redis.selectedValue.scan_cursor > 0) &&
    memberKind != null &&
    memberKind !== "stream";
  const memberCountLabel = useMemo(() => {
    if (!redis.selectedValue || !memberKind) return "";
    const loaded = memberRows.length;
    const total = redis.selectedValue.total ?? null;
    if (memberKind === "hash") {
      return total != null && total > loaded
        ? t("database.redisLoadedFields", { loaded, total })
        : t("database.redisFields", { count: loaded });
    }
    if (memberKind === "zset") {
      return total != null && total > loaded
        ? t("database.redisLoadedMembers", { loaded, total })
        : t("database.redisMembers", { count: loaded });
    }
    if (memberKind === "stream")
      return t("database.redisEntries", { count: redisStreamEntryCount(redis.selectedValue) });
    return total != null && total > loaded
      ? t("database.redisLoadedItems", { loaded, total })
      : t("database.redisItems", { count: loaded });
  }, [memberKind, memberRows.length, redis.selectedValue, t]);
  useEffect(() => {
    setMemberDetailView(selectedMember?.format === "json" ? "json" : "raw");
  }, [selectedMember?.format, selectedMember?.id]);
  const copyMember = (row: RedisMemberRow) => {
    navigator.clipboard?.writeText(row.copyText).catch(() => undefined);
  };
  const closeMemberDetail = () => {
    setEditingMemberId(null);
    setSelectedMemberId(null);
  };
  const startMemberDetailResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    memberDetailResizeStartRef.current = { x: event.clientX, width: memberDetailPanelWidth };
    setResizingMemberDetail(true);
  };
  const startMemberColumnResize = (
    kind: "hash" | "zset",
    event: ReactPointerEvent<HTMLDivElement>,
  ) => {
    event.preventDefault();
    memberColumnResizeStartRef.current = {
      x: event.clientX,
      width: kind === "hash" ? memberHashFieldWidth : memberZsetScoreWidth,
    };
    setResizingMemberColumn(kind);
  };
  const startEditMember = (row: RedisMemberRow) => {
    if (readOnly || !row.editAction) return;
    setSelectedMemberId(row.id);
    setMemberEditValue(row.detailText);
    setEditingMemberId(row.id);
  };
  const cancelMemberEdit = () => {
    setMemberEditValue(selectedMember?.detailText ?? "");
    setEditingMemberId(null);
  };
  const saveMemberEdit = async () => {
    if (!redis.selectedValue || readOnly || !selectedMember?.editAction) return;
    const keyRaw = redis.selectedValue.key_raw;
    const action = selectedMember.editAction;
    setSavingMember(true);
    try {
      if (action.kind === "hash") {
        await redis.setHashField({
          db: activeDb,
          keyRaw,
          field: action.field,
          value: memberEditValue,
        });
      } else if (action.kind === "list") {
        await redis.setListItem({
          db: activeDb,
          keyRaw,
          index: action.index,
          value: memberEditValue,
        });
      } else if (action.kind === "set") {
        await redis.removeSetMember({ db: activeDb, keyRaw, member: action.member });
        await redis.addSetMember({ db: activeDb, keyRaw, member: memberEditValue });
      } else {
        await redis.removeZsetMember({ db: activeDb, keyRaw, member: action.member });
        await redis.addZsetMember({
          db: activeDb,
          keyRaw,
          member: memberEditValue,
          score: action.score,
        });
      }
      setEditingMemberId(null);
      setMemberEditValue("");
      setSelectedMemberId(null);
      await redis.loadValue(activeDb, keyRaw);
      await refreshKeys();
    } finally {
      setSavingMember(false);
    }
  };
  const updateMemberJsonDraft = (pretty: boolean) => {
    if (!selectedMember?.editAction || readOnly) return;
    const source = selectedMemberIsEditing ? memberEditValue : selectedMember.detailText;
    const result = redisJsonText(source, pretty);
    if (result == null) {
      setMemberActionError(t("database.redisJsonFormatError"));
      return;
    }
    setMemberActionError("");
    setSelectedMemberId(selectedMember.id);
    setMemberEditValue(result);
    setEditingMemberId(selectedMember.id);
  };
  const updateMemberJsonWordWrap = (enabled: boolean) => {
    setMemberJsonWordWrap(enabled);
    saveRedisJsonWordWrap(enabled);
  };
  const addMember = async () => {
    if (!redis.selectedValue || readOnly || !canAddMember) return;
    const keyRaw = redis.selectedValue.key_raw;
    const value = newMemberValue;
    if (memberKind === "hash" && !newMemberField.trim()) {
      setMemberActionError(t("database.redisMemberFieldRequired"));
      return;
    }
    if ((memberKind === "list" || memberKind === "set" || memberKind === "zset") && !value.trim()) {
      setMemberActionError(t("database.redisMemberValueRequired"));
      return;
    }
    const score = Number.parseFloat(newMemberScore.trim() || "0");
    if (memberKind === "zset" && !Number.isFinite(score)) {
      setMemberActionError(t("database.redisMemberScoreInvalid"));
      return;
    }

    setAddingMember(true);
    setMemberActionError("");
    try {
      if (memberKind === "list") {
        await redis.pushListItem({ db: activeDb, keyRaw, value });
      } else if (memberKind === "hash") {
        await redis.setHashField({ db: activeDb, keyRaw, field: newMemberField.trim(), value });
      } else if (memberKind === "set") {
        await redis.addSetMember({ db: activeDb, keyRaw, member: value });
      } else if (memberKind === "zset") {
        await redis.addZsetMember({ db: activeDb, keyRaw, member: value, score });
      }
      setNewMemberField("");
      setNewMemberValue("");
      setNewMemberScore("0");
      setSelectedMemberId(null);
      await redis.loadValue(activeDb, keyRaw);
      await refreshKeys();
    } finally {
      setAddingMember(false);
    }
  };
  const deleteMember = async (row: RedisMemberRow) => {
    if (!redis.selectedValue || readOnly || !row.deleteAction) return;
    const keyRaw = redis.selectedValue.key_raw;
    const message =
      row.deleteAction.kind === "hash"
        ? t("database.confirmDeleteRedisHashField", { key: keyRaw, field: row.deleteAction.field })
        : row.deleteAction.kind === "list"
          ? t("database.confirmDeleteRedisListItem", { key: keyRaw, index: row.deleteAction.index })
          : t("database.confirmDeleteRedisSetMember", {
              key: keyRaw,
              member: row.deleteAction.member,
            });
    const ok = await confirm(message, {
      title: t("database.redisDeleteMember"),
      kind: "warning",
      okLabel: t("database.redisDeleteMember"),
      cancelLabel: t("common.cancel"),
    });
    if (!ok) return;

    if (row.deleteAction.kind === "hash") {
      await redis.deleteHashField({ db: activeDb, keyRaw, field: row.deleteAction.field });
    } else if (row.deleteAction.kind === "list") {
      await redis.removeListItem({ db: activeDb, keyRaw, index: row.deleteAction.index });
    } else if (row.deleteAction.kind === "set") {
      await redis.removeSetMember({ db: activeDb, keyRaw, member: row.deleteAction.member });
    } else {
      await redis.removeZsetMember({ db: activeDb, keyRaw, member: row.deleteAction.member });
    }

    setSelectedMemberId(null);
    await redis.loadValue(activeDb, keyRaw);
    await refreshKeys();
  };
  const loadMoreMembers = async () => {
    if (!redis.selectedValue || !canLoadMoreMembers) return;
    const cursor = redis.selectedValue.scan_cursor ?? 0;
    setLoadingMoreMembers(true);
    try {
      await redis.loadMoreValue({
        db: activeDb,
        keyRaw: redis.selectedValue.key_raw,
        keyType: redis.selectedValue.key_type.toLowerCase(),
        cursor,
        count: 200,
      });
    } finally {
      setLoadingMoreMembers(false);
    }
  };
  const copyInsertStatement = () => {
    if (!redis.selectedValue) return;
    if (redis.selectedValue.value_is_binary) {
      setValueFormatError(t("database.redisCopyInsertStatementBinary"));
      return;
    }
    const statement = redisInsertStatement(redis.selectedValue);
    if (!statement) return;
    setValueFormatError("");
    navigator.clipboard?.writeText(statement).catch(() => undefined);
  };
  const updateJsonDraft = (pretty: boolean) => {
    if (!redis.selectedValue || readOnly || redis.selectedValue.value_is_binary) return;
    try {
      const parsed = JSON.parse(valueDraft);
      setValueDraft(JSON.stringify(parsed, null, pretty ? 2 : 0));
      setValueFormatError("");
    } catch {
      setValueFormatError(t("database.redisJsonFormatError"));
    }
  };
  const discardValueDraft = () => {
    if (!redis.selectedValue) return;
    setValueDraft(selectedValueText);
    setValueFormatError("");
  };
  const refreshKey = async (keyRaw: string) => {
    await redis.loadValue(activeDb, keyRaw);
    await refreshKeys();
  };
  const startEditTtl = () => {
    if (!redis.selectedValue || readOnly) return;
    setTtlDraft(redis.selectedValue.ttl > 0 ? String(redis.selectedValue.ttl) : "");
    setTtlError("");
    setEditingTtl(true);
  };
  const saveTtl = async () => {
    if (!redis.selectedValue || readOnly) return;
    const rawTtl = ttlDraft.trim();
    const ttl = rawTtl === "" || rawTtl === "-1" ? -1 : Number.parseInt(rawTtl, 10);
    if (Number.isNaN(ttl)) {
      setTtlError(t("database.redisTtlInvalid"));
      return;
    }
    await redis.setTtl({
      db: activeDb,
      keyRaw: redis.selectedValue.key_raw,
      ttl,
    });
    setEditingTtl(false);
    setTtlError("");
    await redis.loadValue(activeDb, redis.selectedValue.key_raw);
    await refreshKeys();
  };
  const saveValue = async () => {
    if (!redis.selectedValue || readOnly) return;
    if (selectedValueIsBinaryString || selectedValueIsStream) return;
    const keyRaw = redis.selectedValue.key_raw;
    const rawTtl = ttlDraft.trim();
    const ttl = rawTtl === "" || rawTtl === "-1" ? -1 : Number.parseInt(rawTtl, 10);
    if (Number.isNaN(ttl)) {
      setTtlError(t("database.redisTtlInvalid"));
      return;
    }
    await redis.setValue({
      db: activeDb,
      keyRaw,
      value: valueDraft,
      ttl,
    });
    setTtlError("");
    await redis.loadValue(activeDb, keyRaw);
    await refreshKeys();
  };
  const deleteKey = async (keyRaw: string) => {
    const ok = await confirm(t("database.confirmDeleteRedisKey", { name: keyRaw }), {
      title: t("database.redisDeleteKey"),
      kind: "warning",
      okLabel: t("database.redisDeleteKey"),
      cancelLabel: t("common.cancel"),
    });
    if (!ok) return;
    await redis.deleteKey(activeDb, keyRaw);
  };
  const deleteSelectedKeys = async () => {
    const keyRaws = [...selectedKeyRaws];
    if (keyRaws.length === 0) return;
    const ok = await confirm(t("database.confirmDeleteRedisKeys", { count: keyRaws.length }), {
      title: t("database.redisDeleteSelectedKeys"),
      kind: "warning",
      okLabel: t("database.redisDeleteSelectedKeys"),
      cancelLabel: t("common.cancel"),
    });
    if (!ok) return;
    for (const keyRaw of keyRaws) {
      await redis.deleteKey(activeDb, keyRaw);
    }
    setSelectedKeyRaws(new Set());
  };
  const deleteKeyGroup = async (groupName: string, keyRaws: string[]) => {
    if (keyRaws.length === 0) return;
    const ok = await confirm(
      t("database.confirmDeleteRedisKeyGroup", { name: groupName, count: keyRaws.length }),
      {
        title: t("database.redisDeleteKeyGroup"),
        kind: "warning",
        okLabel: t("database.redisDeleteKeyGroup"),
        cancelLabel: t("common.cancel"),
      },
    );
    if (!ok) return;
    for (const keyRaw of keyRaws) {
      await redis.deleteKey(activeDb, keyRaw);
    }
  };
  const flushCurrentDb = async () => {
    const ok = await confirm(t("database.confirmFlushRedisDb", { db: activeDb }), {
      title: t("database.redisFlushDb"),
      kind: "warning",
      okLabel: t("database.redisFlushDbConfirm"),
      cancelLabel: t("common.cancel"),
    });
    if (!ok) return;
    await redis.executeCommand({
      db: activeDb,
      command: "FLUSHDB",
      skipSafetyCheck: true,
    });
    redis.clearSelectedValue();
    setSelectedKeyRaws(new Set());
    await redis.scanKeys({ db: activeDb, pattern: effectivePattern(), count: 100 });
    await redis.loadDatabases();
  };
  const openKeyContextMenu = (event: MouseEvent, keyRaw: string) => {
    event.preventDefault();
    setGroupContextMenu(null);
    setKeyContextMenu({
      x: event.clientX,
      y: event.clientY,
      keyRaw,
    });
  };
  const openGroupContextMenu = (event: MouseEvent, groupName: string, keyRaws: string[]) => {
    event.preventDefault();
    setKeyContextMenu(null);
    setGroupContextMenu({
      x: event.clientX,
      y: event.clientY,
      groupName,
      keyRaws,
    });
  };

  const redisKeyTree = useMemo(
    () => buildRedisKeyTree(redis.keys, activeDb, keySeparator || ":"),
    [activeDb, keySeparator, redis.keys],
  );
  const redisKeyRows = useMemo(
    () => flattenVisibleRedisKeyTree(redisKeyTree, expandedKeyGroups),
    [expandedKeyGroups, redisKeyTree],
  );

  useEffect(() => {
    const availableGroups = collectRedisGroupIds(redisKeyTree);
    setExpandedKeyGroups((current) => {
      const next = new Set<string>();
      current.forEach((id) => {
        if (availableGroups.has(id)) next.add(id);
      });
      if (next.size === current.size) return current;
      return next;
    });
  }, [redisKeyTree]);

  const toggleKeyGroup = (groupId: string) => {
    setExpandedKeyGroups((current) => {
      const next = new Set(current);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  };
  const toggleSelectedKey = (keyRaw: string) => {
    setSelectedKeyRaws((current) => {
      const next = new Set(current);
      if (next.has(keyRaw)) next.delete(keyRaw);
      else next.add(keyRaw);
      return next;
    });
  };
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
    const command = commandText.trim();
    const prompt = `db${commandDb}>`;
    if (!command) {
      appendCommandHistory({
        prompt,
        command: "",
        output: t("database.redisCommandEmpty"),
        error: true,
      });
      return;
    }
    if (isRedisClearScreenCommand(command)) {
      setCommandHistory([]);
      setCommandText("");
      return;
    }
    const safety = classifyRedisCommandSafety(command);
    if (safety === "blocked") {
      appendCommandHistory({
        prompt,
        command,
        output: t("database.redisCommandBlocked", { command }),
        error: true,
      });
      setCommandText("");
      return;
    }
    if (safety === "confirm") {
      const ok = await confirm(t("database.confirmRedisCommand", { command }), {
        title: t("database.redisCommandRequiresConfirmation"),
        kind: "warning",
        okLabel: t("database.redisRunCommand"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
    }
    setCommandText("");
    setCommandRunning(true);
    try {
      const result = await redis.executeCommand({
        db: commandDb,
        command,
        skipSafetyCheck: safety === "confirm",
      });
      appendCommandHistory(
        {
          prompt,
          command,
          output: formatRedisCommandResult(result.value),
          error: false,
        },
        true,
      );
      setCommandDb((currentDb) => nextRedisCommandDb(currentDb, command, result.value));
      if (safety === "confirm" || result.safety === "confirm") {
        await refreshKeys();
      }
    } catch (error) {
      appendCommandHistory(
        {
          prompt,
          command,
          output: error instanceof Error ? error.message : String(error),
          error: true,
        },
        true,
      );
    } finally {
      setCommandRunning(false);
    }
  };

  const selectDatabase = (database: number) => {
    setActiveDb(database);
    setCommandDb(database);
    setExpandedKeyGroups(new Set());
    setSelectedKeyRaws(new Set());
    setSelectedMemberId(null);
    setLoadingMoreMembers(false);
    setEditingMemberId(null);
    setMemberDetailView("json");
    setMemberEditValue("");
    setSavingMember(false);
    setNewMemberField("");
    setNewMemberValue("");
    setNewMemberScore("0");
    setAddingMember(false);
    setMemberActionError("");
    setValueDraft("");
    setValueFormatError("");
    setTtlDraft("");
    setEditingTtl(false);
    setTtlError("");
    setKeyContextMenu(null);
    setGroupContextMenu(null);
    redis.clearKeyspaceState();
    void redis.scanKeys({ db: database, pattern: effectivePattern(), count: 100 });
  };

  const renderMemberRowActions = (row: RedisMemberRow) => (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
      <DbxButton
        variant="ghost"
        size="icon-xs"
        icon={Eye}
        onClick={(event) => {
          event.stopPropagation();
          setSelectedMemberId(row.id);
        }}
        aria-label={`${t("database.redisViewMember")}: ${row.title}`}
        title={t("database.redisViewMember")}
      />
      <DbxButton
        variant="ghost"
        size="icon-xs"
        icon={Copy}
        onClick={(event) => {
          event.stopPropagation();
          copyMember(row);
        }}
        aria-label={`${t("database.redisCopyMember")}: ${row.title}`}
        title={t("database.redisCopyMember")}
      />
      {row.editAction && (
        <DbxButton
          variant="ghost"
          size="icon-xs"
          icon={Pencil}
          disabled={readOnly || savingMember}
          onClick={(event) => {
            event.stopPropagation();
            startEditMember(row);
          }}
          aria-label={`${t("database.redisEditMember")}: ${row.title}`}
          title={t("database.redisEditMember")}
        />
      )}
      {row.deleteAction && (
        <DbxButton
          variant="destructive"
          size="icon-xs"
          icon={Trash2}
          disabled={readOnly}
          onClick={(event) => {
            event.stopPropagation();
            void deleteMember(row);
          }}
          aria-label={`${t("database.redisDeleteMember")}: ${row.title}`}
          title={t("database.redisDeleteMember")}
        />
      )}
    </span>
  );

  return (
    <div style={s.databaseBrowserRoot}>
      <RedisKeyTreePane
        databases={redis.databases}
        activeDb={activeDb}
        pattern={pattern}
        fuzzyKeySearch={fuzzyKeySearch}
        keyRows={redisKeyRows}
        expandedKeyGroups={expandedKeyGroups}
        selectedKeyRaws={selectedKeyRaws}
        activeKeyRaw={redis.selectedValue?.key_raw ?? null}
        keySeparator={keySeparator || ":"}
        keyCount={redis.keys.length}
        totalKeys={redis.totalKeys}
        cursor={redis.cursor}
        loading={redis.loading}
        readOnly={readOnly}
        fetchingAllKeys={fetchingAllKeys}
        fetchAllLoadedKeys={fetchAllLoadedKeys}
        fetchAllTotalKeys={fetchAllTotalKeys}
        onRefresh={refreshKeys}
        onSelectDatabase={selectDatabase}
        onPatternChange={setPattern}
        onSearch={refreshKeys}
        onToggleFuzzySearch={() => setFuzzyKeySearch((value) => !value)}
        onDeleteSelectedKeys={deleteSelectedKeys}
        onToggleKeyGroup={toggleKeyGroup}
        onOpenGroupContextMenu={openGroupContextMenu}
        onLoadKey={(keyRaw) => redis.loadValue(activeDb, keyRaw)}
        onOpenKeyContextMenu={openKeyContextMenu}
        onToggleSelectedKey={toggleSelectedKey}
        onLoadMoreKeys={loadMoreKeys}
        onFetchAllKeys={fetchAllKeys}
        onStopFetchAllKeys={stopFetchAllKeys}
      />
      <div style={s.databaseBrowserMain}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>
              {redis.selectedValue?.key_display ?? t("database.redisValue")}
            </div>
            <div style={s.databaseDialogHint}>
              {redis.selectedValue ? (
                <span
                  style={{ display: "inline-flex", alignItems: "center", flexWrap: "wrap", gap: 6 }}
                >
                  <span style={s.databasePill}>{redis.selectedValue.key_type}</span>
                  {selectedKeySizeLabel && (
                    <span style={s.databasePill}>
                      {t("database.redisColumnSize")}: {selectedKeySizeLabel}
                    </span>
                  )}
                  {!editingTtl ? (
                    <button
                      type="button"
                      style={{
                        ...s.databasePill,
                        cursor: readOnly ? "default" : "pointer",
                        borderColor: "var(--border)",
                      }}
                      disabled={readOnly}
                      onClick={startEditTtl}
                    >
                      {redis.selectedValue.ttl > 0
                        ? t("database.redisTtlSeconds", { ttl: redis.selectedValue.ttl })
                        : t("database.redisNoExpiry")}
                    </button>
                  ) : (
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                      <input
                        aria-label={t("database.redisTtlSecondsInput")}
                        style={{
                          ...s.databaseDialogInput,
                          width: 118,
                          height: 26,
                          padding: "3px 8px",
                        }}
                        value={ttlDraft}
                        onChange={(event) => {
                          setTtlDraft(event.target.value);
                          setTtlError("");
                        }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") void saveTtl();
                          if (event.key === "Escape") setEditingTtl(false);
                        }}
                        placeholder="-1"
                        autoFocus
                      />
                      <DbxButton
                        variant="ghost"
                        size="icon-sm"
                        icon={Save}
                        onClick={() => void saveTtl()}
                        aria-label={t("database.redisSaveTtl")}
                        title={t("database.redisSaveTtl")}
                      />
                      {ttlError && <span style={{ color: "var(--danger)" }}>{ttlError}</span>}
                    </span>
                  )}
                </span>
              ) : (
                t("database.redisSelectKey")
              )}
            </div>
          </div>
          <div style={s.databaseButtonRow}>
            <DbxButton
              variant="ghost"
              size="icon-sm"
              icon={RefreshCcw}
              disabled={!redis.selectedValue}
              onClick={() => redis.selectedValue && void refreshKey(redis.selectedValue.key_raw)}
              aria-label={t("database.refresh")}
              title={t("database.refresh")}
            />
            <DbxButton
              variant="ghost"
              size="icon-sm"
              icon={Copy}
              disabled={!redis.selectedValue}
              onClick={copyValue}
              aria-label={t("database.copyValue")}
              title={t("database.copyValue")}
            />
            <DbxButton
              variant="ghost"
              size="icon-sm"
              icon={Terminal}
              disabled={!redis.selectedValue}
              onClick={copyInsertStatement}
              aria-label={t("database.copyRedisInsertStatement")}
              title={t("database.copyRedisInsertStatement")}
            />
            <DbxButton
              variant="outline"
              size="sm"
              icon={Plus}
              disabled={readOnly}
              onClick={() => setShowCreateKey((value) => !value)}
            >
              {t("database.redisCreateKey")}
            </DbxButton>
            <DbxButton
              variant="outline"
              size="sm"
              icon={Trash2}
              disabled={readOnly || !redis.selectedValue}
              onClick={() => redis.selectedValue && void deleteKey(redis.selectedValue.key_raw)}
            >
              {t("database.redisDeleteKey")}
            </DbxButton>
            <DbxButton
              variant="destructive"
              size="sm"
              icon={Eraser}
              disabled={readOnly}
              onClick={() => void flushCurrentDb()}
            >
              {t("database.redisFlushDb")}
            </DbxButton>
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
              <input
                style={s.databaseDialogInput}
                value={createKeyField}
                onChange={(event) => setCreateKeyField(event.target.value)}
              />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisCreateScore")}</span>
              <input
                style={s.databaseDialogInput}
                value={createKeyScore}
                onChange={(event) => setCreateKeyScore(event.target.value)}
              />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisCreateEntryId")}</span>
              <input
                style={s.databaseDialogInput}
                value={createKeyEntryId}
                onChange={(event) => setCreateKeyEntryId(event.target.value)}
              />
            </label>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.redisTtl")}</span>
              <input
                style={s.databaseDialogInput}
                value={createKeyTtl}
                onChange={(event) => setCreateKeyTtl(event.target.value)}
              />
            </label>
            <DbxButton
              variant="default"
              size="sm"
              onClick={() => void createKey()}
              disabled={!createKeyName.trim()}
            >
              {t("database.saveKey")}
            </DbxButton>
          </div>
        )}
        {memberKind && (
          <div
            style={{
              flexShrink: 0,
              display: "flex",
              flexDirection: "column",
              border: "1px solid var(--border-medium)",
              borderRadius: 8,
              overflow: "hidden",
              background: "var(--bg-panel)",
            }}
          >
            <div
              style={{
                minHeight: 34,
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 10px",
                borderBottom: "1px solid var(--border-dim)",
              }}
            >
              <span style={{ ...s.databaseDialogHint, flex: "1 1 140px" }}>{memberCountLabel}</span>
              {canAddMember && (
                <form
                  style={{ display: "inline-flex", alignItems: "center", flexWrap: "wrap", gap: 6 }}
                  onSubmit={(event) => {
                    event.preventDefault();
                    void addMember();
                  }}
                >
                  {memberKind === "hash" && (
                    <input
                      aria-label={t("database.redisNewMemberField")}
                      style={{
                        ...s.databaseDialogInput,
                        width: 110,
                        height: 26,
                        padding: "3px 8px",
                      }}
                      value={newMemberField}
                      disabled={readOnly || addingMember}
                      onChange={(event) => {
                        setNewMemberField(event.target.value);
                        setMemberActionError("");
                      }}
                      placeholder={t("database.redisColumnField")}
                    />
                  )}
                  {memberKind === "zset" && (
                    <input
                      aria-label={t("database.redisNewMemberScore")}
                      style={{
                        ...s.databaseDialogInput,
                        width: 86,
                        height: 26,
                        padding: "3px 8px",
                      }}
                      value={newMemberScore}
                      disabled={readOnly || addingMember}
                      onChange={(event) => {
                        setNewMemberScore(event.target.value);
                        setMemberActionError("");
                      }}
                      placeholder={t("database.redisColumnScore")}
                    />
                  )}
                  <input
                    aria-label={t("database.redisNewMemberValue")}
                    style={{
                      ...s.databaseDialogInput,
                      width: memberKind === "hash" || memberKind === "zset" ? 132 : 170,
                      height: 26,
                      padding: "3px 8px",
                    }}
                    value={newMemberValue}
                    disabled={readOnly || addingMember}
                    onChange={(event) => {
                      setNewMemberValue(event.target.value);
                      setMemberActionError("");
                    }}
                    placeholder={
                      memberKind === "set" || memberKind === "zset"
                        ? t("database.redisColumnMember")
                        : t("database.redisColumnValue")
                    }
                  />
                  <DbxButton
                    type="submit"
                    variant="default"
                    size="xs"
                    icon={Plus}
                    disabled={!canSubmitNewMember}
                  >
                    <span>
                      {memberKind === "list"
                        ? t("database.redisPushMember")
                        : memberKind === "hash"
                          ? t("database.redisSetMember")
                          : t("database.redisAddMember")}
                    </span>
                  </DbxButton>
                </form>
              )}
              {selectedMember && (
                <DbxButton
                  variant="outline"
                  size="sm"
                  icon={Copy}
                  onClick={() => copyMember(selectedMember)}
                  aria-label={`${t("database.redisCopyMember")}: ${selectedMember.title}`}
                  title={t("database.redisCopyMember")}
                >
                  {t("database.redisCopyMember")}
                </DbxButton>
              )}
            </div>
            {memberActionError && (
              <div
                style={{
                  ...s.databaseError,
                  margin: 0,
                  borderRadius: 0,
                  borderLeft: "none",
                  borderRight: "none",
                  borderTop: "none",
                }}
              >
                {memberActionError}
              </div>
            )}
            {memberKind === "stream" ? (
              <div
                role="list"
                aria-label={t("database.redisEntries", { count: streamMemberGroups.length })}
                style={{ ...s.databaseTableWrap, flex: "unset", height: 190 }}
              >
                {streamMemberGroups.map((group) => (
                  <div
                    key={group.id}
                    role="listitem"
                    data-redis-stream-entry
                    style={{
                      padding: "8px 10px",
                      borderBottom: "1px solid var(--border-dim)",
                      fontFamily: "var(--font-mono)",
                      fontSize: 12,
                    }}
                  >
                    <div style={{ marginBottom: 5, color: "var(--text-muted)", fontSize: 11.5 }}>
                      {group.entryId}
                    </div>
                    <div style={{ display: "grid", gap: 2 }}>
                      {group.rows.map((row) => {
                        const active = selectedMember?.id === row.id;
                        const field = row.cells[1] ?? "";
                        const fieldValue = row.cells[2] ?? "";
                        return (
                          <div
                            key={row.id}
                            style={{
                              minHeight: 28,
                              display: "grid",
                              gridTemplateColumns: "minmax(96px, 0.35fr) minmax(0, 1fr) 64px",
                              alignItems: "center",
                              gap: 8,
                              padding: "2px 4px",
                              borderRadius: 6,
                              cursor: "pointer",
                              background: active ? "var(--bg-hover)" : undefined,
                            }}
                            onClick={() => {
                              setSelectedMemberId(row.id);
                              setEditingMemberId(null);
                            }}
                          >
                            <span
                              style={{
                                minWidth: 0,
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                                color: "var(--accent)",
                              }}
                              title={field}
                            >
                              {field}
                            </span>
                            <span
                              style={{
                                minWidth: 0,
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                                color: "var(--text-secondary)",
                              }}
                              title={fieldValue}
                            >
                              {fieldValue}
                            </span>
                            <span style={{ display: "flex", justifyContent: "flex-end" }}>
                              {renderMemberRowActions(row)}
                            </span>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                ))}
                {streamMemberGroups.length === 0 && (
                  <div style={s.databaseEmptyCompact}>{t("database.empty")}</div>
                )}
              </div>
            ) : (
              <div style={{ ...s.databaseTableWrap, flex: "unset", height: 190 }}>
                <table style={s.databaseTable}>
                  {(memberKind === "hash" || memberKind === "zset") && (
                    <colgroup>
                      <col
                        style={{
                          width:
                            memberKind === "hash" ? memberHashFieldWidth : memberZsetScoreWidth,
                        }}
                      />
                      <col />
                      <col style={{ width: 140 }} />
                    </colgroup>
                  )}
                  <thead>
                    <tr>
                      {memberColumnKeys.map((labelKey, index) => {
                        const resizeKind =
                          memberKind === "hash" || memberKind === "zset" ? memberKind : null;
                        const resizable = index === 0 && resizeKind != null;
                        const resizeLabel =
                          resizeKind === "hash"
                            ? t("database.redisResizeHashFieldColumn")
                            : t("database.redisResizeZsetScoreColumn");
                        return (
                          <th
                            key={labelKey}
                            style={{
                              ...s.databaseTh,
                              ...(resizable
                                ? {
                                    position: "sticky",
                                    width:
                                      resizeKind === "hash"
                                        ? memberHashFieldWidth
                                        : memberZsetScoreWidth,
                                    userSelect: "none",
                                  }
                                : {}),
                            }}
                          >
                            {t(labelKey)}
                            {resizable && (
                              <div
                                role="separator"
                                aria-orientation="vertical"
                                aria-label={resizeLabel}
                                title={resizeLabel}
                                onPointerDown={(event) => {
                                  if (resizeKind) startMemberColumnResize(resizeKind, event);
                                }}
                                style={{
                                  position: "absolute",
                                  top: 0,
                                  right: -4,
                                  width: 8,
                                  height: "100%",
                                  cursor: "col-resize",
                                  zIndex: 2,
                                  borderRight:
                                    resizingMemberColumn === resizeKind
                                      ? "1px solid var(--accent)"
                                      : "1px solid transparent",
                                }}
                              />
                            )}
                          </th>
                        );
                      })}
                      <th style={{ ...s.databaseTh, width: 140 }} />
                    </tr>
                  </thead>
                  <tbody>
                    {memberRows.map((row) => {
                      const active = selectedMember?.id === row.id;
                      return (
                        <tr
                          key={row.id}
                          style={{
                            cursor: "pointer",
                            background: active ? "var(--bg-hover)" : undefined,
                          }}
                          onClick={() => {
                            setSelectedMemberId(row.id);
                            setEditingMemberId(null);
                          }}
                        >
                          {row.cells.map((cell, index) => (
                            <td
                              key={`${row.id}:${index}`}
                              style={{
                                ...s.databaseTd,
                                color:
                                  index === 0 && row.kind === "hash"
                                    ? "var(--accent)"
                                    : s.databaseTd.color,
                                fontFamily: "var(--font-mono)",
                              }}
                              title={cell}
                            >
                              {cell}
                            </td>
                          ))}
                          <td style={{ ...s.databaseTd, width: 140 }}>
                            {renderMemberRowActions(row)}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
            {canLoadMoreMembers && (
              <div style={{ padding: 8, borderTop: "1px solid var(--border-dim)" }}>
                <button
                  type="button"
                  style={{ ...s.databaseListButton, justifyContent: "center" }}
                  disabled={loadingMoreMembers}
                  onClick={() => void loadMoreMembers()}
                >
                  {t("database.loadMore")} ({memberRows.length}/
                  {redis.selectedValue?.total ?? memberRows.length})
                </button>
              </div>
            )}
            {selectedMember && (
              <div
                role="dialog"
                aria-label={`${t("database.redisMemberDetail")}: ${selectedMember.title}`}
                style={{
                  position: "fixed",
                  top: 12,
                  right: 12,
                  bottom: 12,
                  zIndex: 60,
                  width: memberDetailPanelWidth,
                  maxWidth: "calc(100vw - 24px)",
                  display: "flex",
                  flexDirection: "column",
                  border: "1px solid var(--border-medium)",
                  borderRadius: 8,
                  background: "var(--bg-panel)",
                  boxShadow: "0 18px 60px rgba(0, 0, 0, 0.35)",
                  overflow: "hidden",
                  userSelect: resizingMemberDetail ? "none" : undefined,
                }}
              >
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={t("database.redisResizeMemberDetail")}
                  title={t("database.redisResizeMemberDetail")}
                  onPointerDown={startMemberDetailResize}
                  style={{
                    position: "absolute",
                    top: 0,
                    bottom: 0,
                    left: -4,
                    width: 8,
                    cursor: "col-resize",
                    zIndex: 1,
                    borderLeft: resizingMemberDetail
                      ? "1px solid var(--accent)"
                      : "1px solid transparent",
                  }}
                />
                <div
                  style={{
                    minHeight: 42,
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    padding: "0 12px",
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
                      fontSize: 13,
                      fontWeight: 600,
                    }}
                  >
                    {t("database.redisMemberDetail")}: {selectedMember.title}
                  </span>
                  <span style={s.databasePill}>
                    {selectedMember.format === "json" ? "JSON" : t("database.redisRawContent")}
                  </span>
                  {!selectedMemberIsEditing && (
                    <DbxButton
                      variant="ghost"
                      size="icon-xs"
                      icon={Copy}
                      onClick={() => copyMember(selectedMember)}
                      aria-label={`${t("database.redisCopyMember")}: ${selectedMember.title}`}
                      title={t("database.redisCopyMember")}
                    />
                  )}
                  {selectedMemberIsEditing ? (
                    <>
                      <DbxButton
                        variant="ghost"
                        size="icon-xs"
                        icon={Save}
                        disabled={savingMember}
                        onClick={() => void saveMemberEdit()}
                        aria-label={t("database.redisSaveMember")}
                        title={t("database.redisSaveMember")}
                      />
                      <DbxButton
                        variant="ghost"
                        size="icon-xs"
                        icon={X}
                        disabled={savingMember}
                        onClick={cancelMemberEdit}
                        aria-label={t("database.redisCancelMemberEdit")}
                        title={t("database.redisCancelMemberEdit")}
                      />
                    </>
                  ) : (
                    selectedMember.editAction && (
                      <DbxButton
                        variant="ghost"
                        size="icon-xs"
                        icon={Pencil}
                        disabled={readOnly || savingMember}
                        onClick={() => startEditMember(selectedMember)}
                        aria-label={`${t("database.redisEditMember")}: ${selectedMember.title}`}
                        title={t("database.redisEditMember")}
                      />
                    )
                  )}
                  <DbxButton
                    variant="ghost"
                    size="icon-xs"
                    icon={X}
                    onClick={closeMemberDetail}
                    aria-label={t("common.close")}
                    title={t("common.close")}
                  />
                </div>
                {selectedMemberIsEditing && selectedMember.format === "json" && (
                  <div
                    style={{
                      minHeight: 34,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "flex-end",
                      gap: 6,
                      padding: "0 10px",
                      borderBottom: "1px solid var(--border-dim)",
                    }}
                  >
                    <DbxButton
                      variant="ghost"
                      size="icon-xs"
                      icon={Braces}
                      disabled={savingMember}
                      onClick={() => updateMemberJsonDraft(true)}
                      aria-label={t("database.redisFormatMemberJson")}
                      title={t("database.redisFormatMemberJson")}
                    />
                    <DbxButton
                      variant="ghost"
                      size="icon-xs"
                      icon={Minimize2}
                      disabled={savingMember}
                      onClick={() => updateMemberJsonDraft(false)}
                      aria-label={t("database.redisCompressMemberJson")}
                      title={t("database.redisCompressMemberJson")}
                    />
                  </div>
                )}
                {!selectedMemberIsEditing && selectedMember.format === "json" && (
                  <div
                    style={{
                      minHeight: 36,
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "0 10px",
                      borderBottom: "1px solid var(--border-dim)",
                    }}
                  >
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                      <DbxButton
                        variant={memberDetailView === "json" ? "default" : "outline"}
                        size="xs"
                        icon={Braces}
                        aria-pressed={memberDetailView === "json"}
                        onClick={() => setMemberDetailView("json")}
                      >
                        {t("database.redisJsonView")}
                      </DbxButton>
                      <DbxButton
                        variant={memberDetailView === "raw" ? "default" : "outline"}
                        size="xs"
                        icon={Terminal}
                        aria-pressed={memberDetailView === "raw"}
                        onClick={() => setMemberDetailView("raw")}
                      >
                        {t("database.redisRawContent")}
                      </DbxButton>
                    </span>
                    <span style={{ flex: 1 }} />
                    {memberDetailView === "raw" && selectedMember.editAction && !readOnly && (
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                        <DbxButton
                          variant="ghost"
                          size="icon-xs"
                          icon={Braces}
                          onClick={() => updateMemberJsonDraft(true)}
                          aria-label={t("database.redisFormatMemberJson")}
                          title={t("database.redisFormatMemberJson")}
                        />
                        <DbxButton
                          variant="ghost"
                          size="icon-xs"
                          icon={Minimize2}
                          onClick={() => updateMemberJsonDraft(false)}
                          aria-label={t("database.redisCompressMemberJson")}
                          title={t("database.redisCompressMemberJson")}
                        />
                      </span>
                    )}
                    <label
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 5,
                        color: "var(--text-muted)",
                        fontSize: 12,
                        whiteSpace: "nowrap",
                      }}
                    >
                      <WrapText size={13} />
                      <span>{t("database.redisWordWrap")}</span>
                      <input
                        type="checkbox"
                        checked={memberJsonWordWrap}
                        onChange={(event) => updateMemberJsonWordWrap(event.target.checked)}
                        aria-label={t("database.redisWordWrap")}
                      />
                    </label>
                  </div>
                )}
                {selectedMemberIsEditing ? (
                  <textarea
                    aria-label={t("database.redisMemberValue")}
                    style={{
                      flex: 1,
                      minHeight: 0,
                      width: "100%",
                      resize: "none",
                      border: "none",
                      outline: "none",
                      background: "transparent",
                      color: "var(--text-primary)",
                      fontFamily: "var(--font-mono)",
                      fontSize: 12,
                      lineHeight: 1.5,
                      padding: 10,
                    }}
                    value={memberEditValue}
                    disabled={savingMember}
                    onChange={(event) => setMemberEditValue(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") cancelMemberEdit();
                    }}
                    autoFocus
                  />
                ) : selectedMember.format === "json" &&
                  memberDetailView === "json" &&
                  selectedMemberJsonValue ? (
                  <div
                    style={{
                      flex: 1,
                      minHeight: 0,
                      margin: 0,
                      overflow: "auto",
                      color: "var(--text-primary)",
                      fontFamily: "var(--font-mono)",
                      fontSize: 12,
                      lineHeight: 1.5,
                      padding: 10,
                    }}
                  >
                    <RedisJsonTree
                      value={selectedMemberJsonValue.value}
                      wordWrap={memberJsonWordWrap}
                    />
                  </div>
                ) : (
                  <pre
                    style={{
                      flex: 1,
                      minHeight: 0,
                      margin: 0,
                      overflow: "auto",
                      whiteSpace:
                        selectedMember.format === "json"
                          ? memberJsonWordWrap
                            ? "pre-wrap"
                            : "pre"
                          : "pre-wrap",
                      wordBreak:
                        selectedMember.format === "json"
                          ? memberJsonWordWrap
                            ? "break-word"
                            : "normal"
                          : "break-word",
                      color: "var(--text-primary)",
                      fontFamily: "var(--font-mono)",
                      fontSize: 12,
                      lineHeight: 1.5,
                      padding: 10,
                    }}
                  >
                    {memberDetailText}
                  </pre>
                )}
              </div>
            )}
          </div>
        )}
        {showValueDraftEditor && selectedValueJson && (
          <div
            style={{
              minHeight: 36,
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "0 10px",
              border: "1px solid var(--border-dim)",
              borderRadius: 8,
              background: "var(--bg-panel)",
            }}
          >
            <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
              <DbxButton
                variant={valueDetailView === "json" ? "default" : "outline"}
                size="xs"
                icon={Braces}
                aria-pressed={valueDetailView === "json"}
                onClick={() => setValueDetailView("json")}
              >
                {t("database.redisJsonView")}
              </DbxButton>
              <DbxButton
                variant={valueDetailView === "raw" ? "default" : "outline"}
                size="xs"
                icon={Terminal}
                aria-pressed={valueDetailView === "raw"}
                onClick={() => setValueDetailView("raw")}
              >
                {t("database.redisRawContent")}
              </DbxButton>
            </span>
            <span style={{ flex: 1 }} />
            {valueDetailView === "raw" && (
              <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                <DbxButton
                  variant="ghost"
                  size="icon-xs"
                  icon={Braces}
                  disabled={readOnly || !redis.selectedValue || redis.selectedValue.value_is_binary}
                  onClick={() => updateJsonDraft(true)}
                  aria-label={t("database.redisFormatJson")}
                  title={t("database.redisFormatJson")}
                />
                <DbxButton
                  variant="ghost"
                  size="icon-xs"
                  icon={Minimize2}
                  disabled={readOnly || !redis.selectedValue || redis.selectedValue.value_is_binary}
                  onClick={() => updateJsonDraft(false)}
                  aria-label={t("database.redisCompressJson")}
                  title={t("database.redisCompressJson")}
                />
              </span>
            )}
            <label
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                color: "var(--text-muted)",
                fontSize: 12,
                whiteSpace: "nowrap",
              }}
            >
              <WrapText size={13} />
              <span>{t("database.redisWordWrap")}</span>
              <input
                type="checkbox"
                checked={memberJsonWordWrap}
                onChange={(event) => updateMemberJsonWordWrap(event.target.checked)}
                aria-label={t("database.redisWordWrap")}
              />
            </label>
          </div>
        )}
        {showValueDraftEditor &&
          (selectedValueJson && valueDetailView === "json" ? (
            <div
              style={{
                ...s.databaseLargeTextArea,
                overflow: "auto",
                resize: "none",
                fontFamily: "var(--font-mono)",
              }}
            >
              <RedisJsonTree value={selectedValueJson.value} wordWrap={memberJsonWordWrap} />
            </div>
          ) : (
            <textarea
              aria-label={t("database.redisValue")}
              style={{
                ...s.databaseLargeTextArea,
                whiteSpace: selectedValueJson && !memberJsonWordWrap ? "pre" : undefined,
              }}
              value={valueDraft}
              onChange={(event) => {
                setValueDraft(event.target.value);
                setValueFormatError("");
              }}
              readOnly={readOnly || !redis.selectedValue || selectedValueIsBinaryString}
            />
          ))}
        {showValueDraftEditor && selectedValueIsBinaryString && (
          <div style={{ color: "var(--text-muted)", fontSize: 12, padding: "0 2px" }}>
            {t("database.redisBinaryStringReadonlyHint")}
          </div>
        )}
        <RedisCommandSessionView
          commandDb={commandDb}
          commandText={commandText}
          commandHistory={commandHistory}
          commandRunning={commandRunning}
          onCommandTextChange={setCommandText}
          onRunCommand={runCommand}
        />
        <div style={s.databaseToolbar}>
          {showValueDraftEditor && (
            <>
              {!selectedValueJson && (
                <>
                  <DbxButton
                    variant="outline"
                    size="sm"
                    icon={Braces}
                    disabled={
                      readOnly || !redis.selectedValue || redis.selectedValue.value_is_binary
                    }
                    onClick={() => updateJsonDraft(true)}
                  >
                    {t("database.redisFormatJson")}
                  </DbxButton>
                  <DbxButton
                    variant="outline"
                    size="sm"
                    icon={Minimize2}
                    disabled={
                      readOnly || !redis.selectedValue || redis.selectedValue.value_is_binary
                    }
                    onClick={() => updateJsonDraft(false)}
                  >
                    {t("database.redisCompressJson")}
                  </DbxButton>
                </>
              )}
              <input
                aria-label={t("database.redisTtl")}
                style={{ ...s.databaseDialogInput, width: 120 }}
                value={ttlDraft}
                onChange={(event) => setTtlDraft(event.target.value)}
                placeholder="TTL"
              />
              {valueDraftDirty && !selectedValueIsBinaryString && (
                <DbxButton
                  variant="outline"
                  size="sm"
                  icon={X}
                  disabled={readOnly}
                  onClick={discardValueDraft}
                >
                  {t("database.discardValue")}
                </DbxButton>
              )}
              <DbxButton
                variant="default"
                size="sm"
                disabled={readOnly || !redis.selectedValue || selectedValueIsBinaryString}
                onClick={() => void saveValue()}
              >
                {t("database.saveValue")}
              </DbxButton>
            </>
          )}
          <DbxButton
            variant="outline"
            size="sm"
            onClick={clearCommandHistory}
            disabled={commandHistory.length === 0}
          >
            {t("database.redisClearHistory")}
          </DbxButton>
        </div>
        {valueFormatError && <div style={s.databaseError}>{valueFormatError}</div>}
        {ttlError && <div style={s.databaseError}>{ttlError}</div>}
        {redis.error && <div style={s.databaseError}>{redis.error}</div>}
      </div>
      {keyContextMenu && (
        <>
          <div style={s.fileCtxBackdrop} onClick={() => setKeyContextMenu(null)} />
          <div
            role="menu"
            style={{
              ...s.fileCtxMenu,
              left: keyContextMenu.x,
              top: keyContextMenu.y,
              minWidth: 190,
            }}
          >
            {(
              [
                ["copyName", "database.copyName"],
                ["refresh", "database.refresh"],
                ["deleteKey", "database.redisDeleteKey"],
              ] as const
            ).map(([action, labelKey]) => (
              <DbxMenuItem
                key={action}
                icon={action === "copyName" ? Copy : action === "refresh" ? RefreshCcw : Trash2}
                disabled={action === "deleteKey" && readOnly}
                destructive={action === "deleteKey"}
                onClick={() => {
                  const menu = keyContextMenu;
                  setKeyContextMenu(null);
                  if (action === "copyName") {
                    copyKeyName(menu.keyRaw);
                  } else if (action === "refresh") {
                    void refreshKey(menu.keyRaw);
                  } else {
                    void deleteKey(menu.keyRaw);
                  }
                }}
              >
                {t(labelKey)}
              </DbxMenuItem>
            ))}
          </div>
        </>
      )}
      {groupContextMenu && (
        <>
          <div style={s.fileCtxBackdrop} onClick={() => setGroupContextMenu(null)} />
          <div
            role="menu"
            style={{
              ...s.fileCtxMenu,
              left: groupContextMenu.x,
              top: groupContextMenu.y,
              minWidth: 210,
            }}
          >
            {(
              [
                ["copyName", "database.copyName"],
                ["refresh", "database.refresh"],
                ["deleteGroup", "database.redisDeleteKeyGroup"],
              ] as const
            ).map(([action, labelKey]) => (
              <DbxMenuItem
                key={action}
                icon={action === "copyName" ? Copy : action === "refresh" ? RefreshCcw : Trash2}
                disabled={
                  (action === "deleteGroup" && readOnly) ||
                  (action === "deleteGroup" && groupContextMenu.keyRaws.length === 0)
                }
                destructive={action === "deleteGroup"}
                onClick={() => {
                  const menu = groupContextMenu;
                  setGroupContextMenu(null);
                  if (action === "copyName") {
                    copyKeyName(menu.groupName);
                  } else if (action === "refresh") {
                    void refreshKeys();
                  } else {
                    void deleteKeyGroup(menu.groupName, menu.keyRaws);
                  }
                }}
              >
                {t(labelKey)}
              </DbxMenuItem>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
