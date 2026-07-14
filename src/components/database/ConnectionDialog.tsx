import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowDown,
  ArrowUp,
  ChevronRight,
  Copy,
  Database,
  FilePlus,
  Plug,
  Plus,
  Search,
  Server,
  Shield,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import type { AeroricDbConnectionConfig, DbEndpoint, DbxDatabaseType } from "../../types";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import { DbxButton, DbxButtonGroup, DbxSegmentedButton } from "./DbxButton";
import { PasswordInput } from "./DatabaseViewPrimitives";
import { parseConnectionUrl } from "./databaseConnectionUrl";
import s from "../../styles";
import {
  DB_PROFILES,
  buildDbxConnectionConfig,
  createTransportLayerDraft,
  dbxBoolean,
  dbxConfigRecord,
  dbxNumberString,
  dbxString,
  dbxStringList,
  profileForDbxConnection,
  transportLayerDraftFromPayload,
  type ConnectionDraft,
  type DbConfigTab,
  type DbProfile,
  type DbProfileKey,
  type DbProfileViewMode,
  type DbWizardStep,
  type RedisConnectionMode,
  type TransportLayerDraft,
} from "./databaseConnectionDraft";

const CONNECTION_COLOR_OPTIONS = [
  { value: "", labelKey: "database.colorNone" },
  { value: "#22c55e", labelKey: "database.colorGreen" },
  { value: "#eab308", labelKey: "database.colorYellow" },
  { value: "#f97316", labelKey: "database.colorOrange" },
  { value: "#ef4444", labelKey: "database.colorRed" },
  { value: "#3b82f6", labelKey: "database.colorBlue" },
  { value: "#a855f7", labelKey: "database.colorPurple" },
] as const;

function DatabaseProfileIcon({ profile, size = 23 }: { profile: DbProfile; size?: number }) {
  return (
    <span
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        borderRadius: profile.key === "redis" ? 5 : profile.key === "mongodb" ? 999 : 7,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: `${profile.accent}22`,
        border: `1px solid ${profile.accent}66`,
        color: profile.accent,
        fontSize: Math.max(9, size * 0.42),
        fontWeight: 800,
        fontFamily: "var(--font-ui)",
        lineHeight: 1,
      }}
    >
      {profile.iconText}
    </span>
  );
}

interface ConnectionDialogProps {
  open: boolean;
  editingConnection: AeroricDbConnectionConfig | null;
  initialConnectionGroup?: string | null;
  projectRoot?: string;
  onAddLocalConnection: (endpoint: DbEndpoint) => void;
  onSaved: (
    connections: AeroricDbConnectionConfig[],
    saved: AeroricDbConnectionConfig,
  ) => void | Promise<void>;
  onClose: () => void;
}

