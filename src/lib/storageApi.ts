/**
 * 存储连接的 Tauri 命令封装。所有凭据都由后端从 0600 的 secrets 文件读取,
 * 前端只在"用户刚填写/刚授权"时把凭据传给 `storageSaveConnection` 一次。
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  StorageAuthorizationResult,
  StorageCapability,
  StorageConnection,
  StorageCredentialOptions,
  StorageCredentialSource,
  StorageProtocol,
  StorageProtocolDescriptor,
} from "../types/storage";

export const storageApi = {
  /** 列出连接(不含凭据值)。 */
  listConnections(): Promise<StorageConnection[]> {
    return invoke<StorageConnection[]>("storage_list_connections");
  },

  /** 每条连接已保存的凭据键名(不含值),用于表单显示"已保存"。 */
  secretKeys(): Promise<Record<string, string[]>> {
    return invoke<Record<string, string[]>>("storage_secret_keys");
  },

  /** 新增或更新。同协议下 `secrets` 留空会保留已保存值；切换协议会清除旧凭据。 */
  saveConnection(connection: StorageConnection): Promise<StorageConnection[]> {
    return invoke<StorageConnection[]>("storage_save_connection", { connection });
  },

  deleteConnection(connectionId: string): Promise<StorageConnection[]> {
    return invoke<StorageConnection[]>("storage_delete_connection", { connectionId });
  },

  touchConnection(connectionId: string, timestamp = Date.now()): Promise<void> {
    return invoke<void>("storage_touch_connection", { connectionId, timestamp });
  },

  /** 全部 18 种协议的元信息。 */
  protocols(): Promise<StorageProtocolDescriptor[]> {
    return invoke<StorageProtocolDescriptor[]>("storage_protocols");
  },

  capabilities(connectionId: string): Promise<StorageCapability> {
    return invoke<StorageCapability>("storage_capabilities", { connectionId });
  },

  /** 真正建一次后端并列一次根目录。 */
  testConnection(connectionId: string): Promise<void> {
    return invoke<void>("storage_test_connection", { connectionId });
  },

  /** 卸载 AFP / NFS 挂载点。 */
  unmountConnection(connectionId: string): Promise<void> {
    return invoke<void>("storage_unmount_connection", { connectionId });
  },

  /** 走完 OAuth 流程(系统浏览器 + 回环回调),返回待写入的 secrets。 */
  oauthAuthorize(params: {
    protocol: StorageProtocol;
    source: StorageCredentialSource;
    clientId?: string;
    clientSecret?: string;
  }): Promise<StorageAuthorizationResult> {
    return invoke<StorageAuthorizationResult>("storage_oauth_authorize", {
      protocol: params.protocol,
      source: params.source,
      clientId: params.clientId ?? null,
      clientSecret: params.clientSecret ?? null,
    });
  },

  oauthCredentialOptions(protocol: StorageProtocol): Promise<StorageCredentialOptions | null> {
    return invoke<StorageCredentialOptions | null>("storage_oauth_credential_options", {
      protocol,
    });
  },
};
