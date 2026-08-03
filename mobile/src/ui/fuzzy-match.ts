/** Agent 名称模糊搜索:大小写无关的子序列匹配,query 字符按序出现在 text 中即命中。 */
export function fuzzyMatch(text: string, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return true;
  const haystack = text.toLocaleLowerCase();
  let cursor = 0;
  for (const char of needle) {
    // 空格不参与匹配,允许「open router」命中「OpenAI Router」
    if (char === " ") continue;
    const index = haystack.indexOf(char, cursor);
    if (index === -1) return false;
    cursor = index + 1;
  }
  return true;
}
