import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import { STORAGE_PROTOCOLS, type StorageProtocol } from "../types/storage";
import {
  emptyStorageDraft,
  groupStorageProtocols,
  normalizeStorageDraft,
  storageConnectionSummary,
  storageDraftFromConnection,
  storageFieldLabelKey,
  storageFieldsForProtocol,
  storageProtocolGroup,
  switchStorageDraftProtocol,
  validateStorageDraft,
  type StorageConnectionDraft,
} from "../components/storage/storageProtocolForm";

/**
 * 后端 `required_config_keys`(src-tauri/src/storage_conn.rs)与 `secret_keys` 的
 * 镜像。前端表单必须覆盖它们,否则用户填完表单仍会被后端拒绝。
 */
const BACKEND_REQUIRED_CONFIG: Record<StorageProtocol, string[]> = {
  s3: ["bucket", "region"],
  s3Compatible: ["bucket", "endpoint"],
  aliyunOss: ["bucket", "endpoint"],
  tencentCos: ["bucket", "endpoint"],
  jdCloudOss: ["bucket", "endpoint"],
  qiniuKodo: ["bucket", "endpoint"],
  upyun: ["bucket"],
  webdavHttps: ["endpoint"],
  webdavHttp: ["endpoint"],
  dropbox: [],
  oneDrive: [],
  googleDrive: [],
  aliyunDrive: [],
  box: [],
  baiduNetdisk: [],
  smb: ["host", "share"],
  afp: ["host", "share"],
  nfs: ["host", "export"],
};

const BACKEND_SECRET_KEYS: Record<StorageProtocol, string[]> = {
  s3: ["accessKeyId", "secretAccessKey", "sessionToken"],
  s3Compatible: ["accessKeyId", "secretAccessKey", "sessionToken"],
  aliyunOss: ["accessKeyId", "secretAccessKey", "sessionToken"],
  tencentCos: ["accessKeyId", "secretAccessKey", "sessionToken"],
  jdCloudOss: ["accessKeyId", "secretAccessKey", "sessionToken"],
  qiniuKodo: ["accessKeyId", "secretAccessKey", "sessionToken"],
  upyun: ["operator", "password"],
  webdavHttps: ["username", "password"],
  webdavHttp: ["username", "password"],
  dropbox: ["accessToken", "refreshToken", "clientId", "clientSecret", "expiresAtMs"],
  oneDrive: ["accessToken", "refreshToken", "clientId", "clientSecret", "expiresAtMs"],
  googleDrive: ["accessToken", "refreshToken", "clientId", "clientSecret", "expiresAtMs"],
  aliyunDrive: ["accessToken", "refreshToken", "clientId", "clientSecret", "expiresAtMs"],
  box: ["accessToken", "refreshToken", "clientId", "clientSecret", "expiresAtMs"],
  baiduNetdisk: ["accessToken", "refreshToken", "clientId", "clientSecret", "expiresAtMs"],
  smb: ["username", "password"],
  afp: ["username", "password"],
  nfs: [],
};

const OAUTH_PROTOCOLS: StorageProtocol[] = [
  "dropbox",
  "oneDrive",
  "googleDrive",
  "aliyunDrive",
  "box",
  "baiduNetdisk",
];

function draft(protocol: StorageProtocol, over: Partial<StorageConnectionDraft> = {}) {
  return { ...emptyStorageDraft(protocol), name: "Bucket", ...over };
}

