/**
 * 面板上那一串浮层:两个右键菜单、图标选择、标签重命名、五档铺满型 sheet(历史 /
 * 回收站 / 字段 / 属性 / 图谱)、任务收集箱及其右键菜单、语义检索、云盘同步、全库
 * 搜索、快速捕获、导出、导入、命令面板、触发菜单、选区气泡。
 *
 * 收在一个文件里的理由是**层叠顺序** —— 这一批 overlay 同 z-index,谁盖住谁完全由
 * JSX 顺序决定,而那几条先后关系每一条都有理由,散在一屏之外就没人能一次读完。
 * 见不变量 2。
 *
 * 不变量:
 *
 * 1. **必须渲染在面板两列之外**。铺满型的那些自己是 `absolute; inset: 0`,放进侧栏
 *    (`overflow: auto`)或正文那一列会被列宽裁掉半个窗。这个组件返回的是一个
 *    Fragment,插在哪由 `NotebookPanel` 决定 —— 所以这一条得由调用点守着。
 *
 * 2. **JSX 顺序 = 层叠顺序,不能重排**。后写的盖住先写的。具体几条:
 *    - 命令面板排在最后:它是任何状态下都能唤出的那一层,被别的 sheet 盖住就等于
 *      在那些状态下不可用;
 *    - 全库搜索排在所有 sheet 之后:⌘⇧F 是用户当前动作的焦点,别的 overlay 开着时
 *      仍然要能盖到最上面来;
 *    - 语义检索排在收集箱之后、全库搜索之前:它和收集箱互斥(同一个 `sheets` 槽),
 *      而 ⌘⇧F 不在那个槽里;
 *    - 快速捕获排在命令面板之前:它是从命令面板里唤出来的,面板关掉之后这个窗才
 *      出现,两者不会同时在场。
 *
 * 3. **铺满型 overlay 的互斥不在这里判**。`sheets` 只有一个槽,同一时刻最多一档开着
 *    (见 `useNoteSheets`);历史 / 回收站 / 属性不在那个槽里,由面板的 `openSheet`
 *    显式关掉。所以这里每一档只问自己开没开。
 *
 * 4. **小窗(导出 / 导入 / 快速捕获)不参与那个互斥**。整库导出、导入一个大库都可能
 *    要跑一阵,这期间不该把别的面板都关掉。
 *
 * 5. **跟着坐标走的那三个(收集箱右键菜单、触发菜单、选区气泡)是 `fixed`**,不参与
 *    上面的层叠;但触发菜单和气泡都要给命令面板让位 —— ⌘K 不动选区也不动光标,它们
 *    不会自己收,留着就只是挡视线的一块。
 */
