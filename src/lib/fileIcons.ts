/**
 * 文件/目录 → 图标的单一查表。
 *
 * 借鉴 Material Icon Theme(vscode-material-icon-theme,MIT)的**查表优先级**,
 * 而不是它的素材:那个包要 glob 1.2k 个 SVG 进 bundle(主包 +~350 KB、素材 ~1 MB),
 * 而 Aeroric 全站图标走 lucide-react。所以这里照搬的是它的语义 ——
 *
 *   精确文件名 > 名字前缀族(`dockerfile.*` / `.env.*`) > 后缀 > 兜底
 *
 * 一条 entry 同时给出**字形**(lucide 组件由 `FileIcon` 按 kind 选)和**颜色**
 * (CSS token)。合成一张表是重点:此前 `fileIconKind` 和 `getFileColor` 各有一张
 * 互不相干的表,于是出现过 `.wasm` 有专属颜色但字形是通用文件、`Makefile` 有 build
 * 颜色但字形是通用文件、`.env` 有 config 颜色而字形按后缀 `env` 落到 text 这类
 * 「颜色和字形各说各话」。现在两个函数都从这里取值,不可能再分叉。
 *
 * 目录与符号链接各有独立入口和兜底,见 `folderIconOf` / `SYMLINK_ICON`。
 */

import { fileExtensionOf } from "./fileExtensions";

/** 字形分组。lucide 给不出「每种语言一个 logo」,语言身份由颜色承担。 */
export type FileIconKind =
  | "folder"
  | "folder-code"
  | "folder-test"
  | "folder-config"
  | "folder-dependency"
  | "folder-git"
  | "folder-dist"
  | "folder-docs"
  | "folder-assets"
  | "symlink"
  | "code"
  | "markup"
  | "style"
  | "data"
  | "config"
  | "shell"
  | "docker"
  | "git"
  | "build"
  | "manifest"
  | "lock"
  | "secret"
  | "database"
  | "model"
  | "notebook"
  | "diff"
  | "license"
  | "readme"
  | "markdown"
  | "document"
  | "spreadsheet"
  | "presentation"
  | "image"
  | "vector"
  | "video"
  | "audio"
  | "font"
  | "archive"
  | "package"
  | "binary"
  | "text"
  | "file";

export interface FileIcon {
  kind: FileIconKind;
  /** CSS 变量引用,直接进 `color`。 */
  color: string;
}

const icon = (kind: FileIconKind, color: string): FileIcon => ({ kind, color });

/** 未知后缀、无后缀、后端没给 extension —— 都落这里。 */
export const DEFAULT_FILE_ICON: FileIcon = icon("file", "var(--icon-file-default)");
/** 目录兜底(展开态字形由 `FileIcon` 决定,颜色同源)。 */
export const DEFAULT_FOLDER_ICON: FileIcon = icon("folder", "var(--icon-folder)");
/** 符号链接自身。优先于 `is_dir`,所以指向目录的链接也画成链接。 */
export const SYMLINK_ICON: FileIcon = icon("symlink", "var(--icon-file-symlink)");
/** gitignore 掉的条目:字形照常,颜色统一压暗。 */
export const GITIGNORED_ICON_COLOR = "var(--icon-file-ignored)";

