import { describe, expect, it } from "vitest";
import type { AeroricDbConnectionConfig } from "../types";
import {
  DB_PROFILES,
  buildDbxConnectionConfig,
  normalizeDelimitedList,
  normalizeLineList,
  type ConnectionDraft,
  type DbProfileKey,
} from "../components/database/databaseConnectionDraft";

function profile(key: DbProfileKey) {
  return DB_PROFILES.find((item) => item.key === key)!;
}

function baseDraft(key: DbProfileKey, overrides: Partial<ConnectionDraft> = {}): ConnectionDraft {
  const p = profile(key);
  return {
    profile: p,
    name: "",
    connectionGroup: "",
    color: "",
    host: p.localFile ? "" : "127.0.0.1",
    port: String(p.port),
    user: p.user,
    database: "",
    password: "",
    filePath: "",
    readOnly: false,
    initScript: "",
    agentJavaOptions: "",
    isProduction: false,
    productionDatabases: "",
    urlParams: "",
    connectionString: "",
    connectTimeoutSecs: "5",
    queryTimeoutSecs: "30",
    idleTimeoutSecs: "60",
    keepaliveIntervalSecs: "0",
    caCertPath: "",
    clientCertPath: "",
    clientKeyPath: "",
    redisConnectionMode: "standalone",
    redisSentinelMaster: "",
    redisSentinelNodes: "",
    redisSentinelUsername: "",
    redisSentinelPassword: "",
    redisSentinelTls: false,
    redisClusterNodes: "",
    redisKeySeparator: ":",
    mongoUseUrl: false,
    tlsEnabled: false,
    tlsMode: "",
    oracleConnectionType: "service_name",
    oracleSysdba: false,
    transportEnabled: false,
    transportLayers: [],
    ...overrides,
  };
}

const ctx = {
  editingConnection: null,
  projectRoot: null,
  t: (key: string) => key,
};

function dbx(config: AeroricDbConnectionConfig): Record<string, unknown> {
  return config.dbx as Record<string, unknown>;
}