import { vaultRelativePath } from "./attachmentUrls";
import { NoteAiSheet } from "./NoteAiSheet";
import { NoteBubbleMenu } from "./NoteBubbleMenu";
import { NoteCommandPalette } from "./NoteCommandPalette";
import {
  NoteContextMenu,
  type NoteContextMenuAction,
  type NoteContextMenuState,
} from "./NoteContextMenu";
import { toVaultNote } from "./noteConverters";
import { NoteExportSheet } from "./NoteExportSheet";
import type { FieldEntry } from "./noteFields";
import { NoteFieldsSheet } from "./NoteFieldsSheet";
import type { NoteGraph } from "./noteGraph";
import { NoteGraphSheet } from "./NoteGraphSheet";
import { NoteHistorySheet } from "./NoteHistorySheet";
import { noteIconOf } from "./noteIcons";
import { NoteIconPicker } from "./NoteIconPicker";
import { NoteImportSheet } from "./NoteImportSheet";
import {
  NoteListContextMenu,
  type NoteListContextMenuAction,
  type NoteListContextMenuState,
} from "./NoteListContextMenu";
import type { NoteStats } from "./noteOutline";
import { NotePropertiesSheet } from "./NotePropertiesSheet";
import { NoteQuickCapture } from "./NoteQuickCapture";
import type { RagHit } from "./noteRag";
import { NoteSearchSheet } from "./NoteSearchSheet";
import { NoteSyncSheet } from "./NoteSyncSheet";
import { NoteTaskContextMenu, type NoteTaskContextMenuAction } from "./NoteTaskContextMenu";
import type { InboxTask } from "./noteTaskInbox";
import { NoteTaskInboxSheet } from "./NoteTaskInboxSheet";
import { NoteTrashSheet } from "./NoteTrashSheet";
import { NoteTriggerMenu } from "./NoteTriggerMenu";
import { NoteVaultReplaceBar } from "./NoteVaultReplaceBar";
import type { NotebookNote } from "./notebookStore";
import { noteFileContent } from "./notebookVault";
import { TagRenameDialog } from "./TagRenameDialog";
import type { NoteBubbleApi } from "./useNoteBubble";
import type { NoteCaptureApi } from "./useNoteCapture";
import type { NoteCommandPaletteApi } from "./useNoteCommandPalette";
import type { NoteExportApi } from "./useNoteExport";
import type { NoteHistoryApi } from "./useNoteHistory";
import type { NoteIconsApi } from "./useNoteIcons";
import type { NoteImportApi } from "./useNoteImport";
import type { NotePropertiesApi } from "./useNoteProperties";
import type { NoteRagApi } from "./useNoteRag";
import type { NoteSheetsApi } from "./useNoteSheets";
import type { NoteSyncApi } from "./useNoteSync";
import type { NoteTrashApi } from "./useNoteTrash";
import type { NoteTriggersApi } from "./useNoteTriggers";
import type { TagRenameApi } from "./useTagRename";
import type { VaultSearchReplaceApi } from "./useVaultSearchReplace";

/** 浮层只用得上扫描的这三样,`data` 由面板自己折成各视图要的形状后单独传进来。 */
type ScanState = {
  loading: boolean;
  error: string | null;
  refresh: () => void;
};

