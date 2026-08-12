import { describe, expect, it } from "vitest";
import {
  createDatabaseWorkspaceStore,
  type WorkspaceTab,
} from "../components/database/databaseWorkspaceStore";
import { createDebugPanelStore } from "../components/debug/debugPanelStore";
import { createNotebookStore } from "../components/notebook/notebookStore";

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
    first.getState().setWatchDraft("count");
    first.getState().setSessions((sessions) => sessions);
    expect(first.getState().watchDraft).toBe("count");
    expect(second.getState().watchDraft).toBe("");
  });

  it("hydrates and updates notebook state through functional actions", () => {
    const store = createNotebookStore();
    const note = {
      id: "note-1",
      title: "Plan",
      body: "",
      format: "markdown" as const,
      updatedAt: 1,
    };
    store.getState().hydrate([note]);
    store.getState().setNotes((notes) => [{ ...notes[0], title: "Updated" }]);
    expect(store.getState().activeId).toBe(note.id);
    expect(store.getState().notes[0].title).toBe("Updated");
  });
});