/** 精确文件名(小写比较)。优先于一切后缀规则。 */
const BY_FILE_NAME: Record<string, FileIcon> = {
  dockerfile: icon("docker", "var(--icon-file-docker)"),
  "docker-compose.yml": icon("docker", "var(--icon-file-docker)"),
  "docker-compose.yaml": icon("docker", "var(--icon-file-docker)"),
  "compose.yml": icon("docker", "var(--icon-file-docker)"),
  "compose.yaml": icon("docker", "var(--icon-file-docker)"),
  ".dockerignore": icon("docker", "var(--icon-file-docker)"),

  ".gitignore": icon("git", "var(--icon-file-git)"),
  ".gitattributes": icon("git", "var(--icon-file-git)"),
  ".gitmodules": icon("git", "var(--icon-file-git)"),
  ".gitkeep": icon("git", "var(--icon-file-git)"),

  makefile: icon("build", "var(--icon-file-build)"),
  gnumakefile: icon("build", "var(--icon-file-build)"),
  justfile: icon("build", "var(--icon-file-build)"),
  "cmakelists.txt": icon("build", "var(--icon-file-build)"),
  "build.gradle": icon("build", "var(--icon-file-build)"),
  "build.gradle.kts": icon("build", "var(--icon-file-build)"),
  "meson.build": icon("build", "var(--icon-file-build)"),
  "vite.config.ts": icon("build", "var(--icon-file-build)"),
  "vite.config.js": icon("build", "var(--icon-file-build)"),

  "package.json": icon("manifest", "var(--icon-file-manifest)"),
  "cargo.toml": icon("manifest", "var(--icon-file-manifest)"),
  "pyproject.toml": icon("manifest", "var(--icon-file-manifest)"),
  "go.mod": icon("manifest", "var(--icon-file-manifest)"),
  gemfile: icon("manifest", "var(--icon-file-ruby)"),
  rakefile: icon("build", "var(--icon-file-ruby)"),
  "composer.json": icon("manifest", "var(--icon-file-manifest)"),
  "pubspec.yaml": icon("manifest", "var(--icon-file-manifest)"),

  "package-lock.json": icon("lock", "var(--icon-file-lock)"),
  "pnpm-lock.yaml": icon("lock", "var(--icon-file-lock)"),
  "yarn.lock": icon("lock", "var(--icon-file-lock)"),
  "cargo.lock": icon("lock", "var(--icon-file-lock)"),
  "go.sum": icon("lock", "var(--icon-file-lock)"),
  "poetry.lock": icon("lock", "var(--icon-file-lock)"),
  "uv.lock": icon("lock", "var(--icon-file-lock)"),
  "composer.lock": icon("lock", "var(--icon-file-lock)"),
  "gemfile.lock": icon("lock", "var(--icon-file-lock)"),

  ".editorconfig": icon("config", "var(--icon-file-config)"),
  ".npmrc": icon("config", "var(--icon-file-config)"),
  ".nvmrc": icon("config", "var(--icon-file-config)"),
  ".prettierrc": icon("config", "var(--icon-file-config)"),
  ".eslintrc": icon("config", "var(--icon-file-config)"),
  ".browserslistrc": icon("config", "var(--icon-file-config)"),
  ".gitconfig": icon("config", "var(--icon-file-git)"),

  license: icon("license", "var(--icon-file-license)"),
  "license.md": icon("license", "var(--icon-file-license)"),
  "license.txt": icon("license", "var(--icon-file-license)"),
  licence: icon("license", "var(--icon-file-license)"),
  copying: icon("license", "var(--icon-file-license)"),
  notice: icon("license", "var(--icon-file-license)"),

  "readme.md": icon("readme", "var(--icon-file-md)"),
  readme: icon("readme", "var(--icon-file-md)"),
  "readme.txt": icon("readme", "var(--icon-file-md)"),
  "changelog.md": icon("readme", "var(--icon-file-md)"),
  "contributing.md": icon("readme", "var(--icon-file-md)"),
};

/**
 * 名字前缀族。精确名之后、后缀之前,按声明顺序取第一条命中。
 *
 * 存在的理由:`Dockerfile.prod` / `.env.production` 这类「基名 + 任意尾巴」既进不了
 * 精确表(尾巴无穷多),又不能靠后缀(后缀是 `prod` / `production`)。
 */
const BY_NAME_PREFIX: readonly (readonly [string, FileIcon])[] = [
  ["dockerfile.", icon("docker", "var(--icon-file-docker)")],
  [".env.", icon("config", "var(--icon-file-config)")],
  [".env", icon("config", "var(--icon-file-config)")],
  [".docker", icon("docker", "var(--icon-file-docker)")],
  [".git", icon("git", "var(--icon-file-git)")],
];