export type NoteOverlaysProps = {
  /** 当前这一篇。属性窗、图谱焦点、语义检索的上下文、导出窗都要它。 */
  activeNote: NotebookNote | null;
  /** 当前正文的统计。属性窗里的字数 / 标题数 / 阅读时长。 */
  noteStats: NoteStats;
  vault: string | null;
  setActiveId: (noteId: string) => void;

  /** 编辑区右键菜单。 */
  contextMenu: NoteContextMenuState | null;
  onContextMenuAction: (action: NoteContextMenuAction) => void;
  /** 笔记列表右键菜单。 */
  listMenu: NoteListContextMenuState | null;
  onListMenuAction: (action: NoteListContextMenuAction) => void;
  /** 收集箱里那条任务的右键菜单。菜单态自己在 `sheets` 里,见不变量 5。 */
  onTaskMenuAction: (action: NoteTaskContextMenuAction) => void;

  sheets: NoteSheetsApi;
  icons: NoteIconsApi;
  tagRename: TagRenameApi;
  noteHistory: NoteHistoryApi;
  noteTrash: NoteTrashApi;
  noteProperties: NotePropertiesApi;
  rag: NoteRagApi;
  sync: NoteSyncApi;
  vaultSearch: VaultSearchReplaceApi;
  captureSheet: NoteCaptureApi;
  exportSheet: NoteExportApi;
  importSheet: NoteImportApi;
  palette: NoteCommandPaletteApi;
  noteTriggers: NoteTriggersApi;
  bubble: NoteBubbleApi;

  /** 反链扫描。图谱和侧栏的反链档共用同一份,这里只用它的状态。 */
  linkScan: ScanState;
  fieldScan: ScanState;
  taskScan: ScanState;
  /** 全库 frontmatter 字段。 */
  fieldEntries: FieldEntry[];
  /** 全库任务。 */
  inboxTasks: InboxTask[];
  /** 引用图谱。关着时是空图。 */
  noteGraph: NoteGraph;

  /** 点收集箱里的一条任务:先关 sheet 再跳。 */
  onJumpInboxTask: (path: string, line: number) => void;
  /** 点语义检索的一条命中。 */
  onOpenAiHit: (hit: RagHit) => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

export function NoteOverlays({
  activeNote,
  noteStats,
  vault,
  setActiveId,
  contextMenu,
  onContextMenuAction,
  listMenu,
  onListMenuAction,
  onTaskMenuAction,
  sheets,
  icons,
  tagRename,
  noteHistory,
  noteTrash,
  noteProperties,
  rag,
  sync,
  vaultSearch,
  captureSheet,
  exportSheet,
  importSheet,
  palette,
  noteTriggers,
  bubble,
  linkScan,
  fieldScan,
  taskScan,
  fieldEntries,
  inboxTasks,
  noteGraph,
  onJumpInboxTask,
  onOpenAiHit,
  t,
}: NoteOverlaysProps) {
  const pickerNoteId = icons.picker?.noteId ?? "";
  return (
    <>
      {contextMenu && <NoteContextMenu state={contextMenu} onAction={onContextMenuAction} t={t} />}
      {listMenu && <NoteListContextMenu state={listMenu} onAction={onListMenuAction} t={t} />}
      {icons.picker && (
        <NoteIconPicker
          state={icons.picker}
          current={noteIconOf(icons.table, vault ?? "", icons.picker.noteId)}
          onPick={(icon) => icons.pick(pickerNoteId, icon)}
          onClose={icons.closePicker}
          t={t}
        />
      )}
      {tagRename.state && (
        <TagRenameDialog
          state={tagRename.state}
          report={tagRename.report}
          running={tagRename.running}
          error={tagRename.error}
          onSubmit={tagRename.submit}
          onClose={tagRename.close}
          t={t}
        />
      )}
      {noteHistory.state && noteHistory.note && (
        <NoteHistorySheet
          noteTitle={noteHistory.note.title || t("notebook.untitled")}
          entries={noteHistory.state.entries}
          selectedId={noteHistory.state.selectedId}
          snapshotContent={noteHistory.state.snapshot?.content ?? null}
          currentContent={noteFileContent(toVaultNote(noteHistory.note))}
          loading={noteHistory.state.loading}
          snapshotLoading={noteHistory.state.snapshotLoading}
          restoring={noteHistory.state.restoring}
          error={noteHistory.state.error}
          onSelect={noteHistory.select}
          onRestore={noteHistory.restore}
          onClose={noteHistory.close}
          t={t}
        />
      )}
      {/* 回收站和历史面板互斥(`openTrash` 会先关掉历史),所以这里不必再判一次谁在上面。 */}
      {noteTrash.state && (
        <NoteTrashSheet
          items={noteTrash.state.items}
          loading={noteTrash.state.loading}
          busyId={noteTrash.state.busyId}
          purgingAll={noteTrash.state.purgingAll}
          error={noteTrash.state.error}
          onRestore={noteTrash.restore}
          onPurge={noteTrash.purge}
          onPurgeAll={noteTrash.purgeAll}
          onClose={noteTrash.close}
          t={t}
        />
      )}
      {sheets.isOpen("fields") && (
        <NoteFieldsSheet
          entries={fieldEntries}
          loading={fieldScan.loading}
          error={fieldScan.error}
          onOpenNote={(path) => {
            setActiveId(path);
            // 跳过去就把 sheet 收掉:它铺满面板,留着的话用户点了一篇笔记却什么都
            // 没看见。
            sheets.close();
          }}
          onClose={sheets.close}
          t={t}
        />
      )}
      {noteProperties.state && noteProperties.note && (
        <NotePropertiesSheet
          noteTitle={noteProperties.note.title || t("notebook.untitled")}
          notePath={noteProperties.note.id}
          relativePath={vaultRelativePath(vault, noteProperties.note.id)}
          stat={noteProperties.state.stat}
          loading={noteProperties.state.loading}
          error={noteProperties.state.error}
          // 统计走 `noteStats`(= 编辑器里的当前文本):打开属性时已经切到了这条
          // 笔记,所以这两者说的是同一篇。
          words={noteStats.words}
          headings={noteStats.outline.length}
          readingMinutes={noteStats.readingMinutes}
          vault={noteProperties.state.vault}
          vaultLoading={noteProperties.state.vaultLoading}
          vaultError={noteProperties.state.vaultError}
          onClose={noteProperties.close}
          t={t}
        />
      )}
      {sheets.isOpen("graph") && (
        <NoteGraphSheet
          graph={noteGraph}
          focusPath={activeNote?.id ?? null}
          loading={linkScan.loading}
          error={linkScan.error}
          depth={sheets.graphDepth}
          onDepthChange={sheets.setGraphDepth}
          onOpenNote={(path) => {
            setActiveId(path);
            // 跳过去就收掉,理由同字段浏览器:它铺满面板。
            sheets.close();
          }}
          onRefresh={linkScan.refresh}
          onClose={sheets.close}
          t={t}
        />
      )}
      {/* 任务收集箱。`close` 一并清掉那个 fixed 定位的右键菜单 —— 见 `useNoteSheets`
          的不变量 3。 */}
      {sheets.isOpen("taskInbox") && (
        <NoteTaskInboxSheet
          tasks={inboxTasks}
          loading={taskScan.loading}
          error={taskScan.error}
          onJump={onJumpInboxTask}
          onRefresh={taskScan.refresh}
          onClose={sheets.close}
          onContextMenu={(task, anchor) => sheets.setTaskMenu({ x: anchor.x, y: anchor.y, task })}
          t={t}
        />
      )}
      {sheets.taskMenu && (
        <NoteTaskContextMenu state={sheets.taskMenu} onAction={onTaskMenuAction} t={t} />
      )}
      {/* 语义检索。见不变量 2:它排在收集箱之后、全库搜索之前。 */}
      {sheets.isOpen("ai") && (
        <NoteAiSheet
          stats={rag.stats}
          progress={rag.progress}
          query={rag.query}
          onQueryChange={rag.setQuery}
          hits={rag.hits}
          searched={rag.searched}
          searching={rag.searching}
          degraded={rag.degraded}
          vectorsMissing={rag.vectorsMissing}
          context={rag.context}
          contextBusy={rag.contextBusy}
          copied={rag.copied}
          error={rag.error}
          onSearch={rag.search}
          /* 当前笔记传编辑器里的内容而不是让后端读盘:用户问的往往正是刚写下还没
             保存的那几行。没有打开的笔记时传 null(检索仍然有意义)。 */
          onBuildContext={() =>
            rag.buildContext(activeNote ? { path: activeNote.id, body: activeNote.body } : null)
          }
          onCopyContext={rag.copyContext}
          onIndex={rag.index}
          onCancelIndex={rag.cancel}
          onClearIndex={rag.clear}
          onOpenHit={onOpenAiHit}
          onClose={sheets.close}
          t={t}
        />
      )}
      {sheets.isOpen("sync") && (
        <NoteSyncSheet
          remotes={sync.remotes}
          activeId={sync.activeId}
          report={sync.report}
          stale={sync.stale}
          decided={sync.decided}
          running={sync.running}
          error={sync.error}
          onSelectRemote={sync.selectRemote}
          onToggleAuto={sync.toggleAuto}
          onSync={() => sync.sync()}
          onDecide={sync.decide}
          onUndecide={sync.undecide}
          onClose={sheets.close}
          t={t}
        />
      )}
      {/* 全库搜索排在所有 sheet 之后,见不变量 2。 */}
      {vaultSearch.open && (
        <NoteSearchSheet
          query={vaultSearch.query}
          onQueryChange={vaultSearch.setQuery}
          flags={vaultSearch.flags}
          onFlagsChange={vaultSearch.setFlags}
          hits={vaultSearch.hits}
          loading={vaultSearch.loading}
          error={vaultSearch.error}
          capped={vaultSearch.capped}
          searched={vaultSearch.searched}
          onSubmit={vaultSearch.runSearch}
          onOpen={vaultSearch.openHit}
          onClose={vaultSearch.closeSheet}
          inputRef={vaultSearch.inputRef}
          replace={
            <NoteVaultReplaceBar
              value={vaultSearch.replaceValue}
              onValueChange={vaultSearch.setReplaceValue}
              preview={vaultSearch.preview}
              excluded={vaultSearch.excluded}
              onToggleFile={vaultSearch.toggleFile}
              busy={vaultSearch.busy}
              summary={vaultSearch.summary}
              canPreview={vaultSearch.canPreview}
              onPreview={vaultSearch.runPreview}
              onApply={vaultSearch.apply}
              t={t}
            />
          }
          t={t}
        />
      )}
      {/* 快速捕获排在命令面板前面,见不变量 2、4。 */}
      {captureSheet.open && vault && (
        <NoteQuickCapture
          paths={captureSheet.paths()}
          busy={captureSheet.busy}
          error={captureSheet.error}
          onSubmit={captureSheet.submit}
          onClose={captureSheet.closeSheet}
          t={t}
        />
      )}
      {exportSheet.open && (
        <NoteExportSheet
          hasNote={activeNote !== null}
          busy={exportSheet.busy}
          progress={exportSheet.progress}
          notice={exportSheet.notice}
          error={exportSheet.error}
          onRun={exportSheet.run}
          onCancelSite={exportSheet.cancelSite}
          onClose={exportSheet.closeSheet}
          t={t}
        />
      )}
      {importSheet.open && (
        <NoteImportSheet
          providers={importSheet.providers}
          busy={importSheet.busy}
          report={importSheet.report}
          error={importSheet.error}
          onRun={importSheet.run}
          onClose={importSheet.closeSheet}
          t={t}
        />
      )}
      {/* 命令面板排在最后、z-index 最高,见不变量 2。 */}
      {palette.open && (
        <NoteCommandPalette
          query={palette.query}
          onQueryChange={palette.setQuery}
          entries={palette.entries}
          selected={palette.selected}
          onSelectedChange={palette.setSelected}
          onRun={palette.runEntry}
          onClose={palette.closePalette}
          inputRef={palette.inputRef}
          t={t}
        />
      )}
      {/* 触发式菜单。`fixed` + 跟着光标坐标走,不参与上面的层叠;命令面板开着时要收起,
          见不变量 5。 */}
      {noteTriggers.state && !palette.open && (
        <NoteTriggerMenu
          kind={noteTriggers.state.kind}
          query={noteTriggers.state.query}
          rows={noteTriggers.rows}
          selected={noteTriggers.selected}
          onSelectedChange={noteTriggers.setSelected}
          onPick={noteTriggers.commitRow}
          onDismiss={noteTriggers.close}
          anchor={noteTriggers.state.coords}
          t={t}
        />
      )}
      {/* 选区浮动气泡。
          和触发菜单不用在这里互斥:触发菜单只在空选区下弹,气泡只在非空选区下画,
          两者由选区自己分开(有回归测试守着)。再加一道 `!trigger` 就是第二道闸门,
          谁都不是决定性的。
          命令面板要挡,理由见不变量 5。 */}
      {bubble.anchor && !palette.open && (
        <NoteBubbleMenu
          anchor={bubble.anchor}
          onAction={bubble.runAction}
          onDismiss={bubble.dismiss}
          t={t}
        />
      )}
    </>
  );
}
