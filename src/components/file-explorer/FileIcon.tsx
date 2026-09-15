import {
  AudioLines,
  Binary,
  BookOpen,
  BookText,
  Braces,
  Container,
  Cpu,
  Database,
  File,
  FileCode2,
  FileDiff,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileType,
  Film,
  Folder,
  FolderArchive,
  FolderCode,
  FolderCog,
  FolderGit2,
  FolderOpen,
  FolderTree,
  FlaskConical,
  GitBranch,
  Globe,
  Key,
  Link2,
  Lock,
  Notebook,
  Package,
  Palette,
  Presentation,
  ScrollText,
  Settings2,
  Terminal,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { ComponentProps } from "react";
import s from "../../styles";
import { GITIGNORED_ICON_COLOR, entryIconOf, type FileIconKind } from "../../lib/fileIcons";

/**
 * kind → lucide 字形。颜色不在这里 —— 它随 kind 一起来自 `lib/fileIcons` 的同一张表。
 *
 * 目录的展开态在 `EXPANDED_GLYPHS` 里覆写:只有通用文件夹和「代码目录」有对应的
 * 打开态字形,其余保持关闭态(打开一个 `node_modules` 换不出更有信息量的形状)。
 */
const GLYPHS: Record<FileIconKind, LucideIcon> = {
  folder: Folder,
  "folder-code": FolderCode,
  "folder-test": FlaskConical,
  "folder-config": FolderCog,
  "folder-dependency": FolderTree,
  "folder-git": FolderGit2,
  "folder-dist": FolderArchive,
  "folder-docs": BookText,
  "folder-assets": FileImage,
  symlink: Link2,
  code: FileCode2,
  markup: Globe,
  style: Palette,
  data: Braces,
  config: Settings2,
  shell: Terminal,
  docker: Container,
  git: GitBranch,
  build: Wrench,
  manifest: Package,
  lock: Lock,
  secret: Key,
  database: Database,
  model: Cpu,
  notebook: Notebook,
  diff: FileDiff,
  license: ScrollText,
  readme: BookText,
  markdown: FileText,
  document: BookOpen,
  spreadsheet: FileSpreadsheet,
  presentation: Presentation,
  image: FileImage,
  vector: Palette,
  video: Film,
  audio: AudioLines,
  font: FileType,
  archive: FolderArchive,
  package: Package,
  binary: Binary,
  text: FileText,
  file: File,
};

const EXPANDED_GLYPHS: Partial<Record<FileIconKind, LucideIcon>> = {
  folder: FolderOpen,
};

/**
 * 文件树 / 搜索结果 / 拖拽预览 / 新建输入行共用的条目图标。
 *
 * 字形与颜色都来自 `lib/fileIcons` 的单一查表(Material Icon Theme 的查表优先级,
 * lucide 的字形)。gitignore 掉的条目保留字形、只把颜色压暗 —— 形状仍然要能读。
 */
export function FileIcon({
  name,
  ext,
  isDir,
  isSymlink,
  expanded,
  isGitignored,
}: {
  name: string;
  ext?: string;
  isDir: boolean;
  isSymlink?: boolean;
  expanded?: boolean;
  isGitignored?: boolean;
}) {
  const { kind, color } = entryIconOf({
    name,
    extension: ext,
    is_dir: isDir,
    is_symlink: isSymlink,
  });
  const Glyph = (expanded ? EXPANDED_GLYPHS[kind] : undefined) ?? GLYPHS[kind];
  const iconProps: ComponentProps<LucideIcon> = { size: 14, strokeWidth: 1.8 };
  const wrapperStyle = isDir && !isSymlink ? s.fileIconFolder : s.fileIconFile;
  return (
    <span
      style={{
        ...wrapperStyle,
        color: isGitignored ? GITIGNORED_ICON_COLOR : color,
        background: "transparent",
      }}
      data-kind={kind}
    >
      <Glyph {...iconProps} />
    </span>
  );
}