/** 后缀 -> 图标。键是单段;后端报的 `extension` 也永远是单段。 */
const BY_EXTENSION: Record<string, FileIcon> = {
  // ── 代码 ────────────────────────────────────────────────────────────────
  ts: icon("code", "var(--icon-file-ts)"),
  tsx: icon("code", "var(--icon-file-ts)"),
  mts: icon("code", "var(--icon-file-ts)"),
  cts: icon("code", "var(--icon-file-ts)"),
  js: icon("code", "var(--icon-file-js)"),
  jsx: icon("code", "var(--icon-file-js)"),
  mjs: icon("code", "var(--icon-file-js)"),
  cjs: icon("code", "var(--icon-file-js)"),
  rs: icon("code", "var(--icon-file-rust)"),
  py: icon("code", "var(--icon-file-python)"),
  pyi: icon("code", "var(--icon-file-python)"),
  go: icon("code", "var(--icon-file-go)"),
  java: icon("code", "var(--icon-file-java)"),
  kt: icon("code", "var(--icon-file-kotlin)"),
  kts: icon("code", "var(--icon-file-kotlin)"),
  swift: icon("code", "var(--icon-file-swift)"),
  rb: icon("code", "var(--icon-file-ruby)"),
  php: icon("code", "var(--icon-file-php)"),
  c: icon("code", "var(--icon-file-c)"),
  h: icon("code", "var(--icon-file-c)"),
  cc: icon("code", "var(--icon-file-c)"),
  cpp: icon("code", "var(--icon-file-c)"),
  cxx: icon("code", "var(--icon-file-c)"),
  hpp: icon("code", "var(--icon-file-c)"),
  cs: icon("code", "var(--icon-file-csharp)"),
  dart: icon("code", "var(--icon-file-dart)"),
  lua: icon("code", "var(--icon-file-lua)"),
  vue: icon("code", "var(--icon-file-vue)"),
  svelte: icon("code", "var(--icon-file-svelte)"),
  ex: icon("code", "var(--icon-file-elixir)"),
  exs: icon("code", "var(--icon-file-elixir)"),
  hs: icon("code", "var(--icon-file-haskell)"),
  zig: icon("code", "var(--icon-file-zig)"),

  // ── 标记 / 样式 ─────────────────────────────────────────────────────────
  html: icon("markup", "var(--icon-file-html)"),
  htm: icon("markup", "var(--icon-file-html)"),
  xml: icon("markup", "var(--icon-file-xml)"),
  svg: icon("vector", "var(--icon-file-svg)"),
  css: icon("style", "var(--icon-file-css)"),
  scss: icon("style", "var(--icon-file-css)"),
  sass: icon("style", "var(--icon-file-css)"),
  less: icon("style", "var(--icon-file-css)"),

  // ── 数据 / 配置 ─────────────────────────────────────────────────────────
  json: icon("data", "var(--icon-file-json)"),
  jsonc: icon("data", "var(--icon-file-json)"),
  json5: icon("data", "var(--icon-file-json)"),
  yaml: icon("data", "var(--icon-file-yaml)"),
  yml: icon("data", "var(--icon-file-yaml)"),
  toml: icon("data", "var(--icon-file-toml)"),
  csv: icon("spreadsheet", "var(--icon-file-spreadsheet)"),
  tsv: icon("spreadsheet", "var(--icon-file-spreadsheet)"),
  xlsx: icon("spreadsheet", "var(--icon-file-spreadsheet)"),
  xls: icon("spreadsheet", "var(--icon-file-spreadsheet)"),
  ini: icon("config", "var(--icon-file-config)"),
  cfg: icon("config", "var(--icon-file-config)"),
  conf: icon("config", "var(--icon-file-config)"),
  properties: icon("config", "var(--icon-file-config)"),
  lock: icon("lock", "var(--icon-file-lock)"),
  env: icon("config", "var(--icon-file-config)"),
  pem: icon("secret", "var(--icon-file-secret)"),
  key: icon("secret", "var(--icon-file-secret)"),
  crt: icon("secret", "var(--icon-file-secret)"),
  p12: icon("secret", "var(--icon-file-secret)"),

  // ── 脚本 ────────────────────────────────────────────────────────────────
  sh: icon("shell", "var(--icon-file-shell)"),
  bash: icon("shell", "var(--icon-file-shell)"),
  zsh: icon("shell", "var(--icon-file-shell)"),
  fish: icon("shell", "var(--icon-file-shell)"),
  ps1: icon("shell", "var(--icon-file-powershell)"),
  bat: icon("shell", "var(--icon-file-powershell)"),
  cmd: icon("shell", "var(--icon-file-powershell)"),

  // ── 文档 ────────────────────────────────────────────────────────────────
  md: icon("markdown", "var(--icon-file-md)"),
  mdx: icon("markdown", "var(--icon-file-md)"),
  rst: icon("markdown", "var(--icon-file-md)"),
  adoc: icon("markdown", "var(--icon-file-md)"),
  pdf: icon("document", "var(--icon-file-pdf)"),
  epub: icon("document", "var(--icon-file-pdf)"),
  docx: icon("document", "var(--icon-file-document)"),
  doc: icon("document", "var(--icon-file-document)"),
  rtf: icon("document", "var(--icon-file-document)"),
  pptx: icon("presentation", "var(--icon-file-presentation)"),
  ppt: icon("presentation", "var(--icon-file-presentation)"),
  ipynb: icon("notebook", "var(--icon-file-notebook)"),

  // ── 补丁 ────────────────────────────────────────────────────────────────
  diff: icon("diff", "var(--icon-file-diff)"),
  patch: icon("diff", "var(--icon-file-diff)"),

  // ── 数据库 ──────────────────────────────────────────────────────────────
  sql: icon("database", "var(--icon-file-sql)"),
  db: icon("database", "var(--icon-file-sql)"),
  sqlite: icon("database", "var(--icon-file-sql)"),
  sqlite3: icon("database", "var(--icon-file-sql)"),

  // ── 模型权重 ────────────────────────────────────────────────────────────
  pt: icon("model", "var(--icon-file-model)"),
  pth: icon("model", "var(--icon-file-model)"),
  onnx: icon("model", "var(--icon-file-model)"),
  safetensors: icon("model", "var(--icon-file-model)"),
  gguf: icon("model", "var(--icon-file-model)"),

  // ── 媒体 ────────────────────────────────────────────────────────────────
  png: icon("image", "var(--icon-file-image)"),
  jpg: icon("image", "var(--icon-file-image)"),
  jpeg: icon("image", "var(--icon-file-image)"),
  gif: icon("image", "var(--icon-file-image)"),
  webp: icon("image", "var(--icon-file-image)"),
  bmp: icon("image", "var(--icon-file-image)"),
  ico: icon("image", "var(--icon-file-image)"),
  avif: icon("image", "var(--icon-file-image)"),
  mp4: icon("video", "var(--icon-file-video)"),
  mov: icon("video", "var(--icon-file-video)"),
  mkv: icon("video", "var(--icon-file-video)"),
  avi: icon("video", "var(--icon-file-video)"),
  webm: icon("video", "var(--icon-file-video)"),
  mp3: icon("audio", "var(--icon-file-audio)"),
  wav: icon("audio", "var(--icon-file-audio)"),
  flac: icon("audio", "var(--icon-file-audio)"),
  ogg: icon("audio", "var(--icon-file-audio)"),
  m4a: icon("audio", "var(--icon-file-audio)"),
  ttf: icon("font", "var(--icon-file-font)"),
  otf: icon("font", "var(--icon-file-font)"),
  woff: icon("font", "var(--icon-file-font)"),
  woff2: icon("font", "var(--icon-file-font)"),

  // ── 打包 / 二进制 ───────────────────────────────────────────────────────
  zip: icon("archive", "var(--icon-file-archive)"),
  tar: icon("archive", "var(--icon-file-archive)"),
  gz: icon("archive", "var(--icon-file-archive)"),
  tgz: icon("archive", "var(--icon-file-archive)"),
  bz2: icon("archive", "var(--icon-file-archive)"),
  xz: icon("archive", "var(--icon-file-archive)"),
  zst: icon("archive", "var(--icon-file-archive)"),
  "7z": icon("archive", "var(--icon-file-archive)"),
  rar: icon("archive", "var(--icon-file-archive)"),
  whl: icon("package", "var(--icon-file-package)"),
  deb: icon("package", "var(--icon-file-package)"),
  rpm: icon("package", "var(--icon-file-package)"),
  dmg: icon("package", "var(--icon-file-package)"),
  apk: icon("package", "var(--icon-file-package)"),
  jar: icon("package", "var(--icon-file-package)"),
  wasm: icon("binary", "var(--icon-file-wasm)"),
  so: icon("binary", "var(--icon-file-binary)"),
  dylib: icon("binary", "var(--icon-file-binary)"),
  dll: icon("binary", "var(--icon-file-binary)"),
  exe: icon("binary", "var(--icon-file-binary)"),
  bin: icon("binary", "var(--icon-file-binary)"),
  o: icon("binary", "var(--icon-file-binary)"),
  a: icon("binary", "var(--icon-file-binary)"),

  // ── 纯文本 ──────────────────────────────────────────────────────────────
  txt: icon("text", "var(--icon-file-text)"),
  log: icon("text", "var(--icon-file-text)"),
};

