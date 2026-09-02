/**
 * 文件后缀的单一来源。
 *
 * 这几组后缀此前散在两个互不相干的模块里各写一份:`file-viewer/editorUtils.ts`
 * 的 `isPreviewableImageFile` / `isSqliteDatabaseFile`,和 `file-explorer/
 * fileEntryUtils.ts` 的 `fileIconKind` / `isSqliteDatabaseFileName`。图片那七个
 * 和 sqlite 那三个是字面完全一致的重复 —— 加一个后缀要记得改两处,漏一处就出现
 * "图标是图片但点开不预览"这种对不上的状态。
 *
 * 放在 `lib/` 而不是塞进其中一边:file-viewer 与 file-explorer 是平级功能目录,
 * 让前者 import 后者会凭空造出一条跨功能依赖。这里只有纯数据和一个取后缀的函数,
 * 不依赖 React 也不依赖任何组件。
 *
 * 刻意**没有**统一的:markdown。`isMarkdownFile` 认 md/mdx/markdown 三个,而
 * 图标表只认 md/mdx —— 那是一处真实的不一致,已由
 * `src/test/file-extension-predicates.test.ts` 钉住现状,改它属于行为变更,
 * 不混在这次去重里做。
 */

// 标成 `readonly string[]` 而不是 `as const` 元组:调用点要拿任意 string 去
// `.includes()`,元组类型会把参数收窄成字面量联合,逼着每处写一个 cast。
/** 能在编辑器里直接预览的图片。 */
export const PREVIEWABLE_IMAGE_EXTENSIONS: readonly string[] = [
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "bmp",
  "svg",
];

/** 能用内置 SQLite 面板打开的库文件。 */
export const SQLITE_DATABASE_EXTENSIONS: readonly string[] = ["db", "sqlite", "sqlite3"];

/** `isMarkdownFile` 认的那三个(注意图标表只认前两个,见文件头说明)。 */
export const MARKDOWN_EXTENSIONS: readonly string[] = ["md", "mdx", "markdown"];

/**
 * 取小写后缀。
 *
 * `ext` 传了就用它 —— 包含传空字符串的情况,`??` 只挡 null/undefined,
 * 后端把 extension 报成 `""` 时结果就是 `""`。没传则取名字里最后一个 `.` 之后
 * 的部分;名字里没有 `.` 时整个名字被当作后缀(所以 `Dockerfile` → `dockerfile`,
 * 而叫 `png` 的无后缀文件会被当成图片 —— 这是历史行为,已固化在测试里)。
 */
export function fileExtensionOf(name: string, ext?: string | null): string {
  return (ext ?? name.split(".").pop() ?? "").toLowerCase();
}

function hasExtension(list: readonly string[], name: string, ext?: string | null): boolean {
  return list.includes(fileExtensionOf(name, ext));
}

export function hasPreviewableImageExtension(name: string, ext?: string | null): boolean {
  return hasExtension(PREVIEWABLE_IMAGE_EXTENSIONS, name, ext);
}

export function hasSqliteDatabaseExtension(name: string, ext?: string | null): boolean {
  return hasExtension(SQLITE_DATABASE_EXTENSIONS, name, ext);
}

export function hasMarkdownExtension(name: string, ext?: string | null): boolean {
  return hasExtension(MARKDOWN_EXTENSIONS, name, ext);
}
