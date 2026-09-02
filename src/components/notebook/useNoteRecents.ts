/* "最近打开过哪几篇"。命令面板空查询时列它。
 *
 * 三件事必须在一个地方对齐:
 *
 * 1. **换 vault 就换一份**。不清的话上一个库的相对路径会留在内存里,而它们在新库里
 *    可能**恰好也存在**(`Index.md` 这种名字很常见)—— 那样命令面板会把没打开过的
 *    笔记列成"最近打开",用户无从判断这份名单是哪来的。
 *
 * 2. **记账挂在 `activeNoteId` 上,不逐个包 `setActiveId`**。切换笔记的入口有十来处
 *    (列表点选、双链跳转、搜索命中、删除后落到邻居…),漏一个就是一条静默不记账的
 *    路径。
 *
 * 3. **顺序没变就不写盘**。那个 effect 每次重渲染都跑,而绝大多数时候当前笔记没换。
 *
 * 存的是 vault 相对路径(`keys`),对外给的是绝对路径(`noteIds`)—— 换库之后同一篇
 * 笔记的绝对路径会变,相对路径不会。
 */
import { useEffect, useMemo, useState } from "react";
import {
  loadNoteRecents,
  resolveNoteRecents,
  saveNoteRecents,
  touchNoteRecent,
} from "./noteRecents";

export type NoteRecentsOptions = {
  vault: string | null;
  activeNoteId: string | null;
  /** 现存笔记的绝对路径。删掉的笔记不该出现在名单里。 */
  noteIds: string[];
};

export type NoteRecentsApi = {
  /** 解析成绝对路径、且过滤掉已不存在的,最近在前。 */
  noteIds: string[];
};

export function useNoteRecents(options: NoteRecentsOptions): NoteRecentsApi {
  const { vault, activeNoteId, noteIds } = options;

  const [keys, setKeys] = useState<string[]>([]);

  useEffect(() => {
    setKeys(vault ? loadNoteRecents(vault) : []);
  }, [vault]);

  useEffect(() => {
    if (!vault || !activeNoteId) return;
    setKeys((current) => {
      const next = touchNoteRecent(vault, activeNoteId, current);
      if (next.length === current.length && next.every((key, index) => key === current[index])) {
        return current;
      }
      saveNoteRecents(vault, next);
      return next;
    });
  }, [vault, activeNoteId]);

  /* 依赖取拼起来的串而不是 `noteIds` 本身:那个数组每次渲染都是新的(调用处 `map` 出来
     的),按它当依赖等于不缓存。 */
  const noteIdsKey = noteIds.join("");
  const resolved = useMemo(
    () => resolveNoteRecents(vault ?? "", keys, noteIds),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 见上:刻意按拼串而不是 noteIds
    [vault, keys, noteIdsKey],
  );

  return { noteIds: resolved };
}
