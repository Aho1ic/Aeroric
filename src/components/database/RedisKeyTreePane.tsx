import type { MouseEvent } from "react";
import {
  Asterisk,
  ChevronDown,
  ChevronRight,
  Folder,
  KeyRound,
  RefreshCcw,
  Search,
  Trash2,
} from "lucide-react";
import { useI18n } from "../../i18n";
import {
  collectRedisGroupKeyRaws,
  countRedisTreeLeaves,
  type RedisKeyTreeRow,
} from "../../lib/redisKeyTree";
import s from "../../styles";
import type { RedisDatabaseInfo } from "../../types";
import { DbxButton } from "./DbxButton";

interface RedisKeyTreePaneProps {
  databases: RedisDatabaseInfo[];
  activeDb: number;
  pattern: string;
  fuzzyKeySearch: boolean;
  keyRows: RedisKeyTreeRow[];
  expandedKeyGroups: ReadonlySet<string>;
  selectedKeyRaws: ReadonlySet<string>;
  activeKeyRaw: string | null;
  keySeparator: string;
  keyCount: number;
  totalKeys: number;
  cursor: number;
  loading: boolean;
  readOnly: boolean;
  fetchingAllKeys: boolean;
  fetchAllLoadedKeys: number;
  fetchAllTotalKeys: number;
  onRefresh: () => void | Promise<unknown>;
  onSelectDatabase: (database: number) => void;
  onPatternChange: (value: string) => void;
  onSearch: (value: string) => void | Promise<unknown>;
  onToggleFuzzySearch: () => void;
  onDeleteSelectedKeys: () => void | Promise<unknown>;
  onToggleKeyGroup: (groupId: string) => void;
  onOpenGroupContextMenu: (event: MouseEvent, groupName: string, keyRaws: string[]) => void;
  onLoadKey: (keyRaw: string) => void | Promise<unknown>;
  onOpenKeyContextMenu: (event: MouseEvent, keyRaw: string) => void;
  onToggleSelectedKey: (keyRaw: string) => void;
  onLoadMoreKeys: () => void;
  onFetchAllKeys: () => void | Promise<unknown>;
  onStopFetchAllKeys: () => void;
}

