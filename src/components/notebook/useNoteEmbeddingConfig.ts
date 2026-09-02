/**
 * 随手记 RAG 的 embedding 配置从哪来。
 *
 * 在这个 hook 出现之前 `NotebookPanel` 直接把 `DEFAULT_RAG_CONFIG` 递给 `useNoteRag`,
 * 于是设置页改了地址和模型也没人读 —— 面板永远连本机 Ollama。
 *
 * 不变量:
 *
 * 1. **读不出来就用默认值,不报错。** 这份配置只决定「AI 面板连哪个 embedding 服务」,
 *    而设置文件读不出来时整个应用都有更大的麻烦。把面板卡在一条错误上,换来的是用户
 *    连笔记都看不了。真的连不上 provider 时 `useNoteRag` 那边会报,而那条消息才是
 *    可执行的。
 *
 * 2. **`APP_SETTINGS_CHANGED_EVENT` 要重读。** 设置页保存后会派发它。不听的话用户改完
 *    provider 回到面板,搜索仍然打在旧地址上,直到重启 —— 而"改了设置没生效"是最难
 *    自查的一类问题。
 *
 * 3. **返回值按值 memo。** `useNoteRag` 把它塞进 ref、每次 render 覆盖一次,所以身份
 *    变化本身无害;但同一份配置每次 render 换一个对象会让任何将来把它放进依赖数组的
 *    人踩坑。这里按三个字段 memo,配置没变就是同一个对象。
 *
 * 4. **key 不在这里。** 明文不出后端 —— 后端在真要发请求前从钥匙串补
 *    (`notebook::rag::commands::resolve_key`)。
 */
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { APP_SETTINGS_CHANGED_EVENT, type AppSettings } from "../app-settings/types";
import { DEFAULT_RAG_CONFIG, type EmbedProvider, type RagEmbedConfig } from "./noteRag";

/** 后端存的形状(snake_case)→ 命令要的形状(camelCase)。 */
function toConfig(settings: AppSettings | null): RagEmbedConfig {
  const stored = settings?.notebook_embedding_settings;
  if (!stored) return DEFAULT_RAG_CONFIG;
  return {
    // 后端已经归一过(空值补默认),这里只在字段整个缺失时兜底 —— 比如一份手工编辑过
    // 的 settings.json。
    provider: (stored.provider as EmbedProvider) || DEFAULT_RAG_CONFIG.provider,
    baseUrl: stored.base_url || DEFAULT_RAG_CONFIG.baseUrl,
    model: stored.model || DEFAULT_RAG_CONFIG.model,
  };
}

export function useNoteEmbeddingConfig(): RagEmbedConfig {
  const [loaded, setLoaded] = useState<RagEmbedConfig>(DEFAULT_RAG_CONFIG);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      void invoke<AppSettings>("load_app_settings")
        .then((settings) => {
          if (!cancelled) setLoaded(toConfig(settings));
        })
        // 见不变量 1。
        .catch(() => {
          if (!cancelled) setLoaded(DEFAULT_RAG_CONFIG);
        });
    };
    load();
    // 见不变量 2。
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, load);
    return () => {
      cancelled = true;
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, load);
    };
  }, []);

  // 见不变量 3。
  return useMemo(
    () => ({ provider: loaded.provider, baseUrl: loaded.baseUrl, model: loaded.model }),
    [loaded.provider, loaded.baseUrl, loaded.model],
  );
}
