import { describe, expect, it } from "vitest";
import {
  createDatabaseWorkspaceStore,
  type WorkspaceTab,
} from "../components/database/databaseWorkspaceStore";
import { createDebugPanelStore } from "../components/debug/debugPanelStore";
import { createNotebookStore } from "../components/notebook/notebookStore";
import type { DebugSessionSnapshot } from "../types";

describe("scoped workspace stores", () => {
  it("keeps database instances isolated and resets nested state", () => {
    const first = createDatabaseWorkspaceStore();
    const second = createDatabaseWorkspaceStore();
    const tab: WorkspaceTab = { id: "query-1", mode: "query", label: "Query", closable: true };

    first.getState().setWorkspaceTabs([tab]);
    first.getState().setActiveTabId(tab.id);
    first.getState().setActiveDbxConnectionId("connection-1");

    expect(second.getState().workspace.tabs).toEqual([]);
    expect(second.getState().navigation.activeDbxConnectionId).toBeNull();

    first.getState().reset();
    expect(first.getState().workspace.tabs).toEqual([]);
    expect(first.getState().workspace.shortTabIds).not.toBe(
      second.getState().workspace.shortTabIds,
    );
  });

  it("applies functional debug session updates without sharing instances", () => {
    const first = createDebugPanelStore();
    const second = createDebugPanelStore();
    const snapshot = (debugId: string): DebugSessionSnapshot => ({
      debugId,
      configId: "cfg",
      name: debugId,
      program: "/bin/app",
      cwd: "/repo",
      status: "running",
      output: "",
      callStack: [],
      scopes: [],
      startedAt: 1,
    });

    first.getState().setSessions([snapshot("a")]);
    // updater 必须收到当前数组并把返回值落库,而不是把函数本身存进 state。
    first.getState().setSessions((sessions) => [...sessions, snapshot("b")]);

    expect(first.getState().sessions.map((session) => session.debugId)).toEqual(["a", "b"]);
    expect(second.getState().sessions).toEqual([]);

    first.getState().setWatchDraft("count");
    first.getState().reset();
    expect(first.getState().watchDraft).toBe("");
    expect(first.getState().sessions).toEqual([]);
  });

  it("hydrates and updates notebook state through functional actions", () => {
    const store = createNotebookStore();
    const note = {
      // 笔记落盘后 id 就是文件的绝对路径。
      id: "/vault/note-1.md",
      title: "Plan",
      body: "",
      format: "markdown" as const,
      updatedAt: 1,
      sig: null,
      frontmatter: { title: "Plan", editor: "markdown" as const, extra: [] },
      loaded: true,
    };
    store.getState().hydrate([note]);
    store.getState().setNotes((notes) => [{ ...notes[0], title: "Updated" }]);
    expect(store.getState().activeId).toBe(note.id);
    expect(store.getState().notes[0].title).toBe("Updated");
  });
});
