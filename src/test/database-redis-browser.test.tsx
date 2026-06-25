import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { RedisBrowser } from "../components/database/RedisBrowser";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: vi.fn() }));

describe("RedisBrowser", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    window.localStorage.clear();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
  });

  it("loads Redis databases and scans keys", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 2 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
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
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    expect(await screen.findByText("db0")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /user 1/i })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /^user/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();
  });

  it("clears stale Redis workspace state when switching connections", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") {
        const request = args as { connectionId?: string };
        return Promise.resolve(
          request.connectionId === "redis-b" ? [{ db: 2, keys: 0 }] : [{ db: 0, keys: 1 }],
        );
      }
      if (command === "dbx_redis_scan_keys") {
        const request = args as { connectionId?: string };
        if (request.connectionId === "redis-b")
          return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
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
      return Promise.resolve(undefined);
    });

    const { rerender } = render(
      <I18nProvider>
        <RedisBrowser connectionId="redis-a" readOnly={false} />
      </I18nProvider>,
    );

    expect(await screen.findByText("db0")).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: /user 1/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();

    rerender(
      <I18nProvider>
        <RedisBrowser connectionId="redis-b" readOnly={false} />
      </I18nProvider>,
    );

    expect(await screen.findByText("db2")).toBeInTheDocument();
    expect(screen.queryByText("db0")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /user:1 string/i })).not.toBeInTheDocument();
    expect(screen.queryByText("Ada")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("Ada")).not.toBeInTheDocument();
  });

  it("clears stale Redis key and value state when switching databases", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") {
        return Promise.resolve([
          { db: 0, keys: 1 },
          { db: 1, keys: 0 },
        ]);
      }
      if (command === "dbx_redis_scan_keys") {
        const request = args as { db?: number };
        if (request.db === 1) return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
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
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    expect(await screen.findByText("db0")).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: /user 1/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "db1" }));

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /user:1 string/i })).not.toBeInTheDocument(),
    );
    expect(screen.queryByText("Ada")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("Ada")).not.toBeInTheDocument();
  });

  it("creates a Redis key and runs a Redis command", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_create_key") return Promise.resolve(undefined);
      if (command === "dbx_redis_execute_command") {
        return Promise.resolve({ command: "GET", safety: "allowed", value: "Ada" });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await screen.findByText("db0");
    await userEvent.click(screen.getByRole("button", { name: /新建键|Create key/ }));
    await userEvent.type(screen.getByLabelText(/Redis key name/i), "user:1");
    await userEvent.type(screen.getByLabelText(/Redis create value/i), "Ada");
    await userEvent.click(screen.getByRole("button", { name: /保存键|Save key/ }));

    expect(invoke).toHaveBeenCalledWith("dbx_redis_create_key", {
      request: expect.objectContaining({
        connectionId: "redis",
        db: 0,
        keyRaw: "user:1",
        keyType: "string",
        value: "Ada",
      }),
    });

    await userEvent.type(screen.getByLabelText(/Redis 命令|Redis command/), "GET user:1");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));

    expect(invoke).toHaveBeenCalledWith("dbx_redis_execute_command", {
      connectionId: "redis",
      db: 0,
      command: "GET user:1",
      skipSafetyCheck: false,
    });
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();
  });

  it("blocks unsafe Redis commands before invoking Tauri", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await screen.findByText("db0");
    await userEvent.type(screen.getByLabelText(/Redis 命令|Redis command/), "KEYS *");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));

    expect(
      await screen.findByText(/Redis command blocked|Redis 安全策略已阻止/),
    ).toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_execute_command"),
    ).toBe(false);
  });

  it("confirms destructive Redis commands and executes them with the safety override", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_execute_command")
        return Promise.resolve({ command: "DEL", safety: "confirm", value: 1 });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await screen.findByText("db0");
    await userEvent.type(screen.getByLabelText(/Redis 命令|Redis command/), "DEL user:1");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));

    expect(confirm).toHaveBeenCalledWith("Run this Redis command?\n\nDEL user:1", {
      title: "Confirm Redis command",
      kind: "warning",
      okLabel: "Run command",
      cancelLabel: "Cancel",
    });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_execute_command", {
        connectionId: "redis",
        db: 0,
        command: "DEL user:1",
        skipSafetyCheck: true,
      });
    });
    expect(await screen.findByText("1")).toBeInTheDocument();
  });

  it("does not execute destructive Redis commands when confirmation is cancelled", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await screen.findByText("db0");
    await userEvent.type(screen.getByLabelText(/Redis 命令|Redis command/), "SET user:1 Ada");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));

    expect(confirm).toHaveBeenCalled();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_execute_command"),
    ).toBe(false);
  });

  it("confirms and flushes the current Redis database from the workspace", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        const request = args as { cursor?: number };
        if (request.cursor && request.cursor > 0)
          return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
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
      if (command === "dbx_redis_execute_command")
        return Promise.resolve({ command: "FLUSHDB", safety: "confirm", value: "OK" });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await screen.findByText("db0");
    await userEvent.click(screen.getByRole("button", { name: /Clear current DB|清空当前 DB/ }));

    expect(confirm).toHaveBeenCalledWith(
      "This will delete every key in Redis db0 and cannot be undone. Continue?",
      {
        title: "Clear current DB",
        kind: "warning",
        okLabel: "Clear DB",
        cancelLabel: "Cancel",
      },
    );
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_execute_command", {
        connectionId: "redis",
        db: 0,
        command: "FLUSHDB",
        skipSafetyCheck: true,
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis",
      db: 0,
      cursor: 0,
      pattern: "*",
      count: 100,
    });
  });

  it("updates the Redis command prompt after a successful SELECT command", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases")
        return Promise.resolve([
          { db: 0, keys: 0 },
          { db: 2, keys: 0 },
        ]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_execute_command") {
        const request = args as { command: string };
        if (request.command === "SELECT 2")
          return Promise.resolve({ command: "SELECT", safety: "allowed", value: "OK" });
        return Promise.resolve({ command: "GET", safety: "allowed", value: "Grace" });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const commandInput = await screen.findByLabelText(/Redis 命令|Redis command/);
    await userEvent.type(commandInput, "SELECT 2");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));
    expect(await screen.findByText("db2>")).toBeInTheDocument();

    await userEvent.type(commandInput, "GET user:2");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_execute_command", {
        connectionId: "redis",
        db: 2,
        command: "GET user:2",
        skipSafetyCheck: false,
      });
    });
    expect(await screen.findByText("Grace")).toBeInTheDocument();
  });

  it("clears the Redis command terminal with clear without invoking Tauri", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_execute_command")
        return Promise.resolve({ command: "GET", safety: "allowed", value: "Ada" });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const commandInput = await screen.findByLabelText(/Redis 命令|Redis command/);
    await userEvent.type(commandInput, "GET user:1");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();
    const executeCallsBeforeClear = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "dbx_redis_execute_command").length;

    await userEvent.type(commandInput, "clear");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));

    expect(screen.queryByText("Ada")).not.toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "dbx_redis_execute_command"),
    ).toHaveLength(executeCallsBeforeClear);
  });

  it("persists Redis command terminal history for the connection", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_execute_command")
        return Promise.resolve({ command: "GET", safety: "allowed", value: "Ada" });
      return Promise.resolve(undefined);
    });

    const firstRender = render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const commandInput = await screen.findByLabelText(/Redis 命令|Redis command/);
    await userEvent.type(commandInput, "GET user:1");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();

    firstRender.unmount();
    vi.mocked(invoke).mockClear();

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    expect(await screen.findByText("Ada")).toBeInTheDocument();
    expect(screen.getByText("GET user:1")).toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_execute_command"),
    ).toBe(false);
  });

  it("clears persisted Redis command terminal history from the toolbar action", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_execute_command")
        return Promise.resolve({ command: "GET", safety: "allowed", value: "Ada" });
      return Promise.resolve(undefined);
    });

    const firstRender = render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const commandInput = await screen.findByLabelText(/Redis 命令|Redis command/);
    await userEvent.type(commandInput, "GET user:1");
    await userEvent.click(screen.getByRole("button", { name: /运行命令|Run command/ }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Clear history|清空历史/ }));
    expect(screen.queryByText("Ada")).not.toBeInTheDocument();

    firstRender.unmount();

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await screen.findByText("db0");
    expect(screen.queryByText("Ada")).not.toBeInTheDocument();
    expect(screen.queryByText("GET user:1")).not.toBeInTheDocument();
  });

  it("opens a Redis key context menu and confirms key deletion", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
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
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    const keyButton = screen.getByRole("button", { name: /user:1 string/i }) as HTMLButtonElement;
    fireEvent.contextMenu(keyButton);
    expect(screen.getByRole("menuitem", { name: /Copy name|复制名称/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Refresh|刷新/ })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("menuitem", { name: /Copy name|复制名称/ }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("user:1");

    fireEvent.contextMenu(keyButton);
    await userEvent.click(screen.getByRole("menuitem", { name: /Refresh|刷新/ }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_get_value", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:1",
      });
    });

    fireEvent.contextMenu(keyButton);
    const deleteKeyItem = screen.getByRole("menuitem", { name: /Delete key|删除键/ });
    expect(deleteKeyItem).toHaveStyle({
      background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
      borderRadius: "8px",
    });
    await userEvent.click(deleteKeyItem);
    expect(confirm).toHaveBeenCalledWith('Delete Redis key "user:1"?', {
      title: "Delete key",
      kind: "warning",
      okLabel: "Delete key",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
      connectionId: "redis",
      db: 0,
      keyRaw: "user:1",
    });
  });

  it("opens a Redis key group context menu and deletes loaded group keys after confirmation", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 3 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 3,
          keys: [
            {
              key_display: "order:1",
              key_raw: "order:1",
              key_type: "hash",
              ttl: -1,
              size: 2,
              value_preview: "",
            },
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: -1,
              size: 3,
              value_preview: "Ada",
            },
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
      if (command === "dbx_redis_delete_key") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const userGroup = await screen.findByRole("button", { name: /^user 2$/i });
    fireEvent.contextMenu(userGroup);
    expect(
      screen.getByRole("menuitem", { name: /Delete key group|删除键分组/ }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("menuitem", { name: /Copy name|复制名称/ }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("user");

    fireEvent.contextMenu(userGroup);
    await userEvent.click(screen.getByRole("menuitem", { name: /Refresh|刷新/ }));
    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.filter(([command]) => command === "dbx_redis_scan_keys"),
      ).toHaveLength(2);
    });

    fireEvent.contextMenu(userGroup);
    const deleteGroupItem = screen.getByRole("menuitem", { name: /Delete key group|删除键分组/ });
    expect(deleteGroupItem).toHaveStyle({
      background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
      borderRadius: "8px",
    });
    await userEvent.click(deleteGroupItem);
    expect(confirm).toHaveBeenCalledWith('Delete 2 loaded Redis keys in group "user"?', {
      title: "Delete key group",
      kind: "warning",
      okLabel: "Delete key group",
      cancelLabel: "Cancel",
    });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:1",
      });
      expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:2",
      });
    });
    expect(invoke).not.toHaveBeenCalledWith("dbx_redis_delete_key", {
      connectionId: "redis",
      db: 0,
      keyRaw: "order:1",
    });
  });

  it("selects loaded Redis keys and deletes the selected set after one confirmation", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 3 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 3,
          keys: [
            {
              key_display: "order:1",
              key_raw: "order:1",
              key_type: "hash",
              ttl: -1,
              size: 2,
              value_preview: "",
            },
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: -1,
              size: 3,
              value_preview: "Ada",
            },
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
      if (command === "dbx_redis_delete_key") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user 2$/i }));
    await userEvent.click(screen.getByLabelText(/Delete selected: user:1|删除已选: user:1/));
    await userEvent.click(screen.getByLabelText(/Delete selected: user:2|删除已选: user:2/));
    await userEvent.click(
      screen.getByRole("button", { name: /Delete selected \(2\)|删除已选 \(2\)/ }),
    );

    expect(confirm).toHaveBeenCalledWith("Delete 2 selected Redis keys?", {
      title: "Delete selected",
      kind: "warning",
      okLabel: "Delete selected",
      cancelLabel: "Cancel",
    });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:1",
      });
      expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:2",
      });
    });
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalledWith("dbx_redis_delete_key", {
      connectionId: "redis",
      db: 0,
      keyRaw: "order:1",
    });
  });

  it("supports DBX-style fuzzy Redis key pattern searches", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 2 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 0,
          keys: [],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const patternInput = await screen.findByLabelText(/Redis key pattern|Redis 键模式/);
    await userEvent.type(patternInput, "ser{enter}");

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
        connectionId: "redis",
        db: 0,
        cursor: 0,
        pattern: "ser",
        count: 100,
      });
    });

    await userEvent.click(screen.getByRole("button", { name: /Fuzzy|模糊/ }));
    await userEvent.type(patternInput, "{enter}");

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
        connectionId: "redis",
        db: 0,
        cursor: 0,
        pattern: "*ser*",
        count: 100,
      });
    });
  });

  it("ignores late Redis scan results from a previous key pattern", async () => {
    let resolveFooScan: (value: {
      cursor: number;
      total_keys: number;
      keys: unknown[];
    }) => void = () => undefined;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 2 }]);
      if (command === "dbx_redis_scan_keys") {
        const request = args as { pattern?: string };
        if (request.pattern === "foo") {
          return new Promise((resolve) => {
            resolveFooScan = resolve;
          });
        }
        if (request.pattern === "bar") {
          return Promise.resolve({
            cursor: 0,
            total_keys: 1,
            keys: [
              {
                key_display: "bar:1",
                key_raw: "bar:1",
                key_type: "string",
                ttl: -1,
                size: 3,
                value_preview: "Bar",
              },
            ],
          });
        }
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    const patternInput = await screen.findByLabelText(/Redis key pattern|Redis 键模式/);
    await userEvent.type(patternInput, "foo{enter}");
    await userEvent.clear(patternInput);
    await userEvent.type(patternInput, "bar{enter}");

    await userEvent.click(await screen.findByRole("button", { name: /bar\s*1/i }));
    expect(await screen.findByRole("button", { name: /bar:1 string/i })).toBeInTheDocument();

    resolveFooScan({
      cursor: 0,
      total_keys: 1,
      keys: [
        {
          key_display: "foo:1",
          key_raw: "foo:1",
          key_type: "string",
          ttl: -1,
          size: 3,
          value_preview: "Foo",
        },
      ],
    });

    expect(await screen.findByRole("button", { name: /bar:1 string/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /foo:1 string/i })).not.toBeInTheDocument();
  });

  it("loads additional Redis keys from the workspace", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
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
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    expect(await screen.findByRole("button", { name: /user:1 string/i })).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /Load more \(1\/2\)|加载更多 \(1\/2\)/ }),
    );

    expect(await screen.findByRole("button", { name: /user:2 string/i })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis",
      db: 0,
      pattern: "*",
      cursor: 7,
      count: 100,
    });
  });

  it("fetches all remaining Redis key pages from the workspace", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 3 }]);
      if (command === "dbx_redis_scan_keys") {
        const request = args as { cursor?: number };
        if (request.cursor === 5) {
          return Promise.resolve({
            cursor: 8,
            total_keys: 3,
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
        if (request.cursor === 8) {
          return Promise.resolve({
            cursor: 0,
            total_keys: 3,
            keys: [
              {
                key_display: "user:3",
                key_raw: "user:3",
                key_type: "string",
                ttl: -1,
                size: 4,
                value_preview: "Lin",
              },
            ],
          });
        }
        return Promise.resolve({
          cursor: 5,
          total_keys: 3,
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
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user 1$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Fetch all|获取全部/ }));

    expect(await screen.findByRole("button", { name: /user:2 string/i })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /user:3 string/i })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis",
      db: 0,
      pattern: "*",
      cursor: 5,
      count: 100,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis",
      db: 0,
      pattern: "*",
      cursor: 8,
      count: 100,
    });
  });

  it("shows DBX-style fetch-all progress and stops after the current Redis scan page", async () => {
    let resolveFetchPage: ((value: unknown) => void) | null = null;
    const resolvePendingFetchPage = (value: unknown) => {
      if (!resolveFetchPage) throw new Error("Redis fetch-all page was not requested");
      resolveFetchPage(value);
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 3 }]);
      if (command === "dbx_redis_scan_keys") {
        const request = args as { cursor?: number };
        if (request.cursor === 5) {
          return new Promise((resolve) => {
            resolveFetchPage = resolve;
          });
        }
        if (request.cursor === 8) {
          return Promise.resolve({
            cursor: 0,
            total_keys: 3,
            keys: [
              {
                key_display: "user:3",
                key_raw: "user:3",
                key_type: "string",
                ttl: -1,
                size: 4,
                value_preview: "Lin",
              },
            ],
          });
        }
        return Promise.resolve({
          cursor: 5,
          total_keys: 3,
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
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user 1$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Fetch all|获取全部/ }));

    expect(
      await screen.findByText(/1 of 3 keys loaded|已加载 1 \/ 共 3 条 key/),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Stop|停止/ }));

    resolvePendingFetchPage({
      cursor: 8,
      total_keys: 3,
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

    expect(await screen.findByRole("button", { name: /user:2 string/i })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByText(/keys loaded|条 key/)).not.toBeInTheDocument();
    });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === "dbx_redis_scan_keys" && (args as { cursor?: number })?.cursor === 8,
        ),
    ).toBe(false);
  });

  it("formats and compresses the selected Redis JSON value draft", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: -1,
              size: 32,
              value_preview: '{"name":"Ada","roles":["admin"]}',
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
          value: '{"name":"Ada","roles":["admin"]}',
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    const jsonTree = await screen.findByRole("tree", { name: /Redis JSON tree/ });
    expect(screen.getByRole("button", { name: /JSON view|JSON 视图/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(jsonTree).toHaveStyle("white-space: pre-wrap");
    expect(screen.getByText("Object(2)")).toBeInTheDocument();
    expect(screen.getByText('"name"')).toBeInTheDocument();
    expect(screen.getByText('"roles"')).toBeInTheDocument();
    const wordWrap = screen.getByRole("checkbox", { name: /Word wrap|自动换行/ });
    await userEvent.click(wordWrap);
    expect(jsonTree).toHaveStyle("white-space: pre");

    const rawButton = screen.getByRole("button", { name: /Raw content|原始内容/ });
    await userEvent.click(rawButton);
    expect(rawButton).toHaveAttribute("aria-pressed", "true");
    const valueEditor = await screen.findByRole("textbox", { name: /Redis value|Redis 值/ });
    expect(valueEditor).toHaveValue('{"name":"Ada","roles":["admin"]}');

    await userEvent.click(screen.getByRole("button", { name: /^Format$|^格式化$/ }));
    expect(valueEditor).toHaveValue('{\n  "name": "Ada",\n  "roles": [\n    "admin"\n  ]\n}');

    await userEvent.click(screen.getByRole("button", { name: /^Compress$|^压缩$/ }));
    expect(valueEditor).toHaveValue('{"name":"Ada","roles":["admin"]}');

    fireEvent.change(valueEditor, { target: { value: "{bad" } });
    await userEvent.click(screen.getByRole("button", { name: /^Format$|^格式化$/ }));

    expect(await screen.findByText(/Invalid JSON format|JSON 格式无效/)).toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_set_value"),
    ).toBe(false);
  });

  it("reloads the selected Redis value and key preview after saving a value draft", async () => {
    let currentValue = "Ada";
    let currentTtl = 60;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: currentTtl,
              size: currentValue.length,
              value_preview: currentValue,
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "user:1",
          key_raw: "user:1",
          key_type: "string",
          ttl: currentTtl,
          value_is_binary: false,
          value: currentValue,
        });
      }
      if (command === "dbx_redis_set_value") {
        const request = args as { value: string; ttl: number };
        currentValue = request.value;
        currentTtl = request.ttl;
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    const valueEditor = await screen.findByDisplayValue("Ada");
    expect(await screen.findByText(/Size: 3 B|大小: 3 B/)).toBeInTheDocument();
    fireEvent.change(valueEditor, { target: { value: "Grace" } });
    const ttlInput = screen.getByLabelText(/^Redis TTL$/);
    await userEvent.clear(ttlInput);
    await userEvent.type(ttlInput, "120");
    await userEvent.click(screen.getByRole("button", { name: /Discard|丢弃/ }));
    expect(valueEditor).toHaveValue("Ada");
    expect(ttlInput).toHaveValue("120");
    expect(screen.queryByRole("button", { name: /Discard|丢弃/ })).not.toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_set_value"),
    ).toBe(false);

    fireEvent.change(valueEditor, { target: { value: "Grace" } });
    await userEvent.click(screen.getByRole("button", { name: /Save value|保存值/ }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_set_value", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:1",
        value: "Grace",
        ttl: 120,
      });
      expect(invoke).toHaveBeenCalledWith("dbx_redis_get_value", {
        connectionId: "redis",
        db: 0,
        keyRaw: "user:1",
      });
      expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
        connectionId: "redis",
        db: 0,
        cursor: 0,
        pattern: "*",
        count: 100,
      });
    });
    expect(
      await screen.findByRole("button", { name: /TTL: 120s|TTL：120 秒/ }),
    ).toBeInTheDocument();
    expect(await screen.findByText(/Size: 5 B|大小: 5 B/)).toBeInTheDocument();
  });

  it("blocks saving a Redis value draft when the TTL is invalid", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: 60,
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
          ttl: 60,
          value_is_binary: false,
          value: "Ada",
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    expect(await screen.findByDisplayValue("Ada")).toBeInTheDocument();
    const ttlInput = screen.getByLabelText(/^Redis TTL$/);
    await userEvent.clear(ttlInput);
    await userEvent.type(ttlInput, "later");
    await userEvent.click(screen.getByRole("button", { name: /Save value|保存值/ }));

    expect(await screen.findByText(/Enter a valid TTL|请输入有效 TTL/)).toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_set_value"),
    ).toBe(false);
  });

  it("copies the selected Redis value and DBX-style insert statement", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: 60,
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
          ttl: 60,
          value_is_binary: false,
          value: "Ada",
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    expect(await screen.findByText("Ada")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Copy value|复制值/ }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("Ada");

    await userEvent.click(
      screen.getByRole("button", { name: /Copy insert statement|复制插入语句/ }),
    );
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      'SET "user:1" "Ada"\nEXPIRE "user:1" 60',
    );
  });

  it("renders DBX-style grouped Redis stream entries", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "events",
              key_raw: "events",
              key_type: "stream",
              ttl: -1,
              size: 2,
              value_preview: "1-0",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "events",
          key_raw: "events",
          key_type: "stream",
          ttl: -1,
          value_is_binary: false,
          total: 2,
          scan_cursor: 0,
          value: [
            { id: "1-0", fields: { name: "Ada", role: "admin" } },
            { id: "2-0", fields: { name: "Grace" } },
          ],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /events stream/i }));

    expect(await screen.findByText(/2 entries|2 条记录/)).toBeInTheDocument();
    expect(screen.queryByText(/3 entries|3 条记录/)).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: /Entry ID/i })).not.toBeInTheDocument();
    expect(screen.getAllByText("1-0")).toHaveLength(1);
    expect(screen.getAllByText("2-0").length).toBeGreaterThan(0);
    expect(screen.getByText("role")).toBeInTheDocument();
    expect(screen.getByText("admin")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /Redis value|Redis 值/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /^Redis TTL$/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Save value|保存值/ })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /Redis command|Redis 命令/ })).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: /Copy member: 1-0 .* role|复制成员: 1-0 .* role/ }),
    );
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("admin");

    await userEvent.click(
      screen.getByRole("button", {
        name: /View full value: 1-0 .* role|查看完整内容: 1-0 .* role/,
      }),
    );
    expect(
      screen.getByRole("dialog", { name: /Member detail: 1-0 .* role|成员详情: 1-0 .* role/ }),
    ).toBeInTheDocument();
  });

  it("renders DBX-style Redis hash member rows with detail and copy actions", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "profile:1",
              key_raw: "profile:1",
              key_type: "hash",
              ttl: -1,
              size: 3,
              value_preview: "name=Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "profile:1",
          key_raw: "profile:1",
          key_type: "hash",
          ttl: -1,
          value_is_binary: false,
          total: 3,
          scan_cursor: 0,
          value: [
            { field: "name", value: "Ada" },
            { field: "bio", value: '{"city":"Paris"}' },
          ],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^profile/i }));
    await userEvent.click(screen.getByRole("button", { name: /profile:1 hash/i }));

    expect(
      await screen.findByText(/2 of 3 fields loaded|已加载 2 \/ 共 3 个字段/),
    ).toBeInTheDocument();
    const fieldHeader = screen.getByRole("columnheader", { name: /Field|字段/ });
    expect(fieldHeader).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: /Value|值/ })).toBeInTheDocument();
    fireEvent.pointerDown(
      screen.getByRole("separator", { name: /Resize hash field column|调整 Hash 字段列宽/ }),
      { clientX: 100 },
    );
    fireEvent.pointerMove(window, { clientX: 180 });
    fireEvent.pointerUp(window);
    await waitFor(() => expect(fieldHeader).toHaveStyle("width: 260px"));
    expect(screen.getByText("name")).toBeInTheDocument();
    expect(screen.getByText("bio")).toBeInTheDocument();
    expect(screen.getByText('{"city":"Paris"}')).toBeInTheDocument();
    expect(
      screen.queryByRole("dialog", { name: /Member detail|成员详情/ }),
    ).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: /View full value: bio|查看完整内容: bio/ }),
    );
    const detailDialog = screen.getByRole("dialog", { name: /Member detail: bio|成员详情: bio/ });
    expect(detailDialog).toHaveStyle("position: fixed");
    expect(screen.getByText(/Member detail: bio|成员详情: bio/)).toBeInTheDocument();
    fireEvent.pointerDown(
      screen.getByRole("separator", { name: /Resize member detail|调整成员详情宽度/ }),
      { clientX: 100 },
    );
    fireEvent.pointerMove(window, { clientX: 20 });
    fireEvent.pointerUp(window);
    await waitFor(() => expect(detailDialog).toHaveStyle("width: 500px"));
    expect(screen.getByRole("button", { name: /JSON view|JSON 视图/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    const jsonTree = screen.getByRole("tree", { name: /Redis JSON tree/ });
    expect(jsonTree).toHaveStyle("white-space: pre-wrap");
    expect(screen.getByText("Object(1)")).toBeInTheDocument();
    expect(screen.getByText('"city"')).toBeInTheDocument();
    expect(screen.getByText('"Paris"')).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Collapse JSON \$/ }));
    expect(screen.queryByText('"city"')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Expand JSON \$/ }));
    expect(screen.getByText('"city"')).toBeInTheDocument();
    const wordWrap = screen.getByRole("checkbox", { name: /Word wrap|自动换行/ });
    expect(wordWrap).toBeChecked();
    await userEvent.click(wordWrap);
    expect(wordWrap).not.toBeChecked();
    expect(window.localStorage.getItem("dbx-redis-json-word-wrap")).toBe("false");
    expect(jsonTree).toHaveStyle("white-space: pre");

    await userEvent.click(
      within(detailDialog).getByRole("button", { name: /Copy member: bio|复制成员: bio/ }),
    );
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('{"city":"Paris"}');

    const rawButton = screen.getByRole("button", { name: /Raw content|原始内容/ });
    await userEvent.click(rawButton);
    expect(rawButton).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName.toLowerCase() === "pre" && element.textContent === '{"city":"Paris"}',
      ),
    ).toHaveStyle("white-space: pre");
    await userEvent.click(
      screen.getByRole("button", { name: /Format member JSON|格式化成员 JSON/ }),
    );
    const memberValue = screen.getByRole("textbox", { name: /Redis member value|Redis 成员值/ });
    expect(memberValue).toHaveValue('{\n  "city": "Paris"\n}');
    await userEvent.click(
      screen.getByRole("button", { name: /Compress member JSON|压缩成员 JSON/ }),
    );
    expect(memberValue).toHaveValue('{"city":"Paris"}');
    await userEvent.click(screen.getByRole("button", { name: /Close|关闭/ }));
    expect(
      screen.queryByRole("dialog", { name: /Member detail: bio|成员详情: bio/ }),
    ).not.toBeInTheDocument();
  });

  it("resizes DBX-style Redis sorted set score column", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "leaders",
              key_raw: "leaders",
              key_type: "zset",
              ttl: -1,
              size: 1,
              value_preview: "Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "leaders",
          key_raw: "leaders",
          key_type: "zset",
          ttl: -1,
          value_is_binary: false,
          total: 1,
          scan_cursor: 0,
          value: [{ score: 1, member: "Ada" }],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /leaders zset/i }));
    const scoreHeader = await screen.findByRole("columnheader", { name: /Score|分数/ });
    fireEvent.pointerDown(
      screen.getByRole("separator", {
        name: /Resize sorted set score column|调整有序集合分数列宽/,
      }),
      { clientX: 100 },
    );
    fireEvent.pointerMove(window, { clientX: 150 });
    fireEvent.pointerUp(window);
    await waitFor(() => expect(scoreHeader).toHaveStyle("width: 170px"));
    expect(screen.getByText("Ada")).toBeInTheDocument();
  });

  it("loads more Redis collection members from the selected value cursor", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "profile:1",
              key_raw: "profile:1",
              key_type: "hash",
              ttl: -1,
              size: 3,
              value_preview: "name=Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "profile:1",
          key_raw: "profile:1",
          key_type: "hash",
          ttl: -1,
          value_is_binary: false,
          total: 3,
          scan_cursor: 12,
          value: [{ field: "name", value: "Ada" }],
        });
      }
      if (command === "dbx_redis_load_more") {
        expect(args).toEqual({
          connectionId: "redis",
          db: 0,
          keyRaw: "profile:1",
          keyType: "hash",
          cursor: 12,
          count: 200,
        });
        return Promise.resolve({
          key_display: "profile:1",
          key_raw: "profile:1",
          key_type: "hash",
          ttl: -1,
          value_is_binary: false,
          total: null,
          scan_cursor: null,
          value: [
            { field: "role", value: "admin" },
            { field: "city", value: "Paris" },
          ],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^profile/i }));
    await userEvent.click(screen.getByRole("button", { name: /profile:1 hash/i }));

    expect(
      await screen.findByText(/1 of 3 fields loaded|已加载 1 \/ 共 3 个字段/),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /Load more \(1\/3\)|加载更多 \(1\/3\)/ }),
    );

    expect(await screen.findByText(/3 fields|3 个字段/)).toBeInTheDocument();
    expect(screen.getByText("role")).toBeInTheDocument();
    expect(screen.getByText("admin")).toBeInTheDocument();
    expect(screen.getByText("city")).toBeInTheDocument();
    expect(screen.getByText("Paris")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Load more|加载更多/ })).not.toBeInTheDocument();
  });

  it("deletes a Redis hash field from the DBX-style member row action", async () => {
    let deleted = false;
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "profile:1",
              key_raw: "profile:1",
              key_type: "hash",
              ttl: -1,
              size: deleted ? 1 : 2,
              value_preview: "name=Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "profile:1",
          key_raw: "profile:1",
          key_type: "hash",
          ttl: -1,
          value_is_binary: false,
          total: deleted ? 1 : 2,
          scan_cursor: 0,
          value: deleted
            ? [{ field: "name", value: "Ada" }]
            : [
                { field: "name", value: "Ada" },
                { field: "bio", value: "Paris" },
              ],
        });
      }
      if (command === "dbx_redis_hash_del") {
        expect(args).toEqual({
          connectionId: "redis",
          db: 0,
          keyRaw: "profile:1",
          field: "bio",
        });
        deleted = true;
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^profile/i }));
    await userEvent.click(screen.getByRole("button", { name: /profile:1 hash/i }));
    expect(await screen.findByText("bio")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Delete member: bio|删除成员: bio/ }));

    expect(confirm).toHaveBeenCalledWith(
      expect.stringMatching(/Delete field "bio"|删除 Redis 键/),
      expect.any(Object),
    );
    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_hash_del"),
      ).toBe(true);
      expect(screen.queryByText("bio")).not.toBeInTheDocument();
    });
    expect(screen.getByText("name")).toBeInTheDocument();
  });

  it("edits a Redis hash field from the DBX-style member detail action", async () => {
    let edited = false;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "profile:1",
              key_raw: "profile:1",
              key_type: "hash",
              ttl: -1,
              size: 2,
              value_preview: edited ? "bio=London" : "bio=Paris",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "profile:1",
          key_raw: "profile:1",
          key_type: "hash",
          ttl: -1,
          value_is_binary: false,
          total: 2,
          scan_cursor: 0,
          value: [
            { field: "name", value: "Ada" },
            { field: "bio", value: edited ? "London" : "Paris" },
          ],
        });
      }
      if (command === "dbx_redis_hash_set") {
        expect(args).toEqual({
          connectionId: "redis",
          db: 0,
          keyRaw: "profile:1",
          field: "bio",
          value: "London",
          ttl: null,
        });
        edited = true;
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^profile/i }));
    await userEvent.click(screen.getByRole("button", { name: /profile:1 hash/i }));
    expect(await screen.findByText("Paris")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Edit member: bio|编辑成员: bio/ }));
    const memberValue = screen.getByRole("textbox", { name: /Redis member value|Redis 成员值/ });
    await userEvent.clear(memberValue);
    await userEvent.type(memberValue, "London");
    await userEvent.click(screen.getByRole("button", { name: /Save member|保存成员/ }));

    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_hash_set"),
      ).toBe(true);
    });
    expect(await screen.findByText("London")).toBeInTheDocument();
    expect(screen.queryByText("Paris")).not.toBeInTheDocument();
  });

  it("adds a Redis hash field from the DBX-style selected value controls", async () => {
    let added = false;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "profile:1",
              key_raw: "profile:1",
              key_type: "hash",
              ttl: -1,
              size: added ? 2 : 1,
              value_preview: added ? "city=Paris" : "name=Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "profile:1",
          key_raw: "profile:1",
          key_type: "hash",
          ttl: -1,
          value_is_binary: false,
          total: added ? 2 : 1,
          scan_cursor: 0,
          value: added
            ? [
                { field: "name", value: "Ada" },
                { field: "city", value: "Paris" },
              ]
            : [{ field: "name", value: "Ada" }],
        });
      }
      if (command === "dbx_redis_hash_set") {
        expect(args).toEqual({
          connectionId: "redis",
          db: 0,
          keyRaw: "profile:1",
          field: "city",
          value: "Paris",
          ttl: null,
        });
        added = true;
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^profile/i }));
    await userEvent.click(screen.getByRole("button", { name: /profile:1 hash/i }));
    expect(await screen.findByText("name")).toBeInTheDocument();

    await userEvent.type(
      screen.getByRole("textbox", { name: /Redis new member field|Redis 新成员字段/ }),
      "city",
    );
    await userEvent.type(
      screen.getByRole("textbox", { name: /Redis new member value|Redis 新成员值/ }),
      "Paris",
    );
    await userEvent.click(screen.getByRole("button", { name: /Set|设置/ }));

    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_hash_set"),
      ).toBe(true);
    });
    expect(await screen.findByText("city")).toBeInTheDocument();
    expect(screen.getByText("Paris")).toBeInTheDocument();
  });

  it("pushes a Redis list item from the DBX-style selected value controls", async () => {
    let added = false;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "queue",
              key_raw: "queue",
              key_type: "list",
              ttl: -1,
              size: added ? 2 : 1,
              value_preview: added ? "second" : "first",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "queue",
          key_raw: "queue",
          key_type: "list",
          ttl: -1,
          value_is_binary: false,
          total: added ? 2 : 1,
          scan_cursor: 0,
          value: added ? ["first", "second"] : ["first"],
        });
      }
      if (command === "dbx_redis_list_push") {
        expect(args).toEqual({
          connectionId: "redis",
          db: 0,
          keyRaw: "queue",
          value: "second",
          ttl: null,
        });
        added = true;
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /queue list/i }));
    expect((await screen.findAllByText("first")).length).toBeGreaterThan(0);

    await userEvent.type(
      screen.getByRole("textbox", { name: /Redis new member value|Redis 新成员值/ }),
      "second",
    );
    await userEvent.click(screen.getByRole("button", { name: /Push|推入/ }));

    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_list_push"),
      ).toBe(true);
    });
    expect(await screen.findByText("second")).toBeInTheDocument();
  });

  it("shows DBX-style readonly handling and insert-statement feedback for binary Redis values", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "blob:1",
              key_raw: "blob:1",
              key_type: "string",
              ttl: -1,
              size: 3,
              value_preview: "<binary>",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "blob:1",
          key_raw: "blob:1",
          key_type: "string",
          ttl: -1,
          value_is_binary: true,
          value: "<binary>",
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^blob/i }));
    await userEvent.click(screen.getByRole("button", { name: /blob:1 string/i }));

    const valueEditor = await screen.findByRole("textbox", { name: /Redis value|Redis 值/ });
    expect(valueEditor).toHaveValue("<binary>");
    expect(valueEditor).toHaveAttribute("readonly");
    expect(
      screen.getByText(/Binary string values are shown read-only|二进制字符串值以只读方式显示/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Format$|^格式化$/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /^Compress$|^压缩$/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Save value|保存值/ })).toBeDisabled();

    const copyInsert = screen.getByRole("button", { name: /Copy insert statement|复制插入语句/ });
    expect(copyInsert).toBeEnabled();
    await userEvent.click(copyInsert);

    expect(
      await screen.findByText(
        /Cannot generate insert statement for binary data|无法为二进制数据生成插入语句/,
      ),
    ).toBeInTheDocument();
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === "dbx_redis_set_value"),
    ).toBe(false);
  });

  it("edits the selected Redis key TTL from the DBX-style TTL badge", async () => {
    let currentTtl = 60;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys") {
        return Promise.resolve({
          cursor: 0,
          total_keys: 1,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: currentTtl,
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
          ttl: currentTtl,
          value_is_binary: false,
          value: "Ada",
        });
      }
      if (command === "dbx_redis_set_ttl") {
        currentTtl = (args as { ttl: number }).ttl;
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <RedisBrowser connectionId="redis" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /^user/i }));
    await userEvent.click(screen.getByRole("button", { name: /user:1 string/i }));
    await userEvent.click(await screen.findByRole("button", { name: /TTL: 60s|TTL：60 秒/ }));
    const ttlInput = screen.getByLabelText(/Redis TTL seconds|Redis TTL 秒数/);
    await userEvent.clear(ttlInput);
    await userEvent.type(ttlInput, "120");
    await userEvent.click(screen.getByRole("button", { name: /Save TTL|保存 TTL/ }));

    expect(invoke).toHaveBeenCalledWith("dbx_redis_set_ttl", {
      connectionId: "redis",
      db: 0,
      keyRaw: "user:1",
      ttl: 120,
    });
    expect(
      await screen.findByRole("button", { name: /TTL: 120s|TTL：120 秒/ }),
    ).toBeInTheDocument();
  });
});
