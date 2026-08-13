/**
 * 对象存储 / 网盘 / 文件共享连接的前端类型,与 `src-tauri/src/storage_conn.rs`
 * 及 `storage_backend/mod.rs` 的 serde 表示一一对应。
 */

export const STORAGE_PROTOCOLS = [
  "s3",
  "s3Compatible",
  "aliyunOss",
  "tencentCos",
  "jdCloudOss",
  "qiniuKodo",
  "upyun",
  "webdavHttps",
  "webdavHttp",
  "dropbox",
  "oneDrive",
  "googleDrive",
  "aliyunDrive",
  "box",
  "baiduNetdisk",
  "smb",
  "afp",
  "nfs",
] as const;

export type StorageProtocol = (typeof STORAGE_PROTOCOLS)[number];

/** 后端能力位。前端据此禁用不支持的动作,而不是等后端报错。 */
export interface StorageCapability {
  readDir: boolean;
  read: boolean;
  write: boolean;
  createDir: boolean;
  delete: boolean;
  rename: boolean;
  copy: boolean;
  stat: boolean;
  richMetadata: boolean;
}

/** 单个协议的元信息,驱动协议选择器与动态表单。 */
export interface StorageProtocolDescriptor {
  protocol: StorageProtocol;
  /** 英文兜底展示名;用户可见文案优先走 i18n。 */
  displayName: string;
  capability: StorageCapability;
  requiredConfigKeys: string[];
  secretKeys: string[];
  defaultEndpoint: string | null;
  oauth: boolean;
  systemMount: boolean;
  deprecated: boolean;
}

/**
 * 一条存储连接。
 *
 * `storage_list_connections` 返回的对象**不含** `secrets`:凭据存在单独的 0600
 * 文件里,前端只能通过 `storage_secret_keys` 知道某个键是否已设置。
 * 保存时 `secrets` 里的空字符串表示"保留原值"。
 */
export interface StorageConnection {
  id: string;
  name: string;
  group?: string;
  protocol: StorageProtocol;
  config: Record<string, string>;
  secrets?: Record<string, string>;
  createdAt: number;
  lastConnectedAt?: number;
}

/** OAuth 凭据来源:内置公共应用(仅 PKCE)或用户自建应用。 */
export type StorageCredentialSource = "builtin" | "userProvided";

export interface StorageCredentialOptions {
  builtinAvailable: boolean;
  requiresClientSecret: boolean;
  supportsPkce: boolean;
  scope: string;
}

export interface StorageAuthorizationResult {
  secrets: Record<string, string>;
}
