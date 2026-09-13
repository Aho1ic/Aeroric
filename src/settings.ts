/**
 * 空串表示"跟随当前用户 home":后端 `sftp_read_dir` 对空路径/`~` 前缀
 * 落到 `platform::home_dir()`。不要硬编码任何具体机器的目录。
 */
export const DEFAULT_SFTP_LOCAL_PATH = "";
export const SFTP_LOCAL_PATH_STORAGE_KEY = "aeroric:sftpLocalDefaultPath";

export function normalizeSftpLocalDefaultPath(value: unknown): string {
  return typeof value === "string" && value.trim() ? value.trim() : DEFAULT_SFTP_LOCAL_PATH;
}

export function getInitialSftpLocalDefaultPath(): string {
  return normalizeSftpLocalDefaultPath(localStorage.getItem(SFTP_LOCAL_PATH_STORAGE_KEY));
}
