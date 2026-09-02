/**
 * 从路径里取文件名(最后一段)。
 *
 * `path.split(/[\\/]/).pop() ?? path` 这个写法在仓库里散了十来处 —— 诊断面板、
 * 测试失败列表、LSP 跳转、数据库面板各写一遍。它本身不难,但**三种写法混在一起**,
 * 而且差异只在边界上显现,读代码时看不出来谁是有意的:
 *
 *   | 写法                                   | `"/a/b/"` 的结果 |
 *   |----------------------------------------|------------------|
 *   | `.pop() ?? path`                       | `""`             |
 *   | `.filter(Boolean).pop() ?? path`       | `"b"`            |
 *   | `.pop() \|\| path`                      | `"/a/b/"`        |
 *
 * 本文件只统一**第一种**,因为它是「显示用的文件名」这条语义的那些调用点在用的:
 * 路径来自后端给的诊断 / 测试失败 / LSP location,一律是具体文件,不会带尾随分隔符。
 *
 * 刻意**没有**统一后两种:
 * - `run/runConfigState.ts` 与 `debug/debugState.ts` 的 `.filter(Boolean)` 是要
 *   容忍用户手填的、可能带尾随斜杠的路径,换成本函数会让 `"/a/b/"` 变成空名字。
 * - `command-palette/CommandPalette.tsx` 与 `database/databaseViewModel.ts:693`
 *   用 `||` 或带自己的兜底字面量(`"database.db"`),空串要落到兜底上,
 *   `??` 挡不住空串。
 *
 * 这三组的差异已由 `src/test/file-path.test.ts` 逐条钉住 —— 谁想再合并一步,
 * 会先看到那几条断言挂掉,那是提醒他在改行为,不是回归。
 */

/**
 * 取路径的最后一段作为显示用文件名。
 *
 * 同时认 `/` 和 `\`(Windows 路径会以反斜杠形式从后端过来)。`split` 至少返回一个
 * 元素,所以 `?? path` 这一支实际够不着 —— 保留是为了和被它替换掉的五处写法**逐字节
 * 同义**,也让类型收窄成 `string` 而不是 `string | undefined`。
 */
export function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