/** 精确目录名 → 专属图标。展开态字形差异由 `FileIcon` 处理。 */
const BY_FOLDER_NAME: Record<string, FileIcon> = {
  src: icon("folder-code", "var(--icon-folder-code)"),
  lib: icon("folder-code", "var(--icon-folder-code)"),
  app: icon("folder-code", "var(--icon-folder-code)"),
  packages: icon("folder-code", "var(--icon-folder-code)"),
  components: icon("folder-code", "var(--icon-folder-code)"),
  hooks: icon("folder-code", "var(--icon-folder-code)"),

  test: icon("folder-test", "var(--icon-folder-test)"),
  tests: icon("folder-test", "var(--icon-folder-test)"),
  __tests__: icon("folder-test", "var(--icon-folder-test)"),
  spec: icon("folder-test", "var(--icon-folder-test)"),
  e2e: icon("folder-test", "var(--icon-folder-test)"),

  config: icon("folder-config", "var(--icon-folder-config)"),
  ".config": icon("folder-config", "var(--icon-folder-config)"),
  ".vscode": icon("folder-config", "var(--icon-folder-config)"),
  ".github": icon("folder-config", "var(--icon-folder-config)"),
  scripts: icon("folder-config", "var(--icon-folder-config)"),

  node_modules: icon("folder-dependency", "var(--icon-folder-dependency)"),
  vendor: icon("folder-dependency", "var(--icon-folder-dependency)"),
  ".venv": icon("folder-dependency", "var(--icon-folder-dependency)"),
  venv: icon("folder-dependency", "var(--icon-folder-dependency)"),

  ".git": icon("folder-git", "var(--icon-file-git)"),

  dist: icon("folder-dist", "var(--icon-folder-dist)"),
  build: icon("folder-dist", "var(--icon-folder-dist)"),
  target: icon("folder-dist", "var(--icon-folder-dist)"),
  out: icon("folder-dist", "var(--icon-folder-dist)"),
  coverage: icon("folder-dist", "var(--icon-folder-dist)"),

  docs: icon("folder-docs", "var(--icon-folder-docs)"),
  doc: icon("folder-docs", "var(--icon-folder-docs)"),

  assets: icon("folder-assets", "var(--icon-folder-assets)"),
  public: icon("folder-assets", "var(--icon-folder-assets)"),
  static: icon("folder-assets", "var(--icon-folder-assets)"),
  images: icon("folder-assets", "var(--icon-folder-assets)"),
  icons: icon("folder-assets", "var(--icon-folder-assets)"),
};

