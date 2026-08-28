/* 标题锚点 id 的生成规则。
 *
 * 单独成文件是因为**两处必须用同一套规则**:`noteRender` 给渲染出的 `<h*>` 挂
 * id,`noteOutline` 给大纲条目算 anchor。两边一旦漂了,大纲点了就跳不动。
 *
 * 规则跟 `file-viewer/markdownPreview.ts` 保持一致,免得同一篇 markdown 在
 * 文件预览和随手记里得到两套锚点。
 */

/** 同名标题去重用的计数器状态。 */
export type SlugRegistry = Set<string>;

export function createSlugRegistry(): SlugRegistry {
  return new Set<string>();
}

/**
 * 把标题纯文本转成锚点 id。
 *
 * `used` 用来去重:同名标题第二次出现会得到 `-1` 后缀。**每篇笔记一个新的
 * registry** —— 复用会让重新渲染同一篇时所有 id 都带上后缀。
 */
export function slugifyHeading(plain: string, used: SlugRegistry): string {
  const base =
    plain
      .toLowerCase()
      // 保留 CJK:随手记大量中文标题,ASCII 化会让它们全部退化成 `section`
      .replace(/[^\w一-龥 -]/g, "")
      .replace(/\s+/g, "-")
      .replace(/-+/g, "-")
      .replace(/^-+|-+$/g, "") || "section";
  let id = base;
  let suffix = 1;
  while (used.has(id)) id = `${base}-${suffix++}`;
  used.add(id);
  return id;
}
