/**
 * 18 种存储协议的表单描述与草稿校验。
 *
 * 后端 `storage_protocols` 已经给出必填的 config 键与凭据键,这里补充前端需要的
 * 顺序、标签、占位符与可选项;`src/test/storage-protocol-form.test.ts` 会用后端
 * 描述符交叉校验,防止两边漂移。
 */
import type {
  StorageConnection,
  StorageProtocol,
  StorageProtocolDescriptor,
} from "../../types/storage";

/** 字段归属:`config` 写公开文件,`secret` 写 0600 凭据文件。 */
export type StorageFieldKind = "config" | "secret";

export interface StorageFieldSpec {
  key: string;
  kind: StorageFieldKind;
  required: boolean;
  /** 输入框是否遮蔽(密码 / 密钥)。 */
  masked?: boolean;
  /** i18n key,默认 `storage.field.<key>`。 */
  labelKey?: string;
  placeholder?: string;
  /** 额外说明,i18n key。 */
  hintKey?: string;
}

/** 协议分组,仅用于选择器的视觉分组。 */
export type StorageProtocolGroup = "objectStorage" | "webdav" | "cloudDrive" | "fileShare";

const OBJECT_STORE_SECRETS: StorageFieldSpec[] = [
  { key: "accessKeyId", kind: "secret", required: false },
  { key: "secretAccessKey", kind: "secret", required: false, masked: true },
  { key: "sessionToken", kind: "secret", required: false, masked: true },
];

const ROOT_FIELD: StorageFieldSpec = {
  key: "root",
  kind: "config",
  required: false,
  placeholder: "/",
  hintKey: "storage.hint.root",
};

/** OAuth 服务共用:授权面板负责 token,这里只留下手填与路径前缀。 */
const OAUTH_FIELDS: StorageFieldSpec[] = [
  ROOT_FIELD,
  {
    key: "accessToken",
    kind: "secret",
    required: false,
    masked: true,
    hintKey: "storage.hint.accessToken",
  },
];