describe("buildDbxConnectionConfig", () => {
  it("normalizes production database and Java option lists", () => {
    expect(normalizeDelimitedList(" prod_app,prod_analytics\nPROD_APP ")).toEqual([
      "prod_app",
      "prod_analytics",
    ]);
    expect(normalizeLineList(" -Xms256m\n-Xmx1g\n-Xms256m ")).toEqual(["-Xms256m", "-Xmx1g"]);
  });

  it("serializes production safety, init script, and Java agent options", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("oracle", {
        isProduction: false,
        productionDatabases: " prod_app, prod_analytics\nPROD_APP ",
        initScript: "  SET threads = 4;  ",
        agentJavaOptions: " -Xms256m\n-Xmx1g\n-Xms256m ",
      }),
      ctx,
    );
    const config = dbx(result);

    expect(config.is_production).toBe(false);
    expect(config.production_databases).toEqual(["prod_app", "prod_analytics"]);
    expect(config.init_script).toBe("SET threads = 4;");
    expect(config.agent_java_options).toEqual(["-Xms256m", "-Xmx1g"]);
  });

  it("uses connection-wide production protection for local database profiles", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("duckdb", {
        filePath: "/tmp/app.duckdb",
        productionDatabases: "main",
      }),
      ctx,
    );
    const config = dbx(result);

    expect(config.is_production).toBe(true);
    expect(config.production_databases).toEqual([]);
  });

  it("writes postgres sslmode into url_params", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("postgres", { host: "db.example.com", tlsMode: "require" }),
      ctx,
    );
    expect(result.dbType).toBe("postgres");
    const params = new URLSearchParams(dbx(result).url_params as string);
    expect(params.get("sslmode")).toBe("require");
  });

  it("writes mysql ssl-mode into url_params", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("mysql", { host: "db.example.com", tlsMode: "REQUIRED" }),
      ctx,
    );
    const params = new URLSearchParams(dbx(result).url_params as string);
    expect(params.get("ssl-mode")).toBe("REQUIRED");
  });

  it("parses a MongoDB URL when mongoUseUrl is set", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("mongodb", {
        mongoUseUrl: true,
        connectionString: "mongodb://alice:secret@mongo.example.com:27018/analytics",
      }),
      ctx,
    );
    const config = dbx(result);
    expect(config.host).toBe("mongo.example.com");
    expect(config.port).toBe(27018);
    expect(config.username).toBe("alice");
    expect(config.password).toBe("secret");
    expect(config.database).toBe("analytics");
    expect(config.connection_string).toBe(
      "mongodb://alice:secret@mongo.example.com:27018/analytics",
    );
  });

  it("derives redis sentinel host/port from the first node", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("redis", {
        redisConnectionMode: "sentinel",
        redisSentinelMaster: "mymaster",
        redisSentinelNodes: "sentinel-a:26380\nsentinel-b:26381",
      }),
      ctx,
    );
    const config = dbx(result);
    expect(config.host).toBe("sentinel-a");
    expect(config.port).toBe(26380);
    expect(config.redis_connection_mode).toBe("sentinel");
    expect(config.redis_sentinel_master).toBe("mymaster");
    expect(config.redis_sentinel_nodes).toBe("sentinel-a:26380\nsentinel-b:26381");
  });

  it("derives redis cluster host/port from the first node", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("redis", {
        redisConnectionMode: "cluster",
        redisClusterNodes: "node-a:7000, node-b:7001",
      }),
      ctx,
    );
    const config = dbx(result);
    expect(config.host).toBe("node-a");
    expect(config.port).toBe(7000);
    expect(config.redis_cluster_nodes).toBe("node-a:7000\nnode-b:7001");
  });

  it("includes oracle connection type and sysdba flag", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("oracle", {
        host: "ora.example.com",
        oracleConnectionType: "sid",
        oracleSysdba: true,
      }),
      ctx,
    );
    const config = dbx(result);
    expect(config.oracle_connection_type).toBe("sid");
    expect(config.sysdba).toBe(true);
    expect(config.oracle_sysdba).toBeUndefined();
  });

  it("normalizes the legacy oracle sysdba key when editing", () => {
    const editing: AeroricDbConnectionConfig = {
      id: "oracle-existing",
      name: "Oracle",
      dbType: "oracle",
      readOnly: false,
      createdAt: 1,
      dbx: {
        id: "oracle-existing",
        db_type: "oracle",
        oracle_sysdba: true,
      },
    };
    const result = buildDbxConnectionConfig(
      baseDraft("oracle", {
        host: "ora.example.com",
        oracleSysdba: true,
      }),
      { editingConnection: editing, projectRoot: null, t: (key) => key },
    );
    const config = dbx(result);

    expect(config.sysdba).toBe(true);
    expect(config.oracle_sysdba).toBeUndefined();
  });

  it("serializes transport layers only when enabled", () => {
    const layer = {
      id: "t1",
      type: "ssh" as const,
      enabled: true,
      name: "SSH 1",
      host: "bastion.example.com",
      port: "2222",
      user: "deploy",
      username: "",
      password: "pw",
      keyPath: "/keys/id",
      keyPassphrase: "",
      connectTimeoutSecs: "5",
      exposeLan: false,
      useSshAgent: false,
      proxyType: "socks5" as const,
    };
    const disabled = buildDbxConnectionConfig(
      baseDraft("postgres", { host: "db", transportEnabled: false, transportLayers: [layer] }),
      ctx,
    );
    expect(dbx(disabled).transport_layers).toEqual([]);

    const enabled = buildDbxConnectionConfig(
      baseDraft("postgres", { host: "db", transportEnabled: true, transportLayers: [layer] }),
      ctx,
    );
    const layers = dbx(enabled).transport_layers as Array<Record<string, unknown>>;
    expect(layers).toHaveLength(1);
    expect(layers[0].host).toBe("bastion.example.com");
    expect(layers[0].port).toBe(2222);
    expect(layers[0].user).toBe("deploy");
  });

  it("preserves createdAt, projectScope, pinned and connectionGroup when editing", () => {
    const editing: AeroricDbConnectionConfig = {
      id: "dbx-existing",
      name: "Existing",
      dbType: "postgres",
      readOnly: false,
      createdAt: 111,
      lastOpenedAt: 222,
      pinned: true,
      connectionGroup: "group-a",
      projectScope: {
        kind: "local",
        projectRoot: "/proj",
        remoteProjectPath: null,
        sshConnectionId: null,
      },
      dbx: { id: "dbx-existing", db_type: "postgres" },
    };
    const result = buildDbxConnectionConfig(
      baseDraft("postgres", { host: "db", connectionGroup: "ignored-on-edit" }),
      { editingConnection: editing, projectRoot: "/other", t: (key) => key },
    );
    expect(result.id).toBe("dbx-existing");
    expect(result.createdAt).toBe(111);
    expect(result.pinned).toBe(true);
    expect(result.connectionGroup).toBe("group-a");
    expect(result.projectScope).toEqual(editing.projectScope);
  });

  it("uses draft connectionGroup for a new connection", () => {
    const result = buildDbxConnectionConfig(
      baseDraft("postgres", { host: "db", connectionGroup: "  group-new  " }),
      ctx,
    );
    expect(result.connectionGroup).toBe("group-new");
  });

  it("throws hostRequired when a network host is empty", () => {
    expect(() => buildDbxConnectionConfig(baseDraft("postgres", { host: "" }), ctx)).toThrowError(
      "database.hostRequired",
    );
  });

  it("throws filePathRequired when a local file path is empty", () => {
    expect(() => buildDbxConnectionConfig(baseDraft("sqlite", { filePath: "" }), ctx)).toThrowError(
      "database.filePathRequired",
    );
  });
});
