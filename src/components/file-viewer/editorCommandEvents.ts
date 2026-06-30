export const FILE_VIEWER_COMMAND_EVENT = "aeroric:file-viewer-command";

export const FILE_VIEWER_COMMANDS = ["findReferences", "renameSymbol", "quickFix"] as const;

export type FileViewerCommand = (typeof FILE_VIEWER_COMMANDS)[number];

export function isFileViewerCommand(value: unknown): value is FileViewerCommand {
  return (
    typeof value === "string" &&
    FILE_VIEWER_COMMANDS.includes(value as FileViewerCommand)
  );
}

export function dispatchFileViewerCommand(command: FileViewerCommand): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(FILE_VIEWER_COMMAND_EVENT, { detail: { command } }));
}
