import { parentPathOf } from "./treeUtils";
import type { SshConnection } from "../../types";
import type { SftpTauriEndpoint } from "../sftp/sftpTypes";

export type FileExplorerRemoteContext = {
  connection: SshConnection;
  projectPath: string;
};

export type FileExplorerKeyAction = "copyPath" | "rename" | "paste" | "delete" | "preview" | null;
export type FileExplorerClickAction = "select" | "toggle" | "selectAndToggle";

export function fileExplorerKeyAction(event: {
  key: string;
  code?: string;
  metaKey?: boolean;
  altKey?: boolean;
  ctrlKey?: boolean;
}): FileExplorerKeyAction {
  const key = event.key.toLowerCase();
  const code = event.code?.toLowerCase();
  if (!event.metaKey && !event.ctrlKey && !event.altKey && (key === " " || code === "space")) {
    return "preview";
  }
  if (event.metaKey && event.altKey && (key === "c" || code === "keyc")) return "copyPath";
  if (event.metaKey && key === "v") return "paste";
  if (event.metaKey && (event.key === "Backspace" || event.key === "Delete")) return "delete";
  if (!event.metaKey && !event.ctrlKey && !event.altKey && event.key === "Enter") return "rename";
  return null;
}

export function pasteTargetDirectory({
  selectedPath,
  selectedIsDir,
  rootPath,
}: {
  selectedPath: string | null;
  selectedIsDir: boolean;
  rootPath: string;
}): string {
  if (!selectedPath) return rootPath;
  return selectedIsDir ? selectedPath : parentPathOf(selectedPath);
}

export function fileExplorerClickAction({
  isDir,
  isSelected,
}: {
  isDir: boolean;
  isSelected: boolean;
}): FileExplorerClickAction {
  if (!isDir) return "select";
  return isSelected ? "toggle" : "selectAndToggle";
}

export function fileExplorerPreviewEndpoint({
  selectedPath,
  remote,
}: {
  selectedPath: string | null;
  remote?: FileExplorerRemoteContext;
}): SftpTauriEndpoint | null {
  if (!selectedPath) return null;
  if (!remote) return { kind: "local", path: selectedPath };
  return { kind: "ssh", connection: remote.connection, path: selectedPath };
}
