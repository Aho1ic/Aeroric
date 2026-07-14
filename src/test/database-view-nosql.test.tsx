import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { DatabaseView } from "../components/database/DatabaseView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
}));

import { connection, resetDatabaseViewMocks } from "./databaseViewTestUtils";

describe("DatabaseView NoSQL workflows", () => {
  beforeEach(resetDatabaseViewMocks);
  it("lazy-loads Redis keys from the sidebar and opens the selected key in the workspace", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    const redisConnection = {
      id: "redis-source",
      name: "Redis Source",
      dbType: "redis",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([redisConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 2 }]);
      if (command === "dbx_redis_scan_keys") {
        const request = args as { cursor?: number };
        if (request.cursor === 7) {
          return Promise.resolve({
            cursor: 0,
            total_keys: 2,
            keys: [
              {
                key_display: "user:2",
                key_raw: "user:2",
                key_type: "string",
                ttl: -1,
                size: 5,
                value_preview: "Grace",
              },
            ],
          });
        }
        return Promise.resolve({
          cursor: 7,
          total_keys: 2,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: -1,
              size: 3,
              value_preview: "Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "user:1",
          key_raw: "user:1",
          key_type: "string",
          ttl: -1,
          value_is_binary: false,
          value: "Ada",
        });
      }
      if (command === "dbx_redis_delete_key") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Redis Source/i }));
    const redisDatabase = await screen.findByRole("button", { name: /db02/i });
    const expandGlyph = redisDatabase.querySelector("span") as HTMLSpanElement;
    fireEvent.click(expandGlyph);
    const sidebar = screen.getByRole("complementary");
    await within(sidebar).findByRole("button", { name: /user:1 string/i });
    await user.click(await within(sidebar).findByRole("button", { name: /Load more \(1\/2\)/i }));
    expect(
      await within(sidebar).findByRole("button", { name: /user:2 string/i }),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis-source",
      db: 0,
      cursor: 7,
      pattern: "*",
      count: 100,
    });
    const redisKey = within(sidebar).getByRole("button", {
      name: /user:1 string/i,
    }) as HTMLButtonElement;
    await user.click(redisKey);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_get_value", {
        connectionId: "redis-source",
        db: 0,
        keyRaw: "user:1",
      });
    });
    expect(await screen.findByText("Ada")).toBeInTheDocument();

    fireEvent.contextMenu(redisKey);
    expect(screen.getByRole("menuitem", { name: "Open workspace" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Refresh" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Delete key" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Copy name" }));
    expect(writeText).toHaveBeenCalledWith("user:1");

    fireEvent.contextMenu(redisKey);
    await user.click(screen.getByRole("menuitem", { name: "Refresh" }));
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis-source",
      db: 0,
      cursor: 0,
      pattern: "*",
      count: 100,
    });

    fireEvent.contextMenu(redisKey);
    await user.click(screen.getByRole("menuitem", { name: "Delete key" }));
    expect(confirm).toHaveBeenCalledWith('Delete Redis key "user:1"?', {
      title: "Delete key",
      kind: "warning",
      okLabel: "Delete key",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
      connectionId: "redis-source",
      db: 0,
      keyRaw: "user:1",
    });
  });

  it("lazy-loads MongoDB document previews from the sidebar and opens the selected document", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mongoConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { skip?: number; limit?: number };
        if (request.limit === 20 && request.skip === 1) {
          return Promise.resolve({ documents: [{ _id: "2", name: "Grace" }], total: 2 });
        }
        if (request.limit === 100) {
          return Promise.resolve({
            documents: [
              { _id: "1", name: "Ada" },
              { _id: "2", name: "Grace" },
            ],
            total: 2,
          });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 2 });
      }
      if (command === "dbx_mongo_delete_documents") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Mongo Source/i }));
    const sidebar = screen.getByRole("complementary");
    const databaseButtons = await within(sidebar).findAllByRole("button", { name: /^app$/i });
    const databaseButton =
      databaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      databaseButtons[0];
    await user.click(databaseButton);
    const databaseExpandGlyph = databaseButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(databaseExpandGlyph);

    const collectionButton = await within(sidebar).findByRole("button", { name: /^users$/i });
    const expandGlyph = collectionButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(expandGlyph);
    await user.click(await screen.findByRole("button", { name: /Load more \(1\/2\)/i }));
    expect(await screen.findByText(/2 name: Grace/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      projection: "{}",
      sort: "{}",
      skip: 1,
      limit: 20,
    });

    const documentButton = (await screen.findByText(/1 name: Ada/)).closest(
      "button",
    ) as HTMLButtonElement;
    await user.click(documentButton);

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      projection: "{}",
      sort: "{}",
      skip: 0,
      limit: 20,
    });
    await waitFor(() => {
      expect(screen.getByLabelText(/Document JSON|文档 JSON/)).toHaveValue(
        JSON.stringify({ _id: "1", name: "Ada" }, null, 2),
      );
    });
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      projection: "{}",
      sort: "{}",
      skip: 0,
      limit: 100,
    });

    fireEvent.contextMenu(documentButton);
    expect(screen.getByRole("menuitem", { name: "Open workspace" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Refresh" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Delete document" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Copy name" }));
    expect(writeText).toHaveBeenCalledWith("1");

    fireEvent.contextMenu(documentButton);
    await user.click(screen.getByRole("menuitem", { name: "Refresh" }));
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      projection: "{}",
      sort: "{}",
      skip: 0,
      limit: 20,
    });

    fireEvent.contextMenu(documentButton);
    await user.click(screen.getByRole("menuitem", { name: "Delete document" }));
    expect(confirm).toHaveBeenCalledWith('Delete document "1" from "users"?', {
      title: "Delete document",
      kind: "warning",
      okLabel: "Delete document",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_delete_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filterJson: JSON.stringify({ _id: "1" }),
      many: false,
    });
  });

  it("refreshes MongoDB sidebar document previews with the workspace filter", async () => {
    const user = userEvent.setup();
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mongoConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { filter?: string; skip?: number; limit?: number };
        if (request.filter === '{"active":true}' && request.limit === 20 && request.skip === 1) {
          return Promise.resolve({
            documents: [{ _id: "2", name: "Grace", active: true }],
            total: 2,
          });
        }
        if (request.filter === '{"active":true}' && request.limit === 20) {
          return Promise.resolve({
            documents: [{ _id: "1", name: "Ada", active: true }],
            total: 2,
          });
        }
        if (request.limit === 100) {
          return Promise.resolve({
            documents: [{ _id: "1", name: "Ada", active: true }],
            total: 1,
          });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Mongo Source/i }));
    const sidebar = screen.getByRole("complementary");
    const databaseButtons = await within(sidebar).findAllByRole("button", { name: /^app$/i });
    const databaseButton =
      databaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      databaseButtons[0];
    await user.click(databaseButton);
    const databaseExpandGlyph = databaseButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(databaseExpandGlyph);
    await user.click(await within(sidebar).findByRole("button", { name: /^users$/i }));

    const filterInput = await screen.findByLabelText(/Filter JSON|过滤 JSON/);
    fireEvent.change(filterInput, { target: { value: '{"active":true}' } });
    fireEvent.keyDown(filterInput, { key: "Enter", code: "Enter" });

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
        connectionId: "mongo-source",
        database: "app",
        collection: "users",
        filter: '{"active":true}',
        projection: "{}",
        sort: "{}",
        skip: 0,
        limit: 20,
      });
    });

    await user.click(await screen.findByRole("button", { name: /Load more \(1\/2\)/i }));

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: '{"active":true}',
      projection: "{}",
      sort: "{}",
      skip: 1,
      limit: 20,
    });
    expect(await screen.findByText(/2 name: Grace/)).toBeInTheDocument();
  });

  it("shows guidance instead of silently ignoring DBX-only toolbar buttons without a DBX selection", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /Data transfer/i }));
    expect(
      screen.getAllByText("Select a SQL-capable DBX connection to use this tool.").length,
    ).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /Table structure/i }));
    expect(
      screen.getAllByText("Select a DBX SQL table to inspect or edit its structure.").length,
    ).toBeGreaterThan(0);
  });
});
