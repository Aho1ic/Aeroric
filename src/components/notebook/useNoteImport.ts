/* 从别家笔记应用导入(Obsidian / Logseq / Notion / Bear / Roam / Evernote / Apple Notes)。
 *
 * 四件事必须在一个地方对齐:
 *
 * 1. **`busy` 是并发闸门,不只是转圈的开关**。后端在读源端、写 vault,并发两条会互相
 *    拖慢,而且两个导入器可能落到同一个目录。
 *
 * 2. **解禁放在 `finally`**。`runImport` 抛出来的时候按钮也必须解禁,否则面板永久卡住。
 *
 * 3. **取消不动任何状态**。既没成功也没失败,上一次的报告继续留着 —— 把它清成 null 会
 *    让用户以为刚才那次导入的记录丢了。
 *
 * 4. **导入完要重列笔记**。导入把文件直接写进了 vault,内存里的列表不知道;不重列的话
 *    新笔记要等下次挂载才出现,而用户刚看到「已导入 N 条」。重列失败**不**覆盖报告:
 *    文件确实进去了,只是列表没刷上,报告仍然是真的。
 *
 * 各家的目录约定和解析在 `noteImport.ts`(纯逻辑 + 一层可注入的 deps),这里只管状态。 */
import { useState } from "react";
import {
  availableImportProviders,
  defaultImportDeps,
  runImport,
  type ImportProvider,
  type ImportProviderId,
} from "./noteImport";
import type { ImportReport } from "./notebookApi";
import type { Translate } from "./noteExportRun";

export type NoteImportOptions = {
  vault: string | null;
  t: Translate;
  /** 重列笔记并写回内存。抛出的错由这里接住,报告不受影响。 */
  reloadNotes: () => Promise<void>;
  /** 重列失败时的落点:面板顶部那条全局错误(导入本身是成功的,不该报在窗里)。 */
  setPanelError: (message: string) => void;
  errorText: (error: unknown) => string;
};

export type NoteImportApi = {
  open: boolean;
  /** 正在跑的那个导入器。null = 空闲。 */
  busy: ImportProviderId | null;
  report: ImportReport | null;
  error: string | null;
  providers: readonly ImportProvider[];
  openSheet: () => void;
  closeSheet: () => void;
  run: (provider: ImportProvider) => void;
};

export function useNoteImport(options: NoteImportOptions): NoteImportApi {
  const { vault, t, reloadNotes, setPanelError, errorText } = options;

  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<ImportProviderId | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const openSheet = () => {
    // 不清 `report`:重开面板时上一次的报告还有对账价值。错误清掉 —— 那是
    // 上一次尝试的,留着会让人以为这次也失败了。
    setError(null);
    setOpen(true);
  };

  const closeSheet = () => {
    setOpen(false);
  };

  const run = (provider: ImportProvider) => {
    if (busy) return;
    if (!vault) {
      setError(t("notebook.importNoVault"));
      return;
    }
    setError(null);
    setBusy(provider.id);
    void (async () => {
      let outcome: Awaited<ReturnType<typeof runImport>>;
      try {
        outcome = await runImport(provider, vault, defaultImportDeps(), t);
      } finally {
        setBusy(null);
      }
      if (outcome.status === "cancelled") return;
      if (outcome.status === "failed") {
        setError(t("notebook.importFailed", { message: outcome.message }));
        return;
      }
      setReport(outcome.report);
      try {
        await reloadNotes();
      } catch (err) {
        setPanelError(errorText(err));
      }
    })();
  };

  return {
    open,
    busy,
    report,
    error,
    providers: availableImportProviders(),
    openSheet,
    closeSheet,
    run,
  };
}
