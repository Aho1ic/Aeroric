import type { SetStateAction } from "react";
import { createStore } from "zustand/vanilla";
import type { NoteSig } from "./notebookApi";
import type { NoteFrontmatter } from "./noteFrontmatter";

/**
 * 面板里的一条笔记。
 *
 * `id` 就是文件的绝对路径:磁盘上不会有两个同路径文件,所以它天然是主键。
 * 这样面板里所有既有的 `note.id` 用法不用改,同时又指向真实文件。
 *
 * 注:P0 时这里还有个 `format: "markdown" | "richtext"`。P1 上了 WYSIWYG 之后
 * 富文本编辑器下线,存量笔记由 `notebook_convert_richtext` 转成 Markdown ——
 * **现在所有笔记都是 Markdown**,那个字段没有意义了。
 */
export interface NotebookNote {
  /** 绝对路径,同时是主键。 */
  id: string;
  title: string;
  /** Markdown 源码。 */
  body: string;
  updatedAt: number;
  /** 打开时拿到的磁盘指纹,保存时作冲突检测基线。未读入时为 null。 */
  sig: NoteSig | null;
  /** frontmatter 里我们不认识的字段,保存时原样写回,避免抹掉第三方数据。 */
  frontmatter: NoteFrontmatter;
  /** 正文是否已从磁盘读入。列表初次加载只拿元数据。 */
  loaded: boolean;
}

export interface NotebookPanelState {
  notes: NotebookNote[];
  activeId: string | null;
  /** 当前 vault 的绝对路径。null = 还没初始化完。 */
  vault: string | null;
  /** 首次加载 vault 期间为 true,用于区分「还在读」和「真的没有笔记」。 */
  loading: boolean;
  /** 最近一次后台失败(加载 / 保存 / 删除)。UI 要如实显示,不能静默。 */
  error: string | null;
  setNotes: (value: SetStateAction<NotebookNote[]>) => void;
  setActiveId: (id: string | null) => void;
  setVault: (vault: string | null) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  hydrate: (notes: NotebookNote[]) => void;
  reset: () => void;
}

export function createNotebookStore(initialNotes: NotebookNote[] = []) {
  return createStore<NotebookPanelState>((set) => ({
    notes: initialNotes,
    activeId: initialNotes[0]?.id ?? null,
    vault: null,
    // 面板挂载后立刻去读 vault,所以初始就是「正在加载」。
    loading: true,
    error: null,
    setNotes: (value) =>
      set((state) => ({ notes: typeof value === "function" ? value(state.notes) : value })),
    setActiveId: (activeId) => set({ activeId }),
    setVault: (vault) => set({ vault }),
    setLoading: (loading) => set({ loading }),
    setError: (error) => set({ error }),
    hydrate: (notes) => set({ notes, activeId: notes[0]?.id ?? null, loading: false, error: null }),
    reset: () => set({ notes: [], activeId: null, loading: false, error: null }),
  }));
}
