/**
 * `NotebookEmbeddingPanel` 的行为。
 *
 * 重点全在 key 那一格上 —— 它是这个面板里唯一一个「后端不回明文」的字段,于是三条规则都得
 * 钉住:输入框从空的开始、留空保存不动它、「测试连接」送框里那个还没保存的 key。前两条错了
 * 会静默弄丢用户的 key,第三条错了会让用户对着一个刚粘进来的正确 key 看到 401。
 */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import { I18nProvider } from "../i18n";
import { NotebookEmbeddingPanel } from "../components/app-settings/NotebookEmbeddingPanel";
import {
  APP_SETTINGS_CHANGED_EVENT,
  type NotebookEmbeddingSettings,
} from "../components/app-settings/types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const OLLAMA: NotebookEmbeddingSettings = {
  provider: "ollama",
  base_url: "http://127.0.0.1:11434",
  model: "nomic-embed-text",
};

type BackendOptions = {
  stored?: NotebookEmbeddingSettings;
  keyStored?: boolean;
  probe?: number | Error;
};

function savedArg(args: unknown): NotebookEmbeddingSettings {
  return (args as { notebookEmbeddingSettings: NotebookEmbeddingSettings })
    .notebookEmbeddingSettings;
}

function installBackend(options: BackendOptions = {}) {
  const stored = options.stored ?? OLLAMA;
  vi.mocked(invoke).mockImplementation((command, args) => {
    switch (command) {
      case "load_app_settings":
        return Promise.resolve({ notebook_embedding_settings: structuredClone(stored) });
      case "notebook_embedding_key_status":
        return Promise.resolve(options.keyStored ?? false);
      case "update_notebook_embedding_settings":
        // 真后端回的是归一后的整份设置,面板拿它重置 original(于是保存后不再 dirty)。
        return Promise.resolve({ notebook_embedding_settings: savedArg(args) });
      case "notebook_embedding_key_set":
      case "notebook_embedding_key_clear":
        return Promise.resolve();
      case "notebook_rag_probe":
        return options.probe instanceof Error
          ? Promise.reject(options.probe)
          : Promise.resolve(options.probe ?? 768);
      default:
        return Promise.reject(new Error(`unexpected command: ${String(command)}`));
    }
  });
}

function renderPanel() {
  return render(
    <I18nProvider>
      <NotebookEmbeddingPanel />
    </I18nProvider>,
  );
}

async function waitForLoaded(baseUrl = "http://127.0.0.1:11434") {
  await waitFor(() => {
    expect(screen.getByLabelText("Endpoint")).toHaveValue(baseUrl);
  });
}

describe("NotebookEmbeddingPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("loads the stored provider config", async () => {
    installBackend({
      stored: { provider: "openAi", base_url: "https://proxy.example/v1", model: "bge-m3" },
      keyStored: true,
    });
    renderPanel();
    await waitForLoaded("https://proxy.example/v1");

    expect(screen.getByLabelText("Provider")).toHaveValue("openAi");
    expect(screen.getByLabelText("Model")).toHaveValue("bge-m3");
    // 后端不回明文,所以「已经存过」只体现在 placeholder 和那个清除按钮上。
    expect(screen.getByLabelText("API Key")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
  });

  it("hides the clear button when no key is stored", async () => {
    installBackend();
    renderPanel();
    await waitForLoaded();

    expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("carries the endpoint and model over when the provider changes", async () => {
    installBackend();
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.selectOptions(screen.getByLabelText("Provider"), "openAi");

    expect(screen.getByLabelText("Endpoint")).toHaveValue("https://api.openai.com/v1");
    expect(screen.getByLabelText("Model")).toHaveValue("text-embedding-3-small");
  });

  it("keeps a hand-written endpoint when the provider changes", async () => {
    installBackend({ stored: { ...OLLAMA, base_url: "http://box.lan:11434" } });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded("http://box.lan:11434");

    await user.selectOptions(screen.getByLabelText("Provider"), "openAi");

    expect(screen.getByLabelText("Endpoint")).toHaveValue("http://box.lan:11434");
    // 模型那一格还停在上一个预设上,它照样跟着换。
    expect(screen.getByLabelText("Model")).toHaveValue("text-embedding-3-small");
  });

  it("saves the settings and announces the change", async () => {
    installBackend();
    const user = userEvent.setup();
    const changed = vi.fn();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, changed);
    try {
      renderPanel();
      await waitForLoaded();

      await user.clear(screen.getByLabelText("Model"));
      await user.type(screen.getByLabelText("Model"), "mxbai-embed-large");
      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("update_notebook_embedding_settings", {
          notebookEmbeddingSettings: { ...OLLAMA, model: "mxbai-embed-large" },
        });
      });
      expect(changed).toHaveBeenCalledTimes(1);
      // 框里空着就不许碰钥匙串 —— 只想改模型名的用户不该顺手把 key 抹掉。
      expect(invoke).not.toHaveBeenCalledWith("notebook_embedding_key_set", expect.anything());
    } finally {
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, changed);
    }
  });

  it("writes the key only when the box has something in it", async () => {
    installBackend();
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.type(screen.getByLabelText("API Key"), "sk-typed");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("notebook_embedding_key_set", { key: "sk-typed" });
    });
    // 写进去之后框子清空,清除按钮出现。
    expect(screen.getByLabelText("API Key")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
  });

  it("clears the stored key", async () => {
    installBackend({ keyStored: true });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Remove" }));

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
    });
    expect(invoke).toHaveBeenCalledWith("notebook_embedding_key_clear");
  });

  it("sends the just-typed key with the connection test", async () => {
    installBackend({ probe: 1536 });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.selectOptions(screen.getByLabelText("Provider"), "openAi");
    await user.type(screen.getByLabelText("API Key"), "sk-unsaved");
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    expect(await screen.findByText("Connected. Vector size: 1536.")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("notebook_rag_probe", {
      config: {
        provider: "openAi",
        baseUrl: "https://api.openai.com/v1",
        model: "text-embedding-3-small",
        apiKey: "sk-unsaved",
      },
    });
  });

  it("reports a failed probe without touching the form", async () => {
    installBackend({ probe: new Error("connection refused") });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test connection" }));

    expect(
      await screen.findByText("Connection failed: Error: connection refused"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Endpoint")).toHaveValue("http://127.0.0.1:11434");
  });

  it("drops a stale test result when the config changes", async () => {
    installBackend({ probe: 768 });
    const user = userEvent.setup();
    renderPanel();
    await waitForLoaded();

    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("Connected. Vector size: 768.")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Model"), "-v2");
    expect(screen.queryByText("Connected. Vector size: 768.")).not.toBeInTheDocument();
  });
});
