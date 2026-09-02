/* 跨文件重命名标签。
 *
 * 三件事必须在一个地方对齐:
 *
 * 1. **结束后不关窗**。报告(改了几处、跳过哪些、哪些失败)就是这个操作的结果,关掉
 *    等于把它扔了。用户看完自己按「完成」。
 *
 * 2. **开新窗时清掉上一次的报告和错误**,否则会看着像这一次的结果。
 *
 * 3. **完了要重扫标签清单**,并把展开的那一条收起来:清单现在是过期的 —— 旧名字那一行
 *    还在,而它已经不存在了,点它会展开一堆跳不到的引用;展开态按旧 key 记,重扫后
 *    那个 key 可能已经没了。反链清单**不**受影响(标签不是 wikilink),不用跟着重扫。 */
import { useState } from "react";
import { renameVaultTag, type TagRenameReport } from "./notebookApi";
import type { TagRenameDialogState } from "./TagRenameDialog";

export type TagRenameOptions = {
  vault: string | null;
  errorText: (error: unknown) => string;
  /** 重扫标签清单。 */
  refreshTags: () => void;
  /** 收起展开的那一条标签。 */
  collapseOpenTag: () => void;
};

export type TagRenameApi = {
  /** null = 没开窗。非空时同时带着窗的锚点坐标和目标标签。 */
  state: TagRenameDialogState | null;
  report: TagRenameReport | null;
  running: boolean;
  error: string | null;
  openFor: (state: TagRenameDialogState) => void;
  close: () => void;
  submit: (next: string) => void;
};

export function useTagRename(options: TagRenameOptions): TagRenameApi {
  const { vault, errorText, refreshTags, collapseOpenTag } = options;

  const [state, setState] = useState<TagRenameDialogState | null>(null);
  const [report, setReport] = useState<TagRenameReport | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openFor = (next: TagRenameDialogState) => {
    setReport(null);
    setError(null);
    setState(next);
  };

  const close = () => {
    setState(null);
  };

  /** 目标取自当前窗态 —— 窗关着就没有可改的标签。 */
  const submit = (next: string) => {
    const target = state;
    if (!vault || !target) return;
    setRunning(true);
    setError(null);
    void (async () => {
      try {
        setReport(await renameVaultTag(vault, target.key, next));
        collapseOpenTag();
        refreshTags();
      } catch (err) {
        setError(errorText(err));
      } finally {
        setRunning(false);
      }
    })();
  };

  return { state, report, running, error, openFor, close, submit };
}