export function ConnectionDialog({
  open,
  editingConnection,
  initialConnectionGroup,
  projectRoot,
  onAddLocalConnection,
  onSaved,
  onClose,
}: ConnectionDialogProps) {
  const { t } = useI18n();

  const [loading, setLoading] = useState(false);
  const [, setError] = useState<string | null>(null);
  const [connectionTestResult, setConnectionTestResult] = useState<{
    success: boolean;
    message: string;
  } | null>(null);
  const [wizardStep, setWizardStep] = useState<DbWizardStep>("type");
  const [selectedProfileKey, setSelectedProfileKey] = useState<DbProfileKey>("sqlite");
  const [profileViewMode, setProfileViewMode] = useState<DbProfileViewMode>("icon");
  const [profileSearch, setProfileSearch] = useState("");
  const [configTab, setConfigTab] = useState<DbConfigTab>("connection");
  const [draftName, setDraftName] = useState("SQLite");
  const [draftConnectionGroup, setDraftConnectionGroup] = useState("");
  const [draftColor, setDraftColor] = useState("");
  const [draftHost, setDraftHost] = useState("127.0.0.1");
  const [draftPort, setDraftPort] = useState("0");
  const [draftUser, setDraftUser] = useState("");
  const [draftDatabase, setDraftDatabase] = useState("");
  const [draftPassword, setDraftPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [showSentinelPassword, setShowSentinelPassword] = useState(false);
  const [showTransportPassword, setShowTransportPassword] = useState(false);
  const [showTransportPassphrase, setShowTransportPassphrase] = useState(false);
  const [draftFilePath, setDraftFilePath] = useState("");
  const [draftReadOnly, setDraftReadOnly] = useState(false);
  const [draftInitScript, setDraftInitScript] = useState("");
  const [draftAgentJavaOptions, setDraftAgentJavaOptions] = useState("");
  const [draftProductionProtectionEnabled, setDraftProductionProtectionEnabled] = useState(false);
  const [draftProductionScope, setDraftProductionScope] = useState<"connection" | "databases">(
    "connection",
  );
  const [draftProductionDatabases, setDraftProductionDatabases] = useState("");
  const [draftUrlParams, setDraftUrlParams] = useState("");
  const [draftConnectionString, setDraftConnectionString] = useState("");
  const [draftConnectTimeoutSecs, setDraftConnectTimeoutSecs] = useState("5");
  const [draftQueryTimeoutSecs, setDraftQueryTimeoutSecs] = useState("30");
  const [draftIdleTimeoutSecs, setDraftIdleTimeoutSecs] = useState("60");
  const [draftKeepaliveIntervalSecs, setDraftKeepaliveIntervalSecs] = useState("0");
  const [draftCaCertPath, setDraftCaCertPath] = useState("");
  const [draftClientCertPath, setDraftClientCertPath] = useState("");
  const [draftClientKeyPath, setDraftClientKeyPath] = useState("");
  const [draftRedisConnectionMode, setDraftRedisConnectionMode] =
    useState<RedisConnectionMode>("standalone");
  const [draftRedisSentinelMaster, setDraftRedisSentinelMaster] = useState("");
  const [draftRedisSentinelNodes, setDraftRedisSentinelNodes] = useState("");
  const [draftRedisSentinelUsername, setDraftRedisSentinelUsername] = useState("");
  const [draftRedisSentinelPassword, setDraftRedisSentinelPassword] = useState("");
  const [draftRedisSentinelTls, setDraftRedisSentinelTls] = useState(false);
  const [draftRedisClusterNodes, setDraftRedisClusterNodes] = useState("");
  const [draftRedisKeySeparator, setDraftRedisKeySeparator] = useState(":");
  const [draftMongoUseUrl, setDraftMongoUseUrl] = useState(false);
  const [tlsEnabled, setTlsEnabled] = useState(false);
  const [draftTlsMode, setDraftTlsMode] = useState("");
  const [draftOracleConnectionType, setDraftOracleConnectionType] = useState<
    "service_name" | "sid"
  >("service_name");
  const [draftOracleSysdba, setDraftOracleSysdba] = useState(false);
  const [transportEnabled, setTransportEnabled] = useState(false);
  const [draftTransportLayers, setDraftTransportLayers] = useState<TransportLayerDraft[]>([]);
  const [selectedTransportLayerId, setSelectedTransportLayerId] = useState<string | null>(null);

  const selectedProfile =
    DB_PROFILES.find((profile) => profile.key === selectedProfileKey) ?? DB_PROFILES[0];
  const filteredProfiles = useMemo(() => {
    const query = profileSearch.trim().toLowerCase();
    if (!query) return DB_PROFILES;
    return DB_PROFILES.filter(
      (profile) => profile.label.toLowerCase().includes(query) || profile.key.includes(query),
    );
  }, [profileSearch]);
  const selectedTransportLayer = useMemo(
    () =>
      draftTransportLayers.find((layer) => layer.id === selectedTransportLayerId) ??
      draftTransportLayers[0] ??
      null,
    [draftTransportLayers, selectedTransportLayerId],
  );

  const chooseLocalDbFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected === "string") setDraftFilePath(selected);
  }, [projectRoot]);

  const chooseTlsCertificatePath = useCallback(
    async (setter: (path: string) => void) => {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        filters: [
          { name: "Certificates", extensions: ["pem", "crt", "cer", "key", "p12", "pfx"] },
          { name: "All files", extensions: ["*"] },
        ],
        defaultPath: projectRoot,
      });
      if (typeof selected === "string") setter(selected);
    },
    [projectRoot],
  );

  const resetConnectionDraft = useCallback(
    (profile: DbProfile = selectedProfile) => {
      setWizardStep("type");
      setConfigTab("connection");
      setDraftName(profile.label);
      setDraftConnectionGroup("");
      setDraftColor("");
      setDraftHost(profile.localFile ? "" : "127.0.0.1");
      setDraftPort(String(profile.port));
      setDraftUser(profile.user);
      setDraftDatabase("");
      setDraftPassword("");
      setShowPassword(false);
      setShowSentinelPassword(false);
      setShowTransportPassword(false);
      setShowTransportPassphrase(false);
      setDraftFilePath("");
      setDraftReadOnly(false);
      setDraftInitScript("");
      setDraftAgentJavaOptions("");
      setDraftProductionProtectionEnabled(false);
      setDraftProductionScope("connection");
      setDraftProductionDatabases("");
      setDraftUrlParams("");
      setDraftConnectionString("");
      setDraftConnectTimeoutSecs("5");
      setDraftQueryTimeoutSecs("30");
      setDraftIdleTimeoutSecs("60");
      setDraftKeepaliveIntervalSecs("0");
      setDraftCaCertPath("");
      setDraftClientCertPath("");
      setDraftClientKeyPath("");
      setDraftRedisConnectionMode("standalone");
      setDraftRedisSentinelMaster("");
      setDraftRedisSentinelNodes("");
      setDraftRedisSentinelUsername("");
      setDraftRedisSentinelPassword("");
      setDraftRedisSentinelTls(false);
      setDraftRedisClusterNodes("");
      setDraftRedisKeySeparator(":");
      setDraftMongoUseUrl(false);
      setTlsEnabled(false);
      setTransportEnabled(false);
      setDraftTransportLayers([]);
      setSelectedTransportLayerId(null);
    },
    [selectedProfile],
  );

  const applyEditConnection = useCallback((connection: AeroricDbConnectionConfig) => {
    const profile = profileForDbxConnection(connection);
    const config = dbxConfigRecord(connection);
    const transportLayers = Array.isArray(config.transport_layers)
      ? config.transport_layers.map((layer, index) =>
          transportLayerDraftFromPayload(layer, index + 1),
        )
      : [];
    const productionDatabases = dbxStringList(config, "production_databases");
    const isProduction = dbxBoolean(config, "is_production");
    setSelectedProfileKey(profile.key);
    setWizardStep("config");
    setConfigTab("connection");
    setDraftName(dbxString(config, "name", connection.name));
    setDraftConnectionGroup(connection.connectionGroup ?? "");
    setDraftColor(dbxString(config, "color"));
    setDraftHost(profile.localFile ? "" : dbxString(config, "host", "127.0.0.1"));
    setDraftPort(dbxNumberString(config, "port", String(profile.port)));
    setDraftUser(dbxString(config, "username"));
    setDraftDatabase(dbxString(config, "database"));
    setDraftPassword(dbxString(config, "password"));
    setDraftFilePath(profile.localFile ? dbxString(config, "host") : "");
    setDraftReadOnly(dbxBoolean(config, "read_only", connection.readOnly));
    setDraftInitScript(dbxString(config, "init_script"));
    setDraftAgentJavaOptions(dbxStringList(config, "agent_java_options").join("\n"));
    setDraftProductionProtectionEnabled(isProduction || productionDatabases.length > 0);
    setDraftProductionScope(
      isProduction || profile.localFile || productionDatabases.length === 0
        ? "connection"
        : "databases",
    );
    setDraftProductionDatabases(productionDatabases.join("\n"));
    setDraftUrlParams(dbxString(config, "url_params"));
    setDraftConnectionString(dbxString(config, "connection_string"));
    setDraftConnectTimeoutSecs(dbxNumberString(config, "connect_timeout_secs", "5"));
    setDraftQueryTimeoutSecs(dbxNumberString(config, "query_timeout_secs", "30"));
    setDraftIdleTimeoutSecs(dbxNumberString(config, "idle_timeout_secs", "60"));
    setDraftKeepaliveIntervalSecs(dbxNumberString(config, "keepalive_interval_secs", "0"));
    setDraftCaCertPath(dbxString(config, "ca_cert_path"));
    setDraftClientCertPath(dbxString(config, "client_cert_path"));
    setDraftClientKeyPath(dbxString(config, "client_key_path"));
    setDraftRedisConnectionMode(
      config.redis_connection_mode === "sentinel" || config.redis_connection_mode === "cluster"
        ? config.redis_connection_mode
        : "standalone",
    );
    setDraftRedisSentinelMaster(dbxString(config, "redis_sentinel_master"));
    setDraftRedisSentinelNodes(dbxString(config, "redis_sentinel_nodes"));
    setDraftRedisSentinelUsername(dbxString(config, "redis_sentinel_username"));
    setDraftRedisSentinelPassword(dbxString(config, "redis_sentinel_password"));
    setDraftRedisSentinelTls(dbxBoolean(config, "redis_sentinel_tls"));
    setDraftRedisClusterNodes(dbxString(config, "redis_cluster_nodes"));
    setDraftRedisKeySeparator(dbxString(config, "redis_key_separator", ":"));
    setDraftMongoUseUrl(
      connection.dbType === "mongodb" && Boolean(dbxString(config, "connection_string")),
    );
    setTlsEnabled(dbxBoolean(config, "ssl"));
    const urlParamsStr = dbxString(config, "url_params");
    if (connection.dbType === "postgres") {
      const params = new URLSearchParams(urlParamsStr);
      setDraftTlsMode(params.get("sslmode") || "");
    } else if (connection.dbType === "mysql") {
      const params = new URLSearchParams(urlParamsStr);
      setDraftTlsMode(params.get("ssl-mode") || "");
    } else {
      setDraftTlsMode("");
    }
    if (connection.dbType === "oracle") {
      const oracleConfig = config as {
        oracle_connection_type?: string;
        sysdba?: boolean;
        oracle_sysdba?: boolean;
      };
      setDraftOracleConnectionType(
        oracleConfig.oracle_connection_type === "sid" ? "sid" : "service_name",
      );
      setDraftOracleSysdba(
        typeof oracleConfig.sysdba === "boolean"
          ? oracleConfig.sysdba
          : Boolean(oracleConfig.oracle_sysdba),
      );
    } else {
      setDraftOracleConnectionType("service_name");
      setDraftOracleSysdba(false);
    }
    setTransportEnabled(transportLayers.length > 0);
    setDraftTransportLayers(transportLayers);
    setSelectedTransportLayerId(transportLayers[0]?.id ?? null);
    setProfileSearch("");
    setProfileViewMode("icon");
    setError(null);
  }, []);

  const wasOpenRef = useRef(false);
  useEffect(() => {
    if (open && !wasOpenRef.current) {
      if (editingConnection === null) {
        resetConnectionDraft(selectedProfile);
        setDraftConnectionGroup(
          typeof initialConnectionGroup === "string" ? initialConnectionGroup.trim() : "",
        );
        setProfileSearch("");
        setProfileViewMode("icon");
        setError(null);
      } else {
        applyEditConnection(editingConnection);
      }
    }
    wasOpenRef.current = open;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, editingConnection]);

  const handleProfileSelect = useCallback(
    (key: DbProfileKey) => {
      const profile = DB_PROFILES.find((item) => item.key === key) ?? DB_PROFILES[0];
      setSelectedProfileKey(key);
      resetConnectionDraft(profile);
    },
    [resetConnectionDraft],
  );

  const handleProfileDoubleClick = useCallback(
    (key: DbProfileKey) => {
      handleProfileSelect(key);
      setWizardStep("config");
    },
    [handleProfileSelect],
  );

  const applyConnectionUrl = useCallback(() => {
    const parsed = parseConnectionUrl(
      draftConnectionString,
      selectedProfile.key as DbxDatabaseType,
    );
    if (!parsed || !parsed.host) {
      setError(t("database.connectionUrlParseFailed"));
      return;
    }
    setDraftHost(parsed.host);
    if (parsed.port) setDraftPort(parsed.port);
    if (parsed.user !== undefined) setDraftUser(parsed.user);
    if (parsed.password !== undefined) setDraftPassword(parsed.password);
    if (parsed.database !== undefined) setDraftDatabase(parsed.database);
    if (parsed.urlParams !== undefined) setDraftUrlParams(parsed.urlParams);
    if (selectedProfile.key === "mongodb") setDraftMongoUseUrl(true);
    setError(null);
  }, [draftConnectionString, selectedProfile.key, t]);

  const addTransportLayer = useCallback((type: "ssh" | "proxy") => {
    setTransportEnabled(true);
    setDraftTransportLayers((current) => {
      const nextLayer = createTransportLayerDraft(type, current.length + 1);
      setSelectedTransportLayerId(nextLayer.id);
      return [...current, nextLayer];
    });
  }, []);

  const updateTransportLayer = useCallback((id: string, patch: Partial<TransportLayerDraft>) => {
    setDraftTransportLayers((current) =>
      current.map((layer) => (layer.id === id ? { ...layer, ...patch } : layer)),
    );
  }, []);

  const copyTransportLayer = useCallback((id: string) => {
    setTransportEnabled(true);
    setDraftTransportLayers((current) => {
      const index = current.findIndex((layer) => layer.id === id);
      if (index < 0) return current;
      const source = current[index];
      const copy = {
        ...source,
        id: `transport:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      };
      setSelectedTransportLayerId(copy.id);
      return [...current.slice(0, index + 1), copy, ...current.slice(index + 1)];
    });
  }, []);

  const moveTransportLayer = useCallback((id: string, direction: -1 | 1) => {
    setDraftTransportLayers((current) => {
      const index = current.findIndex((layer) => layer.id === id);
      const targetIndex = index + direction;
      if (index < 0 || targetIndex < 0 || targetIndex >= current.length) return current;
      const next = [...current];
      [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
      setSelectedTransportLayerId(id);
      return next;
    });
  }, []);

  const removeTransportLayer = useCallback((id: string) => {
    setDraftTransportLayers((current) => {
      const next = current.filter((layer) => layer.id !== id);
      setSelectedTransportLayerId(next[0]?.id ?? null);
      if (next.length === 0) setTransportEnabled(false);
      return next;
    });
  }, []);

  const buildDbxConnectionDraft = useCallback((): AeroricDbConnectionConfig => {
    const draft: ConnectionDraft = {
      profile: selectedProfile,
      name: draftName,
      connectionGroup: draftConnectionGroup,
      color: draftColor,
      host: draftHost,
      port: draftPort,
      user: draftUser,
      database: draftDatabase,
      password: draftPassword,
      filePath: draftFilePath,
      readOnly: draftReadOnly,
      initScript: draftInitScript,
      agentJavaOptions: draftAgentJavaOptions,
      isProduction:
        draftProductionProtectionEnabled &&
        (draftProductionScope === "connection" || Boolean(selectedProfile.localFile)),
      productionDatabases:
        draftProductionProtectionEnabled &&
        draftProductionScope === "databases" &&
        !selectedProfile.localFile
          ? draftProductionDatabases
          : "",
      urlParams: draftUrlParams,
      connectionString: draftConnectionString,
      connectTimeoutSecs: draftConnectTimeoutSecs,
      queryTimeoutSecs: draftQueryTimeoutSecs,
      idleTimeoutSecs: draftIdleTimeoutSecs,
      keepaliveIntervalSecs: draftKeepaliveIntervalSecs,
      caCertPath: draftCaCertPath,
      clientCertPath: draftClientCertPath,
      clientKeyPath: draftClientKeyPath,
      redisConnectionMode: draftRedisConnectionMode,
      redisSentinelMaster: draftRedisSentinelMaster,
      redisSentinelNodes: draftRedisSentinelNodes,
      redisSentinelUsername: draftRedisSentinelUsername,
      redisSentinelPassword: draftRedisSentinelPassword,
      redisSentinelTls: draftRedisSentinelTls,
      redisClusterNodes: draftRedisClusterNodes,
      redisKeySeparator: draftRedisKeySeparator,
      mongoUseUrl: draftMongoUseUrl,
      tlsEnabled,
      tlsMode: draftTlsMode,
      oracleConnectionType: draftOracleConnectionType,
      oracleSysdba: draftOracleSysdba,
      transportEnabled,
      transportLayers: draftTransportLayers,
    };
    return buildDbxConnectionConfig(draft, {
      editingConnection,
      projectRoot: projectRoot ?? null,
      t,
    });
  }, [
    editingConnection,
    draftConnectionGroup,
    draftDatabase,
    draftCaCertPath,
    draftClientCertPath,
    draftClientKeyPath,
    draftColor,
    draftConnectTimeoutSecs,
    draftConnectionString,
    draftFilePath,
    draftHost,
    draftIdleTimeoutSecs,
    draftInitScript,
    draftKeepaliveIntervalSecs,
    draftName,
    draftAgentJavaOptions,
    draftPassword,
    draftPort,
    draftProductionDatabases,
    draftProductionProtectionEnabled,
    draftProductionScope,
    draftQueryTimeoutSecs,
    draftReadOnly,
    draftTransportLayers,
    draftMongoUseUrl,
    draftRedisClusterNodes,
    draftRedisConnectionMode,
    draftRedisKeySeparator,
    draftRedisSentinelMaster,
    draftRedisSentinelNodes,
    draftRedisSentinelPassword,
    draftRedisSentinelTls,
    draftRedisSentinelUsername,
    draftTlsMode,
    draftOracleConnectionType,
    draftOracleSysdba,
    draftUrlParams,
    draftUser,
    projectRoot,
    selectedProfile,
    t,
    tlsEnabled,
    transportEnabled,
  ]);

  const submitConnection = useCallback(async () => {
    if (editingConnection || selectedProfile.key !== "sqlite") {
      setLoading(true);
      setError(null);
      try {
        const connection = buildDbxConnectionDraft();
        await databaseApi.dbxSaveConnection(connection);
        const next = await databaseApi.dbxListConnections();
        await onSaved(next, connection);
        onClose();
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
      return;
    }
    const path = draftFilePath.trim();
    if (!path) return;
    onAddLocalConnection({ kind: "local", path });
    onClose();
  }, [
    buildDbxConnectionDraft,
    draftFilePath,
    editingConnection,
    onAddLocalConnection,
    onClose,
    onSaved,
    selectedProfile.key,
  ]);

  const testDbxConnectionDraft = useCallback(async () => {
    setLoading(true);
    setConnectionTestResult(null);
    try {
      await databaseApi.dbxTestConnection(buildDbxConnectionDraft());
      setConnectionTestResult({ success: true, message: t("database.connectionTestOk") });
    } catch (err) {
      setConnectionTestResult({ success: false, message: String(err) });
    } finally {
      setLoading(false);
    }
  }, [buildDbxConnectionDraft, t]);

  const connectionDialogTitle = editingConnection
    ? t("database.editConnection")
    : t("database.newConnection");

  if (!open) return null;

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-label={connectionDialogTitle}
        style={s.databaseConnectionDialog}
        onSubmit={(event) => {
          event.preventDefault();
          if (wizardStep === "type") {
            setWizardStep("config");
          } else {
            submitConnection();
          }
        }}
      >
        <div style={s.databaseDialogHeader}>{connectionDialogTitle}</div>
        <div style={s.databaseDialogBody}>
          {wizardStep === "type" ? (
            <>
              <div style={s.databaseTypeChooserHeader}>
                <div style={s.databaseDialogIntro}>{t("database.chooseDatabaseType")}</div>
                <DbxButtonGroup
                  style={s.databaseTypeViewToggle}
                  aria-label={t("database.typeViewMode")}
                >
                  {(["icon", "list"] as const).map((mode) => (
                    <DbxSegmentedButton
                      key={mode}
                      active={profileViewMode === mode}
                      onClick={() => setProfileViewMode(mode)}
                    >
                      {t(mode === "icon" ? "database.typeIconView" : "database.typeListView")}
                    </DbxSegmentedButton>
                  ))}
                </DbxButtonGroup>
              </div>
              <label style={s.databaseSearchBox}>
                <Search size={13} />
                <input
                  aria-label={t("database.typeSearch")}
                  style={s.databaseSearchInput}
                  value={profileSearch}
                  placeholder={t("database.typeSearch")}
                  onChange={(event) => setProfileSearch(event.target.value)}
                />
              </label>
              {profileViewMode === "icon" ? (
                <div style={s.databaseTypeGrid}>
                  {filteredProfiles.map((profile) => (
                    <button
                      key={profile.key}
                      type="button"
                      style={{
                        ...s.databaseTypeCard,
                        ...(selectedProfileKey === profile.key ? s.databaseTypeCardSelected : {}),
                      }}
                      onClick={() => handleProfileSelect(profile.key)}
                      onDoubleClick={() => handleProfileDoubleClick(profile.key)}
                    >
                      <span
                        style={{
                          ...s.databaseTypeIcon,
                          background: `${profile.accent}1f`,
                          color: profile.accent,
                        }}
                      >
                        <DatabaseProfileIcon profile={profile} size={25} />
                      </span>
                      <span style={s.databaseTypeLabel}>{profile.label}</span>
                    </button>
                  ))}
                </div>
              ) : (
                <div style={s.databaseTypeList}>
                  {filteredProfiles.map((profile) => (
                    <button
                      key={profile.key}
                      type="button"
                      style={{
                        ...s.databaseTypeListItem,
                        ...(selectedProfileKey === profile.key ? s.databaseTypeCardSelected : {}),
                      }}
                      onClick={() => handleProfileSelect(profile.key)}
                      onDoubleClick={() => handleProfileDoubleClick(profile.key)}
                    >
                      <span
                        style={{
                          ...s.databaseTypeIconSmall,
                          background: `${profile.accent}1f`,
                          color: profile.accent,
                        }}
                      >
                        <DatabaseProfileIcon profile={profile} size={16} />
                      </span>
                      <span style={s.databaseTypeLabel}>{profile.label}</span>
                      <span style={s.databaseTypeMeta}>
                        {profile.localFile
                          ? t("database.filePath")
                          : `${t("database.port")} ${profile.port}`}
                      </span>
                    </button>
                  ))}
                </div>
              )}
              {filteredProfiles.length === 0 && (
                <div style={s.databaseEmpty}>{t("database.typeSearchEmpty")}</div>
              )}
            </>
          ) : (
            <>
              <div style={s.databaseConfigHeader}>
                <button
                  type="button"
                  style={s.databaseTypeMiniCard}
                  onClick={() => setWizardStep("type")}
                >
                  <span
                    style={{
                      ...s.databaseTypeIconSmall,
                      background: `${selectedProfile.accent}1f`,
                      color: selectedProfile.accent,
                    }}
                  >
                    <DatabaseProfileIcon profile={selectedProfile} size={16} />
                  </span>
                  <span>{selectedProfile.label}</span>
                </button>
              </div>
              <div style={s.databaseConfigTabs}>
                {[
                  ["connection", t("database.connectionInfo"), Database],
                  ["tls", t("database.tlsSsl"), Shield],
                  ["transport", t("database.sshTunnelProxy"), Server],
                  ["advanced", t("database.advanced"), SlidersHorizontal],
                ].map(([key, label, Icon]) => (
                  <button
                    key={key as string}
                    type="button"
                    style={{
                      ...s.databaseConfigTab,
                      ...(configTab === key ? s.databaseConfigTabActive : {}),
                    }}
                    onClick={() => setConfigTab(key as DbConfigTab)}
                  >
                    <Icon size={13} />
                    <span>{label as string}</span>
                  </button>
                ))}
              </div>
              {configTab === "connection" ? (
                <div style={s.databaseDialogFormGrid}>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.connectionName")}</span>
                    <input
                      style={s.databaseDialogInput}
                      value={draftName}
                      onChange={(event) => setDraftName(event.target.value)}
                      autoFocus
                    />
                  </label>
                  <div style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.connectionColor")}</span>
                    <div style={s.databaseColorPickerRow}>
                      {CONNECTION_COLOR_OPTIONS.map((color) => {
                        const selected = draftColor === color.value;
                        return (
                          <button
                            key={color.value || "none"}
                            type="button"
                            aria-label={t(color.labelKey)}
                            title={t(color.labelKey)}
                            style={{
                              ...s.databaseColorSwatch,
                              ...(color.value
                                ? { background: color.value }
                                : s.databaseColorSwatchEmpty),
                              ...(selected ? s.databaseColorSwatchSelected : {}),
                            }}
                            onClick={() => setDraftColor(color.value)}
                          />
                        );
                      })}
                      <label style={s.databaseColorCustom}>
                        <span>{t("database.colorCustom")}</span>
                        <input
                          aria-label={t("database.colorCustom")}
                          type="color"
                          value={draftColor || "#3b82f6"}
                          onChange={(event) => setDraftColor(event.target.value)}
                        />
                      </label>
                    </div>
                  </div>
                  {selectedProfile.localFile ? (
                    <label style={s.databaseDialogField}>
                      <span style={s.databaseDialogLabel}>{t("database.filePath")}</span>
                      <div style={s.databaseInputButtonRow}>
                        <input
                          style={s.databaseDialogInput}
                          value={draftFilePath}
                          onChange={(event) => setDraftFilePath(event.target.value)}
                          placeholder="/path/to/database.db"
                        />
                        <DbxButton
                          variant="ghost"
                          size="icon-sm"
                          icon={FilePlus}
                          onClick={chooseLocalDbFile}
                        />
                      </div>
                    </label>
                  ) : (
                    <>
                      {selectedProfile.key === "mongodb" && (
                        <div style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>
                            {t("database.mongoConnectionMode")}
                          </span>
                          <DbxButtonGroup
                            style={s.databaseTypeViewToggle}
                            aria-label={t("database.mongoConnectionMode")}
                          >
                            {(
                              [
                                [false, t("database.mongoModeForm")],
                                [true, "URL"],
                              ] as const
                            ).map(([useUrl, label]) => (
                              <DbxSegmentedButton
                                key={String(useUrl)}
                                active={draftMongoUseUrl === useUrl}
                                onClick={() => setDraftMongoUseUrl(useUrl)}
                              >
                                {label}
                              </DbxSegmentedButton>
                            ))}
                          </DbxButtonGroup>
                        </div>
                      )}
                      {!(selectedProfile.key === "mongodb" && draftMongoUseUrl) && (
                        <>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.host")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftHost}
                              onChange={(event) => setDraftHost(event.target.value)}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.port")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftPort}
                              onChange={(event) => setDraftPort(event.target.value)}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.user")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftUser}
                              onChange={(event) => setDraftUser(event.target.value)}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.password")}</span>
                            <PasswordInput
                              style={s.databaseDialogInput}
                              value={draftPassword}
                              onChange={(event) => setDraftPassword(event.target.value)}
                              show={showPassword}
                              onToggle={() => setShowPassword((v) => !v)}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.databaseName")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftDatabase}
                              onChange={(event) => setDraftDatabase(event.target.value)}
                            />
                          </label>
                          {selectedProfile.key === "oracle" && (
                            <>
                              <div style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>
                                  {t("database.oracleConnectionType")}
                                </span>
                                <DbxButtonGroup
                                  style={s.databaseTypeViewToggle}
                                  aria-label={t("database.oracleConnectionType")}
                                >
                                  {(["service_name", "sid"] as const).map((type) => (
                                    <DbxSegmentedButton
                                      key={type}
                                      active={draftOracleConnectionType === type}
                                      onClick={() => setDraftOracleConnectionType(type)}
                                    >
                                      {t(`database.oracleConnectionType.${type}`)}
                                    </DbxSegmentedButton>
                                  ))}
                                </DbxButtonGroup>
                              </div>
                              <label style={s.databaseSwitchRow}>
                                <input
                                  type="checkbox"
                                  checked={draftOracleSysdba}
                                  onChange={(event) => setDraftOracleSysdba(event.target.checked)}
                                />
                                <span>{t("database.oracleSysdba")}</span>
                              </label>
                            </>
                          )}
                        </>
                      )}
                      {(selectedProfile.key !== "mongodb" || draftMongoUseUrl) && (
                        <label style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>
                            {selectedProfile.key === "mongodb"
                              ? "URL"
                              : t("database.connectionString")}
                          </span>
                          <div style={s.databaseInputButtonRow}>
                            <input
                              style={s.databaseDialogInput}
                              value={draftConnectionString}
                              onChange={(event) => setDraftConnectionString(event.target.value)}
                              placeholder={
                                selectedProfile.key === "mongodb"
                                  ? "mongodb+srv://user:pass@cluster.mongodb.net/mydb"
                                  : "jdbc:postgresql://localhost:5432/postgres"
                              }
                            />
                            <DbxButton variant="outline" size="sm" onClick={applyConnectionUrl}>
                              {t("database.parseConnectionUrl")}
                            </DbxButton>
                          </div>
                        </label>
                      )}
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.urlParams")}</span>
                        <input
                          style={s.databaseDialogInput}
                          value={draftUrlParams}
                          onChange={(event) => setDraftUrlParams(event.target.value)}
                          placeholder="sslmode=require&connectTimeout=15"
                        />
                      </label>
                      {selectedProfile.key === "redis" && (
                        <>
                          <div style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>
                              {t("database.redisConnectionMode")}
                            </span>
                            <DbxButtonGroup
                              style={s.databaseTypeViewToggle}
                              aria-label={t("database.redisConnectionMode")}
                            >
                              {(["standalone", "sentinel", "cluster"] as const).map((mode) => (
                                <DbxSegmentedButton
                                  key={mode}
                                  active={draftRedisConnectionMode === mode}
                                  onClick={() => setDraftRedisConnectionMode(mode)}
                                >
                                  {t(`database.redisMode.${mode}`)}
                                </DbxSegmentedButton>
                              ))}
                            </DbxButtonGroup>
                          </div>
                          {draftRedisConnectionMode === "sentinel" && (
                            <>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>
                                  {t("database.redisSentinelNodes")}
                                </span>
                                <textarea
                                  style={{
                                    ...s.databaseDialogInput,
                                    minHeight: 64,
                                    resize: "vertical",
                                  }}
                                  value={draftRedisSentinelNodes}
                                  onChange={(event) =>
                                    setDraftRedisSentinelNodes(event.target.value)
                                  }
                                  placeholder={"sentinel-1:26379\nsentinel-2:26379"}
                                />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>
                                  {t("database.redisSentinelMaster")}
                                </span>
                                <input
                                  style={s.databaseDialogInput}
                                  value={draftRedisSentinelMaster}
                                  onChange={(event) =>
                                    setDraftRedisSentinelMaster(event.target.value)
                                  }
                                  placeholder="mymaster"
                                />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>
                                  {t("database.redisSentinelUser")}
                                </span>
                                <input
                                  style={s.databaseDialogInput}
                                  value={draftRedisSentinelUsername}
                                  onChange={(event) =>
                                    setDraftRedisSentinelUsername(event.target.value)
                                  }
                                />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>
                                  {t("database.redisSentinelPassword")}
                                </span>
                                <PasswordInput
                                  style={s.databaseDialogInput}
                                  value={draftRedisSentinelPassword}
                                  onChange={(event) =>
                                    setDraftRedisSentinelPassword(event.target.value)
                                  }
                                  show={showSentinelPassword}
                                  onToggle={() => setShowSentinelPassword((v) => !v)}
                                />
                              </label>
                              <label style={s.databaseSwitchRow}>
                                <input
                                  type="checkbox"
                                  checked={draftRedisSentinelTls}
                                  onChange={(event) =>
                                    setDraftRedisSentinelTls(event.target.checked)
                                  }
                                />
                                <span>{t("database.redisSentinelTls")}</span>
                              </label>
                            </>
                          )}
                          {draftRedisConnectionMode === "cluster" && (
                            <label style={s.databaseDialogField}>
                              <span style={s.databaseDialogLabel}>
                                {t("database.redisClusterNodes")}
                              </span>
                              <textarea
                                style={{
                                  ...s.databaseDialogInput,
                                  minHeight: 64,
                                  resize: "vertical",
                                }}
                                value={draftRedisClusterNodes}
                                onChange={(event) => setDraftRedisClusterNodes(event.target.value)}
                                placeholder={"redis-1:6379\nredis-2:6379"}
                              />
                            </label>
                          )}
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>
                              {t("database.redisKeySeparator")}
                            </span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftRedisKeySeparator}
                              onChange={(event) => setDraftRedisKeySeparator(event.target.value)}
                              placeholder=":"
                            />
                          </label>
                        </>
                      )}
                    </>
                  )}
                </div>
              ) : configTab === "tls" ? (
                <div style={s.databaseDialogPanel}>
                  <label style={s.databaseSwitchRow}>
                    <input
                      type="checkbox"
                      checked={tlsEnabled}
                      onChange={(event) => setTlsEnabled(event.target.checked)}
                    />
                    <span>{t("database.enableTls")}</span>
                  </label>
                  {selectedProfile.key === "postgres" && (
                    <label style={s.databaseDialogField}>
                      <span style={s.databaseDialogLabel}>{t("connection.postgresSslMode")}</span>
                      <select
                        style={{ ...s.databaseDialogInput, height: 32 }}
                        value={draftTlsMode || "prefer"}
                        onChange={(event) => setDraftTlsMode(event.target.value)}
                      >
                        <option value="disable">{t("connection.postgresSslModeDisable")}</option>
                        <option value="prefer">{t("connection.postgresSslModePrefer")}</option>
                        <option value="require">{t("connection.postgresSslModeRequire")}</option>
                        <option value="verify-ca">{t("connection.postgresSslModeVerifyCa")}</option>
                        <option value="verify-full">
                          {t("connection.postgresSslModeVerifyFull")}
                        </option>
                      </select>
                    </label>
                  )}
                  {selectedProfile.key === "mysql" && (
                    <label style={s.databaseDialogField}>
                      <span style={s.databaseDialogLabel}>{t("connection.mysqlTlsMode")}</span>
                      <select
                        style={{ ...s.databaseDialogInput, height: 32 }}
                        value={draftTlsMode || "preferred"}
                        onChange={(event) => setDraftTlsMode(event.target.value)}
                      >
                        <option value="preferred">{t("connection.mysqlTlsModePreferred")}</option>
                        <option value="disabled">{t("connection.mysqlTlsModeDisabled")}</option>
                        <option value="required">{t("connection.mysqlTlsModeRequired")}</option>
                        <option value="verify_ca">{t("connection.mysqlTlsModeVerifyCa")}</option>
                        <option value="verify_identity">
                          {t("connection.mysqlTlsModeVerifyIdentity")}
                        </option>
                      </select>
                    </label>
                  )}
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.caCertPath")}</span>
                    <div style={s.databaseInputButtonRow}>
                      <input
                        style={s.databaseDialogInput}
                        value={draftCaCertPath}
                        onChange={(event) => setDraftCaCertPath(event.target.value)}
                      />
                      <DbxButton
                        variant="ghost"
                        size="icon-sm"
                        icon={FilePlus}
                        aria-label={t("database.chooseCaCertPath")}
                        title={t("database.chooseCaCertPath")}
                        onClick={() => {
                          void chooseTlsCertificatePath(setDraftCaCertPath);
                        }}
                      />
                    </div>
                  </label>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.clientCertPath")}</span>
                    <div style={s.databaseInputButtonRow}>
                      <input
                        style={s.databaseDialogInput}
                        value={draftClientCertPath}
                        onChange={(event) => setDraftClientCertPath(event.target.value)}
                      />
                      <DbxButton
                        variant="ghost"
                        size="icon-sm"
                        icon={FilePlus}
                        aria-label={t("database.chooseClientCertPath")}
                        title={t("database.chooseClientCertPath")}
                        onClick={() => {
                          void chooseTlsCertificatePath(setDraftClientCertPath);
                        }}
                      />
                    </div>
                  </label>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.clientKeyPath")}</span>
                    <div style={s.databaseInputButtonRow}>
                      <input
                        style={s.databaseDialogInput}
                        value={draftClientKeyPath}
                        onChange={(event) => setDraftClientKeyPath(event.target.value)}
                      />
                      <DbxButton
                        variant="ghost"
                        size="icon-sm"
                        icon={FilePlus}
                        aria-label={t("database.chooseClientKeyPath")}
                        title={t("database.chooseClientKeyPath")}
                        onClick={() => {
                          void chooseTlsCertificatePath(setDraftClientKeyPath);
                        }}
                      />
                    </div>
                  </label>
                  <div style={s.databaseDialogHint}>{t("database.tlsHint")}</div>
                </div>
              ) : configTab === "transport" ? (
                <div style={s.databaseDialogPanel}>
                  <label style={s.databaseSwitchRow}>
                    <input
                      type="checkbox"
                      checked={transportEnabled}
                      onChange={(event) => setTransportEnabled(event.target.checked)}
                    />
                    <span>{t("database.enableTransport")}</span>
                  </label>
                  <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                    <DbxButton
                      variant="outline"
                      size="sm"
                      icon={Plus}
                      onClick={() => addTransportLayer("ssh")}
                    >
                      {t("database.addSshHop")}
                    </DbxButton>
                    <DbxButton
                      variant="outline"
                      size="sm"
                      icon={Plus}
                      onClick={() => addTransportLayer("proxy")}
                    >
                      {t("database.addProxyLayer")}
                    </DbxButton>
                  </div>
                  {draftTransportLayers.length > 0 && (
                    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                      {draftTransportLayers.map((layer, index) => (
                        <button
                          key={layer.id}
                          type="button"
                          style={{
                            ...s.databaseListButton,
                            ...(selectedTransportLayer?.id === layer.id
                              ? s.databaseListButtonActive
                              : {}),
                          }}
                          onClick={() => setSelectedTransportLayerId(layer.id)}
                        >
                          <input
                            type="checkbox"
                            checked={layer.enabled}
                            onClick={(event) => event.stopPropagation()}
                            onChange={(event) =>
                              updateTransportLayer(layer.id, { enabled: event.target.checked })
                            }
                          />
                          <span style={{ color: "var(--text-hint)", width: 18 }}>{index + 1}</span>
                          <span
                            style={{
                              flex: 1,
                              minWidth: 0,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {layer.name ||
                              layer.host ||
                              (layer.type === "ssh"
                                ? t("database.addSshHop")
                                : t("database.addProxyLayer"))}
                          </span>
                          <span
                            style={{
                              color: "var(--text-hint)",
                              fontSize: 10,
                              textTransform: "uppercase",
                            }}
                          >
                            {layer.type}
                          </span>
                          <DbxButton
                            variant="ghost"
                            size="icon-xs"
                            icon={Copy}
                            aria-label={t("database.copyTransportLayer")}
                            onClick={(event) => {
                              event.stopPropagation();
                              copyTransportLayer(layer.id);
                            }}
                          />
                          {index > 0 && (
                            <DbxButton
                              variant="ghost"
                              size="icon-xs"
                              icon={ArrowUp}
                              aria-label={t("database.moveTransportLayerUp")}
                              onClick={(event) => {
                                event.stopPropagation();
                                moveTransportLayer(layer.id, -1);
                              }}
                            />
                          )}
                          {index < draftTransportLayers.length - 1 && (
                            <DbxButton
                              variant="ghost"
                              size="icon-xs"
                              icon={ArrowDown}
                              aria-label={t("database.moveTransportLayerDown")}
                              onClick={(event) => {
                                event.stopPropagation();
                                moveTransportLayer(layer.id, 1);
                              }}
                            />
                          )}
                          <DbxButton
                            variant="ghost"
                            size="icon-xs"
                            icon={Trash2}
                            onClick={(event) => {
                              event.stopPropagation();
                              removeTransportLayer(layer.id);
                            }}
                          />
                        </button>
                      ))}
                    </div>
                  )}
                  {selectedTransportLayer ? (
                    <div style={s.databaseDialogFormGrid}>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>
                          {t("database.transportLayerName")}
                        </span>
                        <input
                          style={s.databaseDialogInput}
                          value={selectedTransportLayer.name}
                          onChange={(event) =>
                            updateTransportLayer(selectedTransportLayer.id, {
                              name: event.target.value,
                            })
                          }
                        />
                      </label>
                      <div style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>
                          {t("database.transportLayerType")}
                        </span>
                        <DbxButtonGroup
                          style={s.databaseTypeViewToggle}
                          aria-label={t("database.transportLayerType")}
                        >
                          {(["ssh", "proxy"] as const).map((type) => (
                            <DbxSegmentedButton
                              key={type}
                              active={selectedTransportLayer.type === type}
                              onClick={() =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  type,
                                  port: type === "ssh" ? "22" : "1080",
                                })
                              }
                            >
                              {type === "ssh" ? "SSH" : "Proxy"}
                            </DbxSegmentedButton>
                          ))}
                        </DbxButtonGroup>
                      </div>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>
                          {selectedTransportLayer.type === "ssh"
                            ? t("database.sshHost")
                            : t("database.proxyHost")}
                        </span>
                        <input
                          style={s.databaseDialogInput}
                          value={selectedTransportLayer.host}
                          onChange={(event) =>
                            updateTransportLayer(selectedTransportLayer.id, {
                              host: event.target.value,
                            })
                          }
                          placeholder={
                            selectedTransportLayer.type === "ssh"
                              ? "ssh.example.com"
                              : "proxy.example.com"
                          }
                        />
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.port")}</span>
                        <input
                          style={s.databaseDialogInput}
                          value={selectedTransportLayer.port}
                          onChange={(event) =>
                            updateTransportLayer(selectedTransportLayer.id, {
                              port: event.target.value,
                            })
                          }
                        />
                      </label>
                      {selectedTransportLayer.type === "ssh" ? (
                        <>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.sshUser")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.user}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  user: event.target.value,
                                })
                              }
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.sshPassword")}</span>
                            <PasswordInput
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.password}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  password: event.target.value,
                                })
                              }
                              show={showTransportPassword}
                              onToggle={() => setShowTransportPassword((v) => !v)}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.sshKeyPath")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.keyPath}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  keyPath: event.target.value,
                                })
                              }
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>
                              {t("database.sshKeyPassphrase")}
                            </span>
                            <PasswordInput
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.keyPassphrase}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  keyPassphrase: event.target.value,
                                })
                              }
                              show={showTransportPassphrase}
                              onToggle={() => setShowTransportPassphrase((v) => !v)}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>
                              {t("database.connectTimeoutSecs")}
                            </span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.connectTimeoutSecs}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  connectTimeoutSecs: event.target.value,
                                })
                              }
                            />
                          </label>
                          <label style={s.databaseSwitchRow}>
                            <input
                              type="checkbox"
                              checked={selectedTransportLayer.useSshAgent}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  useSshAgent: event.target.checked,
                                })
                              }
                            />
                            <span>{t("database.sshUseAgent")}</span>
                          </label>
                          <label style={s.databaseSwitchRow}>
                            <input
                              type="checkbox"
                              checked={selectedTransportLayer.exposeLan}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  exposeLan: event.target.checked,
                                })
                              }
                            />
                            <span>{t("database.sshExposeLan")}</span>
                          </label>
                        </>
                      ) : (
                        <>
                          <div style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.proxyType")}</span>
                            <DbxButtonGroup
                              style={s.databaseTypeViewToggle}
                              aria-label={t("database.proxyType")}
                            >
                              {(["socks5", "http"] as const).map((proxyType) => (
                                <DbxSegmentedButton
                                  key={proxyType}
                                  active={selectedTransportLayer.proxyType === proxyType}
                                  onClick={() =>
                                    updateTransportLayer(selectedTransportLayer.id, {
                                      proxyType,
                                    })
                                  }
                                >
                                  {proxyType.toUpperCase()}
                                </DbxSegmentedButton>
                              ))}
                            </DbxButtonGroup>
                          </div>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.proxyUsername")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.username}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  username: event.target.value,
                                })
                              }
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.proxyPassword")}</span>
                            <PasswordInput
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.password}
                              onChange={(event) =>
                                updateTransportLayer(selectedTransportLayer.id, {
                                  password: event.target.value,
                                })
                              }
                              show={showTransportPassword}
                              onToggle={() => setShowTransportPassword((v) => !v)}
                            />
                          </label>
                        </>
                      )}
                    </div>
                  ) : (
                    <div style={s.databaseDialogHint}>{t("database.transportHint")}</div>
                  )}
                </div>
              ) : (
                <div style={s.databaseDialogPanel}>
                  <label style={s.databaseSwitchRow}>
                    <input
                      type="checkbox"
                      checked={draftReadOnly}
                      onChange={(event) => setDraftReadOnly(event.target.checked)}
                    />
                    <span>{t("database.openReadOnly")}</span>
                  </label>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 12,
                      paddingTop: 10,
                      borderTop: "1px solid var(--border-dim)",
                    }}
                  >
                    <span style={{ ...s.databaseDialogLabel, color: "var(--danger)" }}>
                      {t("database.productionProtection")}
                    </span>
                    <label style={s.databaseSwitchRow}>
                      <input
                        type="checkbox"
                        checked={draftProductionProtectionEnabled}
                        onChange={(event) => {
                          const enabled = event.target.checked;
                          setDraftProductionProtectionEnabled(enabled);
                          if (enabled && selectedProfile.localFile) {
                            setDraftProductionScope("connection");
                          }
                        }}
                      />
                      <span>{t("database.productionProtectionEnable")}</span>
                    </label>
                  </div>
                  {draftProductionProtectionEnabled && (
                    <>
                      <div style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.productionScope")}</span>
                        <DbxButtonGroup
                          style={{ ...s.databaseTypeViewToggle, alignSelf: "flex-start" }}
                          aria-label={t("database.productionScope")}
                        >
                          <DbxSegmentedButton
                            active={draftProductionScope === "connection"}
                            onClick={() => setDraftProductionScope("connection")}
                          >
                            {t("database.productionScopeConnection")}
                          </DbxSegmentedButton>
                          <DbxSegmentedButton
                            active={draftProductionScope === "databases"}
                            disabled={Boolean(selectedProfile.localFile)}
                            title={
                              selectedProfile.localFile
                                ? t("database.productionSingleDatabaseHint")
                                : undefined
                            }
                            onClick={() => setDraftProductionScope("databases")}
                          >
                            {t("database.productionScopeDatabases")}
                          </DbxSegmentedButton>
                        </DbxButtonGroup>
                        <span style={s.databaseDialogHint}>
                          {draftProductionScope === "connection"
                            ? t("database.productionConnectionHint")
                            : t("database.productionDatabasesHint")}
                        </span>
                      </div>
                      {draftProductionScope === "databases" && !selectedProfile.localFile && (
                        <label style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>
                            {t("database.productionDatabases")}
                          </span>
                          <textarea
                            style={{
                              ...s.databaseDialogInput,
                              minHeight: 72,
                              height: "auto",
                              padding: 8,
                              lineHeight: 1.45,
                              resize: "vertical",
                            }}
                            value={draftProductionDatabases}
                            onChange={(event) => setDraftProductionDatabases(event.target.value)}
                            placeholder={t("database.productionDatabasesPlaceholder")}
                          />
                        </label>
                      )}
                    </>
                  )}
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.connectTimeoutSecs")}</span>
                    <input
                      style={s.databaseDialogInput}
                      type="number"
                      min={1}
                      max={300}
                      value={draftConnectTimeoutSecs}
                      onChange={(event) => setDraftConnectTimeoutSecs(event.target.value)}
                    />
                  </label>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.queryTimeoutSecs")}</span>
                    <input
                      style={s.databaseDialogInput}
                      type="number"
                      min={0}
                      max={300}
                      value={draftQueryTimeoutSecs}
                      onChange={(event) => setDraftQueryTimeoutSecs(event.target.value)}
                    />
                  </label>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.idleTimeoutSecs")}</span>
                    <input
                      style={s.databaseDialogInput}
                      type="number"
                      min={0}
                      max={600}
                      value={draftIdleTimeoutSecs}
                      onChange={(event) => setDraftIdleTimeoutSecs(event.target.value)}
                    />
                  </label>
                  <label style={s.databaseSwitchRow}>
                    <input
                      type="checkbox"
                      checked={Number.parseInt(draftKeepaliveIntervalSecs, 10) > 0}
                      onChange={(event) =>
                        setDraftKeepaliveIntervalSecs(event.target.checked ? "30" : "0")
                      }
                    />
                    <span>{t("database.enableKeepalive")}</span>
                  </label>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.keepaliveIntervalSecs")}</span>
                    <input
                      style={s.databaseDialogInput}
                      type="number"
                      min={1}
                      max={3600}
                      value={draftKeepaliveIntervalSecs}
                      disabled={Number.parseInt(draftKeepaliveIntervalSecs, 10) <= 0}
                      onChange={(event) => setDraftKeepaliveIntervalSecs(event.target.value)}
                    />
                  </label>
                  {selectedProfile.key === "duckdb" && (
                    <label style={s.databaseDialogField}>
                      <span style={s.databaseDialogLabel}>{t("database.initScript")}</span>
                      <textarea
                        style={{
                          ...s.databaseDialogInput,
                          minHeight: 84,
                          height: "auto",
                          padding: 8,
                          lineHeight: 1.45,
                          resize: "vertical",
                          fontFamily: "var(--font-mono)",
                        }}
                        value={draftInitScript}
                        onChange={(event) => setDraftInitScript(event.target.value)}
                        placeholder={t("database.initScriptPlaceholder")}
                        spellCheck={false}
                      />
                      <span style={s.databaseDialogHint}>{t("database.initScriptHint")}</span>
                    </label>
                  )}
                  {selectedProfile.key === "oracle" && (
                    <label style={s.databaseDialogField}>
                      <span style={s.databaseDialogLabel}>{t("database.agentJavaOptions")}</span>
                      <textarea
                        style={{
                          ...s.databaseDialogInput,
                          minHeight: 72,
                          height: "auto",
                          padding: 8,
                          lineHeight: 1.45,
                          resize: "vertical",
                          fontFamily: "var(--font-mono)",
                        }}
                        value={draftAgentJavaOptions}
                        onChange={(event) => setDraftAgentJavaOptions(event.target.value)}
                        placeholder={t("database.agentJavaOptionsPlaceholder")}
                        spellCheck={false}
                      />
                      <span style={s.databaseDialogHint}>{t("database.agentJavaOptionsHint")}</span>
                    </label>
                  )}
                  <div style={s.databaseDialogHint}>{t("database.advancedHint")}</div>
                </div>
              )}
            </>
          )}
        </div>
        <div style={s.databaseDialogFooter}>
          {connectionTestResult && (
            <span
              style={{
                fontSize: 12,
                color: connectionTestResult.success ? "var(--success)" : "var(--danger)",
                flex: 1,
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {connectionTestResult.message}
              <DbxButton
                variant="ghost"
                size="icon-xs"
                icon={Copy}
                aria-label={t("database.copyTestResult")}
                onClick={() => {
                  void navigator.clipboard.writeText(connectionTestResult.message);
                }}
              />
            </span>
          )}
          <DbxButton variant="outline" size="sm" onClick={onClose}>
            {t("common.cancel")}
          </DbxButton>
          {wizardStep === "config" && selectedProfile.key !== "sqlite" && (
            <DbxButton
              variant="outline"
              size="sm"
              icon={Plug}
              onClick={testDbxConnectionDraft}
              disabled={
                loading || (selectedProfile.localFile ? !draftFilePath.trim() : !draftHost.trim())
              }
            >
              {t("database.testConnection")}
            </DbxButton>
          )}
          <DbxButton
            variant="default"
            size="sm"
            icon={wizardStep === "type" ? ChevronRight : Plus}
            type="submit"
            disabled={
              loading ||
              (wizardStep === "config" &&
                (selectedProfile.localFile ? !draftFilePath.trim() : !draftHost.trim()))
            }
          >
            {wizardStep === "type"
              ? t("database.next")
              : editingConnection
                ? t("common.save")
                : t("database.addConnection")}
          </DbxButton>
        </div>
      </form>
    </div>
  );
}
