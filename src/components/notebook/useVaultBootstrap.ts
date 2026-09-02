/**
 * 面板挂载后的那一串开场:确保 vault 存在 → 迁移遗留数据 → 列出笔记,以及跟着 vault
 * 就绪扫一次的全库标题索引和用户模板表。
 *
 * 三件事收在一起的理由是它们共用同一个触发时机("vault 从 null 变成有")和同一套
 * 失败态度(该静默的静默、该上错误条的上错误条),而这套态度不是自明的 —— 见不变量 4。
 *
 * 不变量:
 *
 * 1. **迁移排在 `listNotes` 之前**。反过来的话刚迁过来的笔记要等下一次挂载才看得见。
 *    迁移本身幂等,已经迁过时是一次廉价的空操作,所以不必先判断"要不要迁"。
 *
 * 2. **初始化只在挂载时跑一次**,依赖数组是空的。vault 切换(P2 的多仓库)会另走一条
 *    显式路径,不靠这个 effect 重跑。
 *
 * 3. **标题索引不跟着 `notes` 重扫**。它唯一服务的对象是"在列表里、但正文还没读进来"的
 *    笔记,而那些全部来自挂载时那次 `listNotes`;之后每条进列表的笔记都是读全的(新建
 *    拿到的是完整笔记,回收站恢复会 `loadNoteByPath`),内存里的标题本来就是真的,
 *    `linkTitleOf` 会优先用它。跟着 `notes` 重扫是纯损失:自动保存每敲一个字都换掉笔记
 *    对象,那会变成每敲一个字扫一遍全库。
 *
 * 4. **只有"你的笔记出事了"才占错误提示条**。所以:
 *    - 迁移失败 → 上提示条(否则用户只会发现"老笔记不见了"而不知道原因),但不阻塞面板,
 *      vault 里可能已经有别的笔记;
 *    - 富文本转换失败 → 上提示条,也不阻塞:笔记仍是有效的 `.md`,只是正文里留着 HTML;
 *    - 标题索引扫不动 → 静默。笔记照样能读能写,只是按标题写的链接暂时退化成只认文件名;
 *    - 模板读不到 → 静默。最坏的结果是命令面板里少几条自定义命令。
 *
 * 5. **只有初始化那条失败要 `setLoading(false)`**。`hydrate` 自己会清掉 loading,而失败
 *    时没人去清 —— 不补这一下面板会永远停在骨架屏上。
 *
 * 6. **每个 async effect 都有 `cancelled` 闸门**。面板可以在任何一趟 await 中间被卸载
 *    (关侧栏、切项目),回来之后往已经卸载的树上 setState 是一条 React 警告加一次
 *    白写的状态。
 *
 * 7. **没有 vault 时把模板表清空**,而不是留着上一个库的。换库之后命令面板里不该还
 *    列着别人的模板。
 */
import { useEffect, useState } from "react";

import { runLegacyMigration } from "./migrateLegacyNotes";
import { toPanelNote } from "./noteConverters";
import type { NotebookNote } from "./notebookStore";
import {
  convertRichtextNotes,
  ensureDefaultVault,
  listUserTemplates,
  vaultIndex,
  type UserTemplate,
} from "./notebookApi";
import { listNotes } from "./notebookVault";

export type VaultBootstrapOptions = {
  /** 当前 vault。null = 初始化还没跑完(标题索引和模板都等它)。 */
  vault: string | null;
  setVault: (vault: string | null) => void;
  /** 列表就位。它自己会清掉 loading 和错误,见不变量 5。 */
  hydrate: (notes: NotebookNote[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  errorText: (error: unknown) => string;
};

export type VaultBootstrapApi = {
  /** 全库的「路径 → frontmatter 标题」。扫不动时是空表(见不变量 4)。 */
  indexedTitles: Map<string, string>;
  /** `<vault>/.notebook/templates/*.md`。读不到时是空数组。 */
  userTemplates: readonly UserTemplate[];
};

export function useVaultBootstrap({
  vault,
  setVault,
  hydrate,
  setLoading,
  setError,
  errorText,
}: VaultBootstrapOptions): VaultBootstrapApi {
  const [indexedTitles, setIndexedTitles] = useState<Map<string, string>>(() => new Map());
  const [userTemplates, setUserTemplates] = useState<readonly UserTemplate[]>([]);

  // 开场那一串。见不变量 1、2、4、5、6。
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const root = await ensureDefaultVault();
        if (cancelled) return;

        const migration = await runLegacyMigration();
        if (cancelled) return;
        if (migration.status === "failed") setError(migration.message);

        /* P1 收尾迁移:把 P0 留下的富文本笔记(HTML + `editor: richtext`)转成 Markdown。
           放在这里而不是只跑一次:用户可能之后从备份恢复出富文本笔记。没有待转文件时
           它只是一次目录扫描,很便宜。 */
        try {
          await convertRichtextNotes(root);
        } catch (error) {
          if (!cancelled) setError(errorText(error));
        }
        if (cancelled) return;

        const listed = await listNotes(root);
        if (cancelled) return;
        setVault(root);
        hydrate(listed.map(toPanelNote));
      } catch (error) {
        if (cancelled) return;
        setError(errorText(error));
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // 见不变量 2。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 全库标题索引。见不变量 3、4、6。
  useEffect(() => {
    if (!vault) return;
    let cancelled = false;
    void (async () => {
      try {
        const entries = await vaultIndex(vault);
        if (cancelled) return;
        setIndexedTitles(new Map(entries.map((entry) => [entry.path, entry.title])));
      } catch {
        /* 静默,见不变量 4。 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [vault]);

  /* 用户模板。和标题索引一样只在 vault 就绪时读一次,不跟着面板每次打开重读:模板是
     用户偶尔手工放进去的文件,而这一趟是 readdir + 逐个读文件 —— 挂在 vault 上已经覆盖
     「换库」这个唯一会变的维度。用户新加了模板文件时重开一次应用(或换一次库)就能刷到,
     这个代价比每次开面板都扫一遍目录小。见不变量 4、6、7。 */
  useEffect(() => {
    if (!vault) {
      setUserTemplates([]);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const list = await listUserTemplates(vault);
        if (cancelled) return;
        setUserTemplates(list);
      } catch {
        if (!cancelled) setUserTemplates([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [vault]);

  return { indexedTitles, userTemplates };
}