const FIELD_SPECS: Record<StorageProtocol, StorageFieldSpec[]> = {
  s3: [
    { key: "bucket", kind: "config", required: true, placeholder: "my-bucket" },
    { key: "region", kind: "config", required: true, placeholder: "us-east-1" },
    { key: "endpoint", kind: "config", required: false },
    ROOT_FIELD,
    ...OBJECT_STORE_SECRETS,
  ],
  s3Compatible: [
    { key: "bucket", kind: "config", required: true, placeholder: "my-bucket" },
    { key: "endpoint", kind: "config", required: true, placeholder: "https://s3.example.com" },
    { key: "region", kind: "config", required: false, placeholder: "us-east-1" },
    ROOT_FIELD,
    ...OBJECT_STORE_SECRETS,
  ],
  aliyunOss: [
    { key: "bucket", kind: "config", required: true, placeholder: "my-bucket" },
    {
      key: "endpoint",
      kind: "config",
      required: true,
      placeholder: "https://oss-cn-hangzhou.aliyuncs.com",
    },
    ROOT_FIELD,
    { key: "accessKeyId", kind: "secret", required: false },
    { key: "secretAccessKey", kind: "secret", required: false, masked: true },
  ],
  tencentCos: [
    { key: "bucket", kind: "config", required: true, placeholder: "my-bucket-1250000000" },
    {
      key: "endpoint",
      kind: "config",
      required: true,
      placeholder: "https://cos.ap-guangzhou.myqcloud.com",
    },
    ROOT_FIELD,
    { key: "accessKeyId", kind: "secret", required: false, labelKey: "storage.field.secretId" },
    {
      key: "secretAccessKey",
      kind: "secret",
      required: false,
      masked: true,
      labelKey: "storage.field.secretKey",
    },
  ],
  jdCloudOss: [
    { key: "bucket", kind: "config", required: true, placeholder: "my-bucket" },
    { key: "endpoint", kind: "config", required: true },
    { key: "region", kind: "config", required: false, placeholder: "cn-north-1" },
    ROOT_FIELD,
    { key: "accessKeyId", kind: "secret", required: false },
    { key: "secretAccessKey", kind: "secret", required: false, masked: true },
  ],
  qiniuKodo: [
    { key: "bucket", kind: "config", required: true, placeholder: "my-bucket" },
    { key: "endpoint", kind: "config", required: true, hintKey: "storage.hint.qiniuEndpoint" },
    { key: "region", kind: "config", required: false, placeholder: "cn-east-1" },
    ROOT_FIELD,
    { key: "accessKeyId", kind: "secret", required: false },
    { key: "secretAccessKey", kind: "secret", required: false, masked: true },
  ],
  upyun: [
    { key: "bucket", kind: "config", required: true, hintKey: "storage.hint.upyunBucket" },
    ROOT_FIELD,
    { key: "operator", kind: "secret", required: true },
    { key: "password", kind: "secret", required: true, masked: true },
  ],
  webdavHttps: [
    {
      key: "endpoint",
      kind: "config",
      required: true,
      placeholder: "https://dav.example.com/remote.php/dav",
      hintKey: "storage.hint.webdavHttps",
    },
    ROOT_FIELD,
    { key: "username", kind: "secret", required: false },
    { key: "password", kind: "secret", required: false, masked: true },
  ],
  webdavHttp: [
    {
      key: "endpoint",
      kind: "config",
      required: true,
      placeholder: "http://192.168.1.10:5005/dav",
      hintKey: "storage.hint.webdavHttp",
    },
    ROOT_FIELD,
    { key: "username", kind: "secret", required: false },
    { key: "password", kind: "secret", required: false, masked: true },
  ],
  dropbox: OAUTH_FIELDS,
  oneDrive: OAUTH_FIELDS,
  googleDrive: OAUTH_FIELDS,
  aliyunDrive: [
    { key: "driveType", kind: "config", required: false, hintKey: "storage.hint.driveType" },
    ...OAUTH_FIELDS,
  ],
  box: OAUTH_FIELDS,
  baiduNetdisk: OAUTH_FIELDS,
  smb: [
    { key: "host", kind: "config", required: true, placeholder: "192.168.1.10" },
    { key: "share", kind: "config", required: true, placeholder: "public" },
    { key: "port", kind: "config", required: false, placeholder: "445" },
    { key: "domain", kind: "config", required: false, placeholder: "WORKGROUP" },
    ROOT_FIELD,
    { key: "username", kind: "secret", required: false },
    { key: "password", kind: "secret", required: false, masked: true },
  ],
  afp: [
    { key: "host", kind: "config", required: true, placeholder: "192.168.1.10" },
    { key: "share", kind: "config", required: true, placeholder: "public" },
    { key: "username", kind: "secret", required: false },
    { key: "password", kind: "secret", required: false, masked: true },
  ],
  nfs: [
    { key: "host", kind: "config", required: true, placeholder: "192.168.1.10" },
    { key: "export", kind: "config", required: true, placeholder: "/volume1/data" },
  ],
};

const PROTOCOL_GROUPS: Record<StorageProtocol, StorageProtocolGroup> = {
  s3: "objectStorage",
  s3Compatible: "objectStorage",
  aliyunOss: "objectStorage",
  tencentCos: "objectStorage",
  jdCloudOss: "objectStorage",
  qiniuKodo: "objectStorage",
  upyun: "objectStorage",
  webdavHttps: "webdav",
  webdavHttp: "webdav",
  dropbox: "cloudDrive",
  oneDrive: "cloudDrive",
  googleDrive: "cloudDrive",
  aliyunDrive: "cloudDrive",
  box: "cloudDrive",
  baiduNetdisk: "cloudDrive",
  smb: "fileShare",
  afp: "fileShare",
  nfs: "fileShare",
};

export const STORAGE_PROTOCOL_GROUP_ORDER: StorageProtocolGroup[] = [
  "objectStorage",
  "webdav",
  "cloudDrive",
  "fileShare",
];

export function storageProtocolGroup(protocol: StorageProtocol): StorageProtocolGroup {
  return PROTOCOL_GROUPS[protocol];
}