/** own-property 查表:名叫 `constructor` 的文件不能命中 Object.prototype。 */
function lookup(table: Record<string, FileIcon>, key: string): FileIcon | undefined {
  return Object.prototype.hasOwnProperty.call(table, key) ? table[key] : undefined;
}

/**
 * 文件图标。`ext` 是后端给的后缀,传了就参与兜底那一步(见 `fileExtensionOf`)。
 *
 * 优先级:精确名 → 名字前缀族 → 后缀 → 兜底。
 */
export function fileIconOf(name: string, ext?: string | null): FileIcon {
  const lowered = name.toLowerCase();

  const byName = lookup(BY_FILE_NAME, lowered);
  if (byName) return byName;

  for (const [prefix, entry] of BY_NAME_PREFIX) {
    if (lowered.startsWith(prefix)) return entry;
  }

  /* 单段后缀:`ext` 传了就用它,与 `fileExtensionOf` 一贯的「后端报的类型优先于名字」
     保持一致(后端把 `opaque` 报成 `sqlite` 时要认)。 */
  const resolved = fileExtensionOf(name, ext);
  if (resolved) {
    const byExtension = lookup(BY_EXTENSION, resolved);
    if (byExtension) return byExtension;
  }

  return DEFAULT_FILE_ICON;
}

/** 目录图标。未知名字回落通用文件夹。 */
export function folderIconOf(name: string): FileIcon {
  return lookup(BY_FOLDER_NAME, name.toLowerCase()) ?? DEFAULT_FOLDER_ICON;
}

/**
 * 条目图标的总入口:符号链接 > 目录 > 文件。
 *
 * 链接排最前是有意的 —— `is_dir` 走的是跟随链接后的类型,指向目录的链接两个标记
 * 同时为真,若先判目录就再也画不出「这是个链接」。
 */
export function entryIconOf(entry: {
  name: string;
  extension?: string | null;
  is_dir: boolean;
  is_symlink?: boolean;
}): FileIcon {
  if (entry.is_symlink) return SYMLINK_ICON;
  if (entry.is_dir) return folderIconOf(entry.name);
  return fileIconOf(entry.name, entry.extension);
}
