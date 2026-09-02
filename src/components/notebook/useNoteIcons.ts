/**
 * 笔记的自定义图标:那张表、选择器的窗态、以及"选一个"这件事。
 *
 * 表存在 `<vault>/.notebook/icons.json` 里,键是 vault 相对路径 —— 绝对路径会在整个库
 * 搬家之后全部失配。
 *
 * 不变量:
 *
 * 1. **只在 vault 就绪时读一次**。之后的改动都经过 `pick`,那里同时更新内存和磁盘,
 *    所以没有"磁盘上更新、内存里旧"的窗口需要靠重读来收敛。
 *
 * 2. **读失败不报错**。图标是装饰,而面板那条错误提示是用来说"你的笔记出事了"的 ——
 *    读不到就全用默认图标,静默降级。**写**失败要报:那是用户刚做的一个动作没生效。
 *
 * 3. **写是乐观更新 + 失败回滚**。先换内存里那张表让列表当场变,再写盘;写失败就放回
 *    上一份并报错。留着一个「看起来改了、重开面板又变回去」的图标比当场说失败更难查。
 *
 * 4. **同一个引用表示没变化**(重复点同一个图标),那就不写盘 —— `withNoteIcon` 负责
 *    给出这个判断,这里只是不去做那次 IO。
 *
 * 5. **选择器的坐标由调用方给**。它接在列表右键菜单原来那个位置弹出,而菜单那时已经
 *    关掉了 —— 坐标只存在于调用的那一刻。
 */
import { useCallback, useEffect, useState } from "react";

import type { NoteIconPickerState } from "./NoteIconPicker";
import { withNoteIcon, type NoteIconName } from "./noteIcons";
import { readNoteIcons, writeNoteIcons } from "./notebookApi";

export type NoteIconsOptions = {
  vault: string | null;
  /** 写盘失败时报到面板那条错误提示上。 */
  onError: (message: string) => void;
  errorText: (error: unknown) => string;
  /** 没有 vault 时的提示文案(选图标要写盘,没库写不了)。 */
  noVaultMessage: string;
};

export type NoteIconsApi = {
  /** vault 相对路径 → 图标名。 */
  table: Record<string, string>;
  /** 选择器的窗态,`null` = 没开。 */
  picker: NoteIconPickerState | null;
  openPicker: (state: NoteIconPickerState) => void;
  closePicker: () => void;
  /** 选一个图标(`null` = 恢复默认)。顺带关掉选择器。 */
  pick: (noteId: string, icon: NoteIconName | null) => void;
};

export function useNoteIcons({
  vault,
  onError,
  errorText,
  noVaultMessage,
}: NoteIconsOptions): NoteIconsApi {
  const [table, setTable] = useState<Record<string, string>>({});
  const [picker, setPicker] = useState<NoteIconPickerState | null>(null);

  useEffect(() => {
    if (!vault) return;
    let cancelled = false;
    void (async () => {
      try {
        const next = await readNoteIcons(vault);
        if (cancelled) return;
        setTable(next);
      } catch {
        /* 见不变量 2:读不到就全用默认图标,不占用错误提示条。 */
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [vault]);

  const openPicker = useCallback((state: NoteIconPickerState) => setPicker(state), []);
  const closePicker = useCallback(() => setPicker(null), []);

  const pick = (noteId: string, icon: NoteIconName | null) => {
    setPicker(null);
    if (!vault) {
      onError(noVaultMessage);
      return;
    }
    const next = withNoteIcon(table, vault, noteId, icon);
    // 见不变量 4。
    if (next === table) return;
    const previous = table;
    setTable(next);
    void writeNoteIcons(vault, next).catch((error: unknown) => {
      setTable(previous);
      onError(errorText(error));
    });
  };

  return { table, picker, openPicker, closePicker, pick };
}
