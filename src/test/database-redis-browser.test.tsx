import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { RedisBrowser } from "../components/database/RedisBrowser";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("RedisBrowser", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
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
    expect(await screen.findByText("user:1")).toBeInTheDocument();
    await userEvent.click(screen.getByText("user:1"));
    expect(await screen.findByText("Ada")).toBeInTheDocument();
  });

  it("creates a Redis key and runs a Redis command", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 0 }]);
      if (command === "dbx_redis_scan_keys") return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
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
});