/** 字段表。`descriptor` 用于补齐厂商默认端点的占位符。 */
export function storageFieldsForProtocol(
  protocol: StorageProtocol,
  descriptor?: StorageProtocolDescriptor,
): StorageFieldSpec[] {
  const specs = FIELD_SPECS[protocol] ?? [];
  const defaultEndpoint = descriptor?.defaultEndpoint;
  if (!defaultEndpoint) return specs;
  return specs.map((spec) =>
    spec.key === "endpoint" && !spec.placeholder ? { ...spec, placeholder: defaultEndpoint } : spec,
  );
}

export function storageFieldLabelKey(spec: StorageFieldSpec): string {
  return spec.labelKey ?? `storage.field.${spec.key}`;
}

/** 按协议分组的选择器条目。 */
export function groupStorageProtocols(
  descriptors: StorageProtocolDescriptor[],
): Array<{ group: StorageProtocolGroup; descriptors: StorageProtocolDescriptor[] }> {
  return STORAGE_PROTOCOL_GROUP_ORDER.map((group) => ({
    group,
    descriptors: descriptors.filter((item) => storageProtocolGroup(item.protocol) === group),
  })).filter((entry) => entry.descriptors.length > 0);
}

// ---------------------------------------------------------------------------
// 草稿
// ---------------------------------------------------------------------------

export interface StorageConnectionDraft {
  id: string;
  name: string;
  group: string;
  protocol: StorageProtocol;
  config: Record<string, string>;
  /** 用户本次输入的凭据。空字符串表示"保留已保存的值"。 */
  secrets: Record<string, string>;
  createdAt: number;
  lastConnectedAt?: number;
}

export type StorageDraftErrors = Record<string, string>;

