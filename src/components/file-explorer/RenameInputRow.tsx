import s from "../../styles";
import { FileIcon } from "./FileIcon";
import type { TreeNode } from "./types";

export function RenameInputRow({
  node,
  depth,
  value,
  onChange,
  onCommit,
  onCancel,
  inputRef,
}: {
  node: TreeNode;
  depth: number;
  value: string;
  onChange: (next: string) => void;
  onCommit: () => void;
  onCancel: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
}) {
  return (
    <div
      style={{ ...s.fileTreeCreateRow, paddingLeft: 8 + depth * 14 }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <span style={s.fileTreeChevronSpacer} />
      <FileIcon
        name={value || node.name}
        ext={node.extension}
        isDir={node.is_dir}
        expanded={node.expanded}
        isGitignored={node.is_gitignored}
      />
      <input
        ref={inputRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            onCommit();
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        onBlur={onCancel}
        spellCheck={false}
        autoComplete="off"
        style={s.fileTreeCreateInput}
      />
    </div>
  );
}
