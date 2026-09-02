/* `useNoteEmbeddingConfig` 从哪读、什么时候重读。
 *
 * 三件"错了不报错、只是行为不对"的事:
 *   1. 设置文件读不出来时得落回默认值 —— 把 AI 面板卡在一条错误上,换来的是用户连笔记
 *      都看不了,而真连不上 provider 时 `useNoteRag` 那边会报,那条消息才可执行。
 *   2. 保存设置后要重读 —— 不听 `APP_SETTINGS_CHANGED_EVENT` 的话,用户改完 provider
 *      回到面板,检索仍然打在旧地址上,直到重启。
 *   3. 配置没变就返回同一个对象 —— 将来谁把它放进依赖数组都不会因此每帧重跑。
 */

import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

import { APP_SETTINGS_CHANGED_EVENT } from "../components/app-settings/types";
import { DEFAULT_RAG_CONFIG } from "../components/notebook/noteRag";
import { useNoteEmbeddingConfig } from "../components/notebook/useNoteEmbeddingConfig";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));

const openAi = {
  provider: "openAi",
  base_url: "https://api.openai.com/v1",
  model: "text-embedding-3-small",
};

beforeEach(() => {
  invokeMock.mockReset();
});

describe("useNoteEmbeddingConfig", () => {
  it("读设置页存下的那一份", async () => {
    invokeMock.mockResolvedValue({ notebook_embedding_settings: openAi });
    const { result } = renderHook(() => useNoteEmbeddingConfig());

    await waitFor(() => expect(result.current.provider).toBe("openAi"));
    expect(result.current).toEqual({
      provider: "openAi",
      baseUrl: "https://api.openai.com/v1",
      model: "text-embedding-3-small",
    });
    expect(invokeMock).toHaveBeenCalledWith("load_app_settings");
  });

  it("设置读不出来时落回默认值,不往上抛", async () => {
    invokeMock.mockRejectedValue(new Error("settings.json 读不了"));
    const { result } = renderHook(() => useNoteEmbeddingConfig());

    await waitFor(() => expect(invokeMock).toHaveBeenCalled());
    expect(result.current).toEqual(DEFAULT_RAG_CONFIG);
  });

  it("老配置里没有这一段时用本机 Ollama", async () => {
    invokeMock.mockResolvedValue({});
    const { result } = renderHook(() => useNoteEmbeddingConfig());

    await waitFor(() => expect(invokeMock).toHaveBeenCalled());
    expect(result.current).toEqual(DEFAULT_RAG_CONFIG);
  });

  it("字段被手工清空时逐个补默认值", async () => {
    invokeMock.mockResolvedValue({
      notebook_embedding_settings: { provider: "", base_url: "", model: "" },
    });
    const { result } = renderHook(() => useNoteEmbeddingConfig());

    await waitFor(() => expect(invokeMock).toHaveBeenCalled());
    expect(result.current).toEqual(DEFAULT_RAG_CONFIG);
  });

  it("设置页保存后重读", async () => {
    invokeMock.mockResolvedValue({ notebook_embedding_settings: DEFAULT_RAG_CONFIG });
    const { result } = renderHook(() => useNoteEmbeddingConfig());
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    invokeMock.mockResolvedValue({ notebook_embedding_settings: openAi });
    act(() => {
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    });

    await waitFor(() => expect(result.current.baseUrl).toBe("https://api.openai.com/v1"));
    expect(result.current.model).toBe("text-embedding-3-small");
  });

  it("配置没变时返回同一个对象", async () => {
    invokeMock.mockResolvedValue({ notebook_embedding_settings: openAi });
    const { result, rerender } = renderHook(() => useNoteEmbeddingConfig());
    await waitFor(() => expect(result.current.provider).toBe("openAi"));

    const first = result.current;
    rerender();
    expect(result.current).toBe(first);
  });
});
