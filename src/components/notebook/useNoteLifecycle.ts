/**
 * 笔记的增删:空白新建、按模板新建、日记、以及删除。
 *
 * 四条路都要"把新的那篇放进列表并切过去"或"从列表里拿掉",而列表和当前选中都在
 * store 上 —— 收在一起是为了让这条"进出列表"的口径只有一份。
 *
 * 不变量:
 *
 * 1. **没有 vault 就明说**。四条路都要写盘,静默 return 会让用户以为按钮坏了。
 *
 * 2. **已经在列表里的不重复加**(`adopt`)。日记开第二次、或者按同一个模板建到已存在的
 *    路径上时,重新读一遍会把内存里那份**未落盘**的工作副本换成磁盘上的旧内容 ——
 *    用户刚打的字就没了。所以在列表里就只切过去,连 IPC 都不发。
 *
 * 3. **日记不用模板覆盖已存在的那份**。落点固定为 `<vault>/Daily/YYYY-MM-DD.md`,
 *    后端的 `openOrCreateNoteAt` 只在文件不存在时才写模板。
 *
 * 4. **删除是乐观移除 + 失败还原**。UI 立刻响应;IPC 失败时文件还在磁盘上,列表要放回去,
 *    否则用户以为删掉了。
 *
 * 5. **删除前取消挂起的自动保存**。这不是"防止删掉的文件被重新创建"的那道防线 ——
 *    真正兜住那件事的是乐观移除 + `flushNote` 里的 `!current` 早退(定时器醒来时笔记
 *    已经不在 notesRef 里)。这一下的意义是省掉一次无用的 IPC,并且在将来有人把移除
 *    改成"等 IPC 成功再移除"时仍然成立。
 */
import type { NotebookNote } from "./notebookStore";
import {
  createNote as createVaultNote,
  createNoteFromTemplate,
  openOrCreateNoteAt,
  removeNote,
  type VaultNote,
} from "./notebookVault";
import { toPanelNote, toVaultNote } from "./noteConverters";
import { buildTemplate, DAILY_TEMPLATE, type NoteTemplate } from "./noteTemplates";
import { dailyNotePath, dailyStepFrom } from "./noteDaily";
import { fillTitle, type UserTemplateEntry } from "./noteUserTemplates";

/* 比 `noteExportRun` 的 `Translate` 宽一档:`buildTemplate` 的插值里有数字(周数)。 */
type Translate = (key: string, vars?: Record<string, string | number>) => string;

export type NoteLifecycleOptions = {
  vault: string | null;
  notes: readonly NotebookNote[];
  activeNoteId: string | null;
  setNotes: (updater: (current: NotebookNote[]) => NotebookNote[]) => void;
  setActiveId: (noteId: string) => void;
  onError: (message: string) => void;
  errorText: (error: unknown) => string;
  /** 删除前取消那一篇挂起的自动保存,见不变量 5。 */
  cancelSave: (noteId: string) => void;
  /** 空白新建之后把焦点送到标题栏。 */
  focusTitleAfter: (noteId: string) => void;
  /** 空白新建要切到编辑态 —— 新笔记是空的,阅读态下什么都看不见。 */
  toEditMode: () => void;
  t: Translate;
};

export type NoteLifecycleApi = {
  /** 空白新建。切到编辑态,并把焦点送到标题栏。 */
  addNote: () => void;
  addFromTemplate: (template: NoteTemplate) => void;
  addFromUserTemplate: (entry: UserTemplateEntry) => void;
  /** 打开某一天的日记,没有就按模板建出来。 */
  openDaily: (date: Date) => void;
  /** 前一天 / 后一天。当前打开的是日记就以它为基准,这样能连着翻。 */
  stepDaily: (delta: number) => void;
  /** 删除任意一条(进回收站)。标题栏删当前这条,列表右键菜单删被点中的那条。 */
  remove: (noteId: string) => void;
};

export function useNoteLifecycle({
  vault,
  notes,
  activeNoteId,
  setNotes,
  setActiveId,
  onError,
  errorText,
  cancelSave,
  focusTitleAfter,
  toEditMode,
  t,
}: NoteLifecycleOptions): NoteLifecycleApi {
  const noVault = () => {
    onError(t("notebook.vaultUnavailable"));
  };

  /** 把一条笔记放进列表并切过去。见不变量 2。 */
  const adopt = (note: VaultNote) => {
    const panelNote = toPanelNote(note);
    setNotes((current) =>
      current.some((existing) => existing.id === panelNote.id) ? current : [panelNote, ...current],
    );
    setActiveId(panelNote.id);
  };

  const addNote = () => {
    toEditMode();
    if (!vault) {
      noVault();
      return;
    }
    void (async () => {
      try {
        const note = toPanelNote(await createVaultNote(vault, ""));
        setNotes((current) => [note, ...current]);
        setActiveId(note.id);
        focusTitleAfter(note.id);
      } catch (error) {
        onError(errorText(error));
      }
    })();
  };

  /** 按模板新建。文件名由后端从标题分配,所以同一个模板可以反复用。 */
  const addFromTemplate = (template: NoteTemplate) => {
    if (!vault) {
      noVault();
      return;
    }
    const { title, body } = buildTemplate(template, new Date(), t);
    void (async () => {
      try {
        adopt(await createNoteFromTemplate(vault, title, body));
      } catch (error) {
        onError(errorText(error));
      }
    })();
  };

  /**
   * 按用户自定义模板新建。
   *
   * 和内置模板走同一条路(后端按标题分配文件名),区别只在正文哪来:这里是磁盘上那个
   * `.md` 文件的字面内容,`{{date}}` / `{{time}}` 已经在 `expandUserTemplate` 里展开过。
   *
   * `{{title}}` 留到这一步才替换,而且用的是**最终标题** —— 也就是 `name` 展开后的
   * 那个串。正文里写 `# {{title}}` 是最常见的模板首行,它必须和笔记标题一致。
   */
  const addFromUserTemplate = (entry: UserTemplateEntry) => {
    if (!vault) {
      noVault();
      return;
    }
    void (async () => {
      try {
        adopt(await createNoteFromTemplate(vault, entry.name, fillTitle(entry.body, entry.name)));
      } catch (error) {
        onError(errorText(error));
      }
    })();
  };

  const openDaily = (date: Date) => {
    if (!vault) {
      noVault();
      return;
    }
    const path = dailyNotePath(vault, date);
    // 见不变量 2:已经在列表里就只切过去。
    if (notes.some((note) => note.id === path)) {
      setActiveId(path);
      return;
    }
    const { title, body } = buildTemplate(DAILY_TEMPLATE, date, t);
    void (async () => {
      try {
        adopt(await openOrCreateNoteAt(path, title, body));
      } catch (error) {
        onError(errorText(error));
      }
    })();
  };

  const stepDaily = (delta: number) => {
    openDaily(dailyStepFrom(activeNoteId, new Date(), delta));
  };

  const remove = (noteId: string) => {
    const target = notes.find((note) => note.id === noteId);
    if (!target) return;
    // 见不变量 5。
    cancelSave(target.id);
    // 先从列表里移除,UI 立刻响应;失败再放回去(不变量 4)。
    setNotes((current) => current.filter((note) => note.id !== target.id));
    void (async () => {
      try {
        await removeNote(toVaultNote(target));
      } catch (error) {
        onError(errorText(error));
        setNotes((current) =>
          current.some((note) => note.id === target.id) ? current : [target, ...current],
        );
      }
    })();
  };

  return { addNote, addFromTemplate, addFromUserTemplate, openDaily, stepDaily, remove };
}
