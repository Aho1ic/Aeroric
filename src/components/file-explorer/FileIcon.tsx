import {
  Archive,
  Box,
  Code2,
  Database,
  File,
  FileImage,
  FileJson,
  FileText,
  Folder,
  Package,
  Cpu,
  Video,
} from "lucide-react";
import s from "../../styles";
import { getFileColor } from "../../utils";
import { GITIGNORED_COLOR } from "./types";
import { fileIconKind } from "./fileEntryUtils";

export function FileIcon({
  name,
  ext,
  isDir,
  expanded,
  isGitignored,
}: {
  name: string;
  ext?: string;
  isDir: boolean;
  expanded?: boolean;
  isGitignored?: boolean;
}) {
  if (isDir) {
    const folderColor = isGitignored ? GITIGNORED_COLOR : "var(--icon-folder)";
    return (
      <span style={{ ...s.fileIconFolder, color: folderColor }}>
        <Folder size={14} fill={expanded ? "currentColor" : "none"} strokeWidth={1.8} />
      </span>
    );
  }
  const color = isGitignored ? GITIGNORED_COLOR : getFileColor(name, ext);
  const iconProps = { size: 14, strokeWidth: 1.8 };
  const kind = fileIconKind({ name, extension: ext, is_dir: false });
  return (
    <span style={{ ...s.fileIconFile, color, background: "transparent" }} data-kind={kind}>
      {kind === "database" && <Database {...iconProps} />}
      {kind === "model" && <Cpu {...iconProps} />}
      {kind === "video" && <Video {...iconProps} />}
      {kind === "package" && <Package {...iconProps} />}
      {kind === "image" && <FileImage {...iconProps} />}
      {kind === "markdown" && <FileText {...iconProps} />}
      {kind === "json" && <FileJson {...iconProps} />}
      {kind === "archive" && <Archive {...iconProps} />}
      {kind === "code" && <Code2 {...iconProps} />}
      {kind === "text" && <FileText {...iconProps} />}
      {kind === "file" && <File {...iconProps} />}
      {kind === "folder" && <Box {...iconProps} />}
    </span>
  );
}