describe("存储协议表单描述", () => {
  it("覆盖全部 18 种协议", () => {
    expect(STORAGE_PROTOCOLS).toHaveLength(18);
    for (const protocol of STORAGE_PROTOCOLS) {
      expect(storageFieldsForProtocol(protocol).length, protocol).toBeGreaterThan(0);
      expect(storageProtocolGroup(protocol), protocol).toBeTruthy();
    }
  });

  it("每个后端必填的 config 键都在表单里并标为必填", () => {
    for (const protocol of STORAGE_PROTOCOLS) {
      const specs = storageFieldsForProtocol(protocol);
      for (const key of BACKEND_REQUIRED_CONFIG[protocol]) {
        const spec = specs.find((item) => item.key === key && item.kind === "config");
        expect(spec, `${protocol}.${key} 缺少表单字段`).toBeDefined();
        expect(spec?.required, `${protocol}.${key} 应为必填`).toBe(true);
      }
    }
  });

  it("表单里的凭据字段都属于后端认可的 secret 键", () => {
    for (const protocol of STORAGE_PROTOCOLS) {
      const secretSpecs = storageFieldsForProtocol(protocol).filter(
        (spec) => spec.kind === "secret",
      );
      for (const spec of secretSpecs) {
        expect(
          BACKEND_SECRET_KEYS[protocol],
          `${protocol}.${spec.key} 不是后端的 secret 键,会被当成公开 config 落盘`,
        ).toContain(spec.key);
      }
    }
  });

  it("密码类字段一律遮蔽", () => {
    for (const protocol of STORAGE_PROTOCOLS) {
      for (const spec of storageFieldsForProtocol(protocol)) {
        if (/password|secret|Secret|Token/.test(spec.key) && spec.key !== "sessionToken") {
          expect(spec.masked, `${protocol}.${spec.key} 应遮蔽`).toBe(true);
        }
      }
    }
  });

  it("所有标签与提示的 i18n key 在 en / zh 都存在", () => {
    for (const protocol of STORAGE_PROTOCOLS) {
      expect(en[`storage.protocol.${protocol}`], protocol).toBeTruthy();
      expect(zh[`storage.protocol.${protocol}`], protocol).toBeTruthy();
      for (const spec of storageFieldsForProtocol(protocol)) {
        const labelKey = storageFieldLabelKey(spec);
        expect(en[labelKey], labelKey).toBeTruthy();
        expect(zh[labelKey], labelKey).toBeTruthy();
        if (spec.hintKey) {
          expect(en[spec.hintKey], spec.hintKey).toBeTruthy();
          expect(zh[spec.hintKey], spec.hintKey).toBeTruthy();
        }
      }
    }
  });

  it("协议分组把 18 种服务分完且不重复", () => {
    const descriptors = STORAGE_PROTOCOLS.map((protocol) => ({
      protocol,
      displayName: protocol,
      capability: {
        readDir: true,
        read: true,
        write: true,
        createDir: true,
        delete: true,
        rename: true,
        copy: true,
        stat: true,
        richMetadata: true,
      },
      requiredConfigKeys: [],
      secretKeys: [],
      defaultEndpoint: null,
      oauth: false,
      systemMount: false,
      deprecated: false,
    }));
    const grouped = groupStorageProtocols(descriptors);
    const flat = grouped.flatMap((entry) => entry.descriptors.map((item) => item.protocol));
    expect(new Set(flat).size).toBe(18);
    expect(flat).toHaveLength(18);
  });
});

describe("存储草稿校验", () => {
  it("名称必填", () => {
    expect(validateStorageDraft(draft("s3", { name: "  " })).name).toBe(
      "storage.error.nameRequired",
    );
  });

  it("缺少必填 config 键时报错", () => {
    const errors = validateStorageDraft(draft("s3"));
    expect(errors.bucket).toBe("storage.error.fieldRequired");
    expect(errors.region).toBe("storage.error.fieldRequired");
  });

  it("必填凭据已保存过就允许留空", () => {
    const upyun = draft("upyun", { config: { bucket: "media" } });
    expect(validateStorageDraft(upyun).operator).toBe("storage.error.fieldRequired");
    expect(validateStorageDraft(upyun, ["operator", "password"]).operator).toBeUndefined();
  });

  it("WebDAV 强制区分 http / https", () => {
    expect(
      validateStorageDraft(draft("webdavHttps", { config: { endpoint: "http://dav.test/x" } }))
        .endpoint,
    ).toBe("storage.error.httpsRequired");
    expect(
      validateStorageDraft(draft("webdavHttp", { config: { endpoint: "https://dav.test/x" } }))
        .endpoint,
    ).toBe("storage.error.httpRequired");
    expect(
      validateStorageDraft(draft("webdavHttps", { config: { endpoint: "https://dav.test/x" } }))
        .endpoint,
    ).toBeUndefined();
  });

  it("拒绝非 http(s) 与畸形接入点", () => {
    for (const endpoint of ["ftp://x.test", "not a url", "s3.example.com"]) {
      expect(
        validateStorageDraft(draft("s3Compatible", { config: { bucket: "b", endpoint } })).endpoint,
        endpoint,
      ).toBe("storage.error.invalidEndpoint");
    }
  });

  it("校验端口、绝对导出路径与主机形态", () => {
    expect(
      validateStorageDraft(draft("smb", { config: { host: "h", share: "s", port: "70000" } })).port,
    ).toBe("storage.error.invalidPort");
    expect(
      validateStorageDraft(draft("nfs", { config: { host: "h", export: "volume1" } })).export,
    ).toBe("storage.error.absolutePathRequired");
    expect(validateStorageDraft(draft("smb", { config: { host: "a/b", share: "s" } })).host).toBe(
      "storage.error.invalidHost",
    );
  });

  it("OAuth 协议在授权前也能通过校验", () => {
    for (const protocol of OAUTH_PROTOCOLS) {
      expect(validateStorageDraft(draft(protocol)), protocol).toEqual({});
    }
  });
});