export function newStorageConnectionId(): string {
  return `storage-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function emptyStorageDraft(
  protocol: StorageProtocol = "s3",
  group = "",
): StorageConnectionDraft {
  return {
    id: newStorageConnectionId(),
    name: "",
    group,
    protocol,
    config: {},
    secrets: {},
    createdAt: 0,
  };
}

export function storageDraftFromConnection(
  connection: StorageConnection | null | undefined,
  protocol: StorageProtocol = "s3",
  group = "",
): StorageConnectionDraft {
  if (!connection) return emptyStorageDraft(protocol, group);
  return {
    id: connection.id,
    name: connection.name,
    group: connection.group ?? "",
    protocol: connection.protocol,
    config: { ...connection.config },
    // 凭据从不回传前端,草稿里永远从空开始。
    secrets: {},
    createdAt: connection.createdAt,
    lastConnectedAt: connection.lastConnectedAt,
  };
}

/** 切换协议时保留仍然有用的字段,丢掉不属于新协议的输入。 */
export function switchStorageDraftProtocol(
  draft: StorageConnectionDraft,
  protocol: StorageProtocol,
): StorageConnectionDraft {
  const nextConfigKeys = new Set(
    storageFieldsForProtocol(protocol)
      .filter((spec) => spec.kind === "config")
      .map((spec) => spec.key),
  );
  const nextSecretKeys = new Set(
    storageFieldsForProtocol(protocol)
      .filter((spec) => spec.kind === "secret")
      .map((spec) => spec.key),
  );
  const config: Record<string, string> = {};
  for (const [key, value] of Object.entries(draft.config)) {
    if (nextConfigKeys.has(key)) config[key] = value;
  }
  const secrets: Record<string, string> = {};
  for (const [key, value] of Object.entries(draft.secrets)) {
    if (nextSecretKeys.has(key)) secrets[key] = value;
  }
  return { ...draft, protocol, config, secrets };
}

function isBlank(value: string | undefined): boolean {
  return !value || !value.trim();
}

/**
 * 校验草稿。
 *
 * `savedSecretKeys` 是该连接已经落盘的凭据键;必填凭据只要已保存过就允许留空
 * (编辑时表单不回显凭据值)。
 */
export function validateStorageDraft(
  draft: StorageConnectionDraft,
  savedSecretKeys: string[] = [],
): StorageDraftErrors {
  const errors: StorageDraftErrors = {};
  if (isBlank(draft.name)) errors.name = "storage.error.nameRequired";

  const saved = new Set(savedSecretKeys);
  for (const spec of storageFieldsForProtocol(draft.protocol)) {
    const value = spec.kind === "config" ? draft.config[spec.key] : draft.secrets[spec.key];
    if (spec.required && isBlank(value) && !(spec.kind === "secret" && saved.has(spec.key))) {
      errors[spec.key] = "storage.error.fieldRequired";
      continue;
    }
    if (isBlank(value)) continue;
    if (spec.key === "port") {
      const port = Number(value.trim());
      if (!Number.isInteger(port) || port < 1 || port > 65535) {
        errors.port = "storage.error.invalidPort";
      }
      continue;
    }
    if (spec.key === "endpoint") {
      const endpointError = validateEndpoint(draft.protocol, value.trim());
      if (endpointError) errors.endpoint = endpointError;
      continue;
    }
    if (spec.key === "export" && !value.trim().startsWith("/")) {
      errors.export = "storage.error.absolutePathRequired";
      continue;
    }
    if (spec.key === "host" && /[\s/\\]/.test(value.trim())) {
      errors.host = "storage.error.invalidHost";
    }
  }
  return errors;
}

function validateEndpoint(protocol: StorageProtocol, endpoint: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(endpoint);
  } catch {
    return "storage.error.invalidEndpoint";
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return "storage.error.invalidEndpoint";
  }
  if (protocol === "webdavHttps" && parsed.protocol !== "https:") {
    return "storage.error.httpsRequired";
  }
  if (protocol === "webdavHttp" && parsed.protocol !== "http:") {
    return "storage.error.httpRequired";
  }
  return null;
}

/** 草稿 → 可提交给 `storage_save_connection` 的连接。 */
export function normalizeStorageDraft(
  draft: StorageConnectionDraft,
  now = Date.now(),
): StorageConnection {
  const specs = storageFieldsForProtocol(draft.protocol);
  const configKeys = new Set(specs.filter((s) => s.kind === "config").map((s) => s.key));
  const secretKeys = new Set(specs.filter((s) => s.kind === "secret").map((s) => s.key));

  const config: Record<string, string> = {};
  for (const [key, value] of Object.entries(draft.config)) {
    if (!configKeys.has(key)) continue;
    const trimmed = value.trim();
    if (trimmed) config[key] = trimmed;
  }
  const secrets: Record<string, string> = {};
  for (const [key, value] of Object.entries(draft.secrets)) {
    // OAuth 授权写回的键(refreshToken / expiresAtMs 等)不在表单字段里,一并保留。
    if (!secretKeys.has(key) && !OAUTH_WRITEBACK_KEYS.has(key)) continue;
    const trimmed = value.trim();
    if (trimmed) secrets[key] = trimmed;
  }

  const group = draft.group.trim();
  return {
    id: draft.id,
    name: draft.name.trim(),
    ...(group ? { group } : {}),
    protocol: draft.protocol,
    config,
    ...(Object.keys(secrets).length > 0 ? { secrets } : {}),
    createdAt: draft.createdAt > 0 ? draft.createdAt : now,
    ...(draft.lastConnectedAt ? { lastConnectedAt: draft.lastConnectedAt } : {}),
  };
}

/** OAuth 授权结果里可能出现、但不作为表单字段展示的凭据键。 */
const OAUTH_WRITEBACK_KEYS = new Set([
  "accessToken",
  "refreshToken",
  "clientId",
  "clientSecret",
  "expiresAtMs",
]);

/** 连接摘要,用于卡片与下拉框的第二行说明。 */
export function storageConnectionSummary(connection: StorageConnection): string {
  const { config, protocol } = connection;
  switch (protocol) {
    case "s3":
    case "s3Compatible":
    case "aliyunOss":
    case "tencentCos":
    case "jdCloudOss":
    case "qiniuKodo":
    case "upyun":
      return [config.bucket, config.region || config.endpoint].filter(Boolean).join(" · ");
    case "webdavHttps":
    case "webdavHttp":
      return config.endpoint ?? "";
    case "smb":
    case "afp":
      return [config.host, config.share].filter(Boolean).join("/");
    case "nfs":
      return [config.host, config.export].filter(Boolean).join(":");
    default:
      return config.root ?? "/";
  }
}
