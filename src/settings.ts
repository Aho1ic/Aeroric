export const DEFAULT_SFTP_LOCAL_PATH = "/Users/macbook/Downloads/同步空间";
export const SFTP_LOCAL_PATH_STORAGE_KEY = "aeroric:sftpLocalDefaultPath";

export function normalizeSftpLocalDefaultPath(value: unknown): string {
  return typeof value === "string" && value.trim() ? value.trim() : DEFAULT_SFTP_LOCAL_PATH;
}

export function getInitialSftpLocalDefaultPath(): string {
  return normalizeSftpLocalDefaultPath(localStorage.getItem(SFTP_LOCAL_PATH_STORAGE_KEY));
}