describe("存储草稿归一", () => {
  it("裁掉不属于当前协议的字段并 trim", () => {
    const connection = normalizeStorageDraft(
      draft("s3", {
        name: "  Media  ",
        group: " Prod ",
        config: { bucket: " media ", region: "us-east-1", host: "leftover" },
        secrets: { accessKeyId: " AKIA ", password: "leftover" },
      }),
      1000,
    );
    expect(connection.name).toBe("Media");
    expect(connection.group).toBe("Prod");
    expect(connection.config).toEqual({ bucket: "media", region: "us-east-1" });
    expect(connection.secrets).toEqual({ accessKeyId: "AKIA" });
    expect(connection.createdAt).toBe(1000);
  });

  it("保留 OAuth 授权写回的凭据键", () => {
    const connection = normalizeStorageDraft(
      draft("dropbox", {
        secrets: { refreshToken: "r1", clientId: "c1", expiresAtMs: "123" },
      }),
    );
    expect(connection.secrets).toEqual({
      refreshToken: "r1",
      clientId: "c1",
      expiresAtMs: "123",
    });
  });

  it("切换协议时不会复用旧协议的凭据或 OAuth token", () => {
    const next = switchStorageDraftProtocol(
      draft("dropbox", {
        config: { root: "/media" },
        secrets: {
          accessToken: "dropbox-access",
          refreshToken: "dropbox-refresh",
          clientId: "dropbox-client",
        },
      }),
      "googleDrive",
    );
    expect(next.protocol).toBe("googleDrive");
    expect(next.config).toEqual({ root: "/media" });
    expect(next.secrets).toEqual({});
  });

  it("空分组不写 group 字段", () => {
    expect(normalizeStorageDraft(draft("dropbox", { group: "  " }))).not.toHaveProperty("group");
  });

  it("已有连接保留原 createdAt", () => {
    const connection = normalizeStorageDraft(draft("dropbox", { createdAt: 42 }), 999);
    expect(connection.createdAt).toBe(42);
  });

  it("从连接建草稿时不带任何凭据", () => {
    const result = storageDraftFromConnection({
      id: "c1",
      name: "Bucket",
      protocol: "s3",
      config: { bucket: "media" },
      secrets: { accessKeyId: "SHOULD-NOT-APPEAR" },
      createdAt: 7,
    });
    expect(result.secrets).toEqual({});
    expect(result.config).toEqual({ bucket: "media" });
  });

  it("切换协议时只保留新协议仍有的字段", () => {
    const next = switchStorageDraftProtocol(
      draft("s3", {
        config: { bucket: "media", region: "us-east-1", root: "/a" },
        secrets: { accessKeyId: "AKIA" },
      }),
      "smb",
    );
    expect(next.protocol).toBe("smb");
    // root 两个协议都有,bucket/region 只属于 S3。
    expect(next.config).toEqual({ root: "/a" });
    expect(next.secrets).toEqual({});
  });
});

describe("连接摘要", () => {
  it("按协议给出可读的第二行", () => {
    expect(
      storageConnectionSummary({
        id: "1",
        name: "n",
        protocol: "s3",
        config: { bucket: "media", region: "us-east-1" },
        createdAt: 0,
      }),
    ).toBe("media · us-east-1");
    expect(
      storageConnectionSummary({
        id: "1",
        name: "n",
        protocol: "nfs",
        config: { host: "10.0.0.2", export: "/vol1" },
        createdAt: 0,
      }),
    ).toBe("10.0.0.2:/vol1");
    expect(
      storageConnectionSummary({
        id: "1",
        name: "n",
        protocol: "smb",
        config: { host: "10.0.0.2", share: "public" },
        createdAt: 0,
      }),
    ).toBe("10.0.0.2/public");
  });
});