export function RedisKeyTreePane({
  databases,
  activeDb,
  pattern,
  fuzzyKeySearch,
  keyRows,
  expandedKeyGroups,
  selectedKeyRaws,
  activeKeyRaw,
  keySeparator,
  keyCount,
  totalKeys,
  cursor,
  loading,
  readOnly,
  fetchingAllKeys,
  fetchAllLoadedKeys,
  fetchAllTotalKeys,
  onRefresh,
  onSelectDatabase,
  onPatternChange,
  onSearch,
  onToggleFuzzySearch,
  onDeleteSelectedKeys,
  onToggleKeyGroup,
  onOpenGroupContextMenu,
  onLoadKey,
  onOpenKeyContextMenu,
  onToggleSelectedKey,
  onLoadMoreKeys,
  onFetchAllKeys,
  onStopFetchAllKeys,
}: RedisKeyTreePaneProps) {
  const { t } = useI18n();
  return (
    <div style={s.databaseBrowserSidebar}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>Redis</div>
          <div style={s.databaseDialogHint}>{t("database.redisBrowserHint")}</div>
        </div>
        <DbxButton
          variant="ghost"
          size="icon-sm"
          icon={RefreshCcw}
          onClick={() => void onRefresh()}
          aria-label={t("database.refresh")}
        />
      </div>
      <div style={s.databaseButtonRow}>
        {databases.map((database) => (
          <DbxButton
            key={database.db}
            variant={activeDb === database.db ? "default" : "outline"}
            size="xs"
            onClick={() => onSelectDatabase(database.db)}
          >
            db{database.db}
          </DbxButton>
        ))}
      </div>
      <label style={s.databaseSearchBox}>
        <Search size={14} />
        <input
          aria-label={t("database.redisKeyPattern")}
          style={s.databaseSearchInput}
          value={pattern}
          onChange={(event) => onPatternChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void onSearch(event.currentTarget.value);
          }}
          placeholder="*"
        />
        <DbxButton
          variant={fuzzyKeySearch ? "default" : "outline"}
          size="xs"
          icon={Asterisk}
          aria-pressed={fuzzyKeySearch}
          title={t("database.redisFuzzyMatchTitle")}
          onClick={onToggleFuzzySearch}
        >
          {t("database.redisFuzzyMatch")}
        </DbxButton>
      </label>
      {selectedKeyRaws.size > 0 && (
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={s.databaseDialogHint}>
            {t("database.redisSelectedKeys", { count: selectedKeyRaws.size })}
          </span>
          <DbxButton
            variant="destructive"
            size="sm"
            icon={Trash2}
            disabled={readOnly}
            onClick={() => void onDeleteSelectedKeys()}
          >
            <span>
              {t("database.redisDeleteSelectedKeys")} ({selectedKeyRaws.size})
            </span>
          </DbxButton>
        </div>
      )}
      <div style={s.databaseScroll}>
        {keyRows.map(({ node, depth }) => {
          const paddingLeft = 8 + depth * 16;
          if (node.kind === "group") {
            const expanded = expandedKeyGroups.has(node.id);
            return (
              <button
                key={node.id}
                type="button"
                style={{ ...s.databaseListButton, paddingLeft }}
                onClick={() => onToggleKeyGroup(node.id)}
                onContextMenu={(event) =>
                  onOpenGroupContextMenu(
                    event,
                    node.pathSegments.join(keySeparator),
                    collectRedisGroupKeyRaws(node),
                  )
                }
                aria-expanded={expanded}
              >
                {expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                <Folder size={13} />
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    textAlign: "left",
                  }}
                >
                  {node.label}
                </span>
                <span style={s.databasePill}>{countRedisTreeLeaves(node)}</span>
              </button>
            );
          }
          const active = activeKeyRaw === node.keyRaw;
          const selected = selectedKeyRaws.has(node.keyRaw);
          return (
            <button
              key={node.id}
              type="button"
              style={{
                ...s.databaseListButton,
                ...(active ? s.databaseListButtonActive : {}),
                paddingLeft,
              }}
              onClick={() => void onLoadKey(node.keyRaw)}
              onContextMenu={(event) => onOpenKeyContextMenu(event, node.keyRaw)}
              aria-label={`${node.fullKeyDisplay} ${node.keyType}`}
              title={node.fullKeyDisplay}
            >
              <input
                type="checkbox"
                aria-label={`${t("database.redisDeleteSelectedKeys")}: ${node.fullKeyDisplay}`}
                checked={selected}
                disabled={readOnly}
                onChange={() => undefined}
                onClick={(event) => {
                  event.stopPropagation();
                  onToggleSelectedKey(node.keyRaw);
                }}
                style={{ width: 14, height: 14, flexShrink: 0 }}
              />
              <KeyRound size={13} />
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  textAlign: "left",
                }}
              >
                {node.label}
              </span>
              <span style={s.databasePill}>{node.keyType}</span>
            </button>
          );
        })}
        {keyCount === 0 && (
          <div style={s.databaseEmptyCompact}>
            {loading ? t("database.loading") : t("database.empty")}
          </div>
        )}
        {keyCount > 0 &&
          cursor !== 0 &&
          (fetchingAllKeys ? (
            <div style={{ display: "grid", gap: 6 }}>
              <div style={{ ...s.databaseDialogHint, textAlign: "center" }}>
                {fetchAllTotalKeys > 0
                  ? t("database.redisFetchAllProgress", {
                      loaded: fetchAllLoadedKeys,
                      total: fetchAllTotalKeys,
                    })
                  : t("database.redisFetchAllProgressUnknown", { loaded: fetchAllLoadedKeys })}
              </div>
              <button
                type="button"
                style={{ ...s.databaseListButton, color: "var(--danger)" }}
                onClick={onStopFetchAllKeys}
              >
                {t("database.redisStopFetchAll")}
              </button>
            </div>
          ) : (
            <div style={{ display: "flex", gap: 6 }}>
              <button
                type="button"
                style={{ ...s.databaseListButton, flex: 1 }}
                onClick={onLoadMoreKeys}
                disabled={loading}
              >
                {t("database.loadMore")} ({keyCount}/{totalKeys || keyCount})
              </button>
              <button
                type="button"
                style={{ ...s.databaseListButton, flex: 1 }}
                onClick={() => void onFetchAllKeys()}
                disabled={loading}
              >
                {t("database.redisFetchAllKeys")}
              </button>
            </div>
          ))}
      </div>
    </div>
  );
}
