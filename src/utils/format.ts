/**
 * 体积/数量格式化的共享实现。
 *
 * 这里刻意保留**三个**函数而不是合并成一个:代码里原有的四份 `formatSize`
 * 语义并不相同,强行统一会改变 UI 上的字符输出。
 *
 * - `formatBytes`         1024 基数,KB/MB 都用 `toFixed(1)`  —— SFTP 列表、本地历史
 * - `formatBytesRounded`  1024 基数,KB 用 `Math.round`      —— 随手记附件
 * - `formatCharCount`     1000 基数,后缀 `K`,数的是**字符不是字节** —— 文本附件
 *
 * 最后一个尤其不能并进前两个:它统计的是 `text.length`(字符数),用 1024 去分档
 * 会让"1000 个字"显示成 "1000" 而不是 "1.0K"。
 */

/**
 * 字节数转可读串。`1024` 基数,KB 与 MB 保留一位小数。
 *
 * @param size 字节数。`null` / `undefined` 返回 `nullLabel`。
 * @param nullLabel 空值占位符,默认 `"--"`。
 */
export function formatBytes(size?: number | null, nullLabel = "--"): string {
  if (size == null) return nullLabel;
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * 同 [`formatBytes`],但 KB 档取整而非保留小数。
 *
 * 随手记附件列表用这个 —— 附件多且行窄,`12 KB` 比 `12.3 KB` 更省位置。
 */
export function formatBytesRounded(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * 字符数转可读串。`1000` 基数,上千用 `K` 后缀。
 *
 * 注意没有单位后缀(不是 `B`)—— 数的是字符,不是字节。
 */
export function formatCharCount(length: number): string {
  if (length < 1000) return `${length}`;
  return `${(length / 1000).toFixed(1)}K`;
}
