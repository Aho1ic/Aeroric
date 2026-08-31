/* 语义检索面板的状态与动作。
 *
 * 抽成 hook 而不是写进 NotebookPanel:那个文件已经 3800 行,而这里有一件必须一处
 * 做对的事 —— **进度监听要在建索引之前就挂上**。`notebook_rag_index` 会一直 await
 * 到这一轮跑完,进度全靠事件;等 await 返回再挂监听的话,整轮的进度一条都收不到,
 * 表现是"点了建索引之后界面一直没反应,过很久突然就好了"。
 *
 * 另一件是**进度只认自己那个 vault**。事件按 vault 平铺(多个 vault 可以同时在建),
 * 不过滤的话另一个库的进度会画到这个面板上。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  cancelRagIndex,
  clearRagIndex,
  ragContext,
  ragIndexStats,
  ragSearch,
  runRagIndex,
  type RagContextBundle,
  type RagDegraded,
  type RagEmbedConfig,
  type RagHit,
  type RagIndexProgress,
  type RagIndexStats,
} from "./noteRag";

/** 进度事件的 topic。与 Rust 侧 `PROGRESS_EVENT` 一致。 */
const PROGRESS_EVENT = "notebook-rag-index-progress";

export type NoteRagState = {
  stats: RagIndexStats | null;
  progress: RagIndexProgress | null;
  query: string;
  hits: RagHit[];
  searched: boolean;
  searching: boolean;
  degraded: RagDegraded[];
  vectorsMissing: boolean;
  context: RagContextBundle | null;
  contextBusy: boolean;
  /** 上下文刚被复制走。一句话的反馈,留在面板里 —— 和导出面板一致。 */
  copied: boolean;
  error: string | null;
};

export type NoteRagApi = NoteRagState & {
  setQuery: (value: string) => void;
  search: () => void;
  buildContext: (current: { path: string; body: string } | null) => void;
  copyContext: () => void;
  index: (scope: "all" | "failedOnly") => void;
  cancel: () => void;
  clear: () => void;
  refreshStats: () => void;
};

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * `enabled` 转 true 时读一次索引状态。关着的时候不读 —— 那是一次开库读表,而绝大
 * 多数时候面板没打开。
 */
export function useNoteRag(
  vault: string | null,
  enabled: boolean,
  config: RagEmbedConfig,
): NoteRagApi {
  const [stats, setStats] = useState<RagIndexStats | null>(null);
  const [progress, setProgress] = useState<RagIndexProgress | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<RagHit[]>([]);
  const [searched, setSearched] = useState(false);
  const [searching, setSearching] = useState(false);
  const [degraded, setDegraded] = useState<RagDegraded[]>([]);
  const [vectorsMissing, setVectorsMissing] = useState(false);
  const [context, setContext] = useState<RagContextBundle | null>(null);
  const [contextBusy, setContextBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statsToken, setStatsToken] = useState(0);

  /* 配置进 ref 而不是进各个 callback 的依赖:它是个每次渲染都新建的对象字面量,
     进依赖会让所有动作函数每次渲染都换新的,而其中一个挂在 effect 上。 */
  const configRef = useRef(config);
  configRef.current = config;

  /* 进度监听常驻(只要面板开着),不跟着"正在建索引"开关 —— 挂监听是异步的,
     等点了按钮再挂会漏掉最前面几条,而 `scanning` 那一段恰好是最长的沉默期。 */
  useEffect(() => {
    if (!vault || !enabled) return;
    const pending = listen<RagIndexProgress>(PROGRESS_EVENT, (event) => {
      // 别的 vault 也可能在建。不过滤的话它的进度会画到这个面板上。
      if (event.payload.vault !== vault) return;
      setProgress(
        event.payload.phase === "done" ||
          event.payload.phase === "cancelled" ||
          event.payload.phase === "failed"
          ? null
          : event.payload,
      );
      /* 整轮失败的原因只在事件里 —— `runRagIndex` 那一路对"跑完了但整轮失败"是
         正常返回的。不接住这里就没有任何地方会显示它。 */
      if (event.payload.error) setError(event.payload.error);
    });
    /* 无条件退订。`listen` 返回的是 promise,卸载时它可能还没落地 —— 挂在 `.then`
       上而不是"落地了才退订",否则快开快关会留下一个收不掉的监听。 */
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [enabled, vault]);

  useEffect(() => {
    if (!vault || !enabled) return;
    let cancelled = false;
    void (async () => {
      try {
        const next = await ragIndexStats(vault);
        if (!cancelled) setStats(next);
      } catch (failure: unknown) {
        if (!cancelled) setError(messageOf(failure));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [enabled, statsToken, vault]);

  const refreshStats = useCallback(() => setStatsToken((current) => current + 1), []);

  const search = useCallback(() => {
    if (!vault) return;
    const text = query.trim();
    if (text.length === 0) return;
    setSearching(true);
    setError(null);
    void (async () => {
      try {
        const outcome = await ragSearch(vault, text, configRef.current);
        setHits(outcome.hits);
        setDegraded(outcome.degraded);
        setVectorsMissing(outcome.vectorsMissing);
        setSearched(true);
      } catch (failure: unknown) {
        setError(messageOf(failure));
      } finally {
        setSearching(false);
      }
    })();
  }, [query, vault]);

  const buildContext = useCallback(
    (current: { path: string; body: string } | null) => {
      if (!vault) return;
      const text = query.trim();
      if (text.length === 0) return;
      setContextBusy(true);
      setError(null);
      void (async () => {
        try {
          const bundle = await ragContext({
            vault,
            query: text,
            config: configRef.current,
            currentPath: current?.path ?? null,
            currentBody: current?.body ?? null,
          });
          setContext(bundle);
          setDegraded(bundle.degraded);
          setVectorsMissing(bundle.vectorsMissing);
        } catch (failure: unknown) {
          setError(messageOf(failure));
        } finally {
          setContextBusy(false);
        }
      })();
    },
    [query, vault],
  );

  /* 写剪贴板用 `navigator.clipboard.writeText`,和面板别处一致(读才走 Tauri 插件 ——
     `readText` 在 WebView 里常因权限被拒)。`?.` 是因为非安全上下文里它不存在。 */
  const copyContext = useCallback(() => {
    const text = context?.text;
    if (!text) return;
    void (async () => {
      try {
        await navigator.clipboard?.writeText(text);
        setCopied(true);
      } catch (failure: unknown) {
        // 复制失败必须说 —— 静默的话用户会去粘贴一份旧内容。
        setError(messageOf(failure));
      }
    })();
  }, [context?.text]);

  /* 装配出新的一段之后把"已复制"收掉:那条提示讲的是上一段文本,留着会让用户以为
     新的这段也已经在剪贴板里了。 */
  useEffect(() => {
    setCopied(false);
  }, [context?.text]);

  const index = useCallback(
    (scope: "all" | "failedOnly") => {
      if (!vault) return;
      setError(null);
      /* 先手动置一个 scanning 进度。第一条事件到之前(要开库、走完 vault)界面
         否则完全没有反应,而那一段恰好是最久的。 */
      setProgress({
        vault,
        phase: "scanning",
        total: 0,
        done: 0,
        failed: 0,
        current: null,
        error: null,
      });
      void (async () => {
        try {
          await runRagIndex(vault, configRef.current, scope);
        } catch (failure: unknown) {
          setError(messageOf(failure));
        } finally {
          /* 无论成败都清进度并重读状态:失败时事件可能一条都没来过(库就打不开),
             那时进度会永远卡在 scanning 上。 */
          setProgress(null);
          refreshStats();
        }
      })();
    },
    [refreshStats, vault],
  );

  const cancel = useCallback(() => {
    if (!vault) return;
    void cancelRagIndex(vault).catch((failure: unknown) => setError(messageOf(failure)));
  }, [vault]);

  const clear = useCallback(() => {
    if (!vault) return;
    setError(null);
    void (async () => {
      try {
        await clearRagIndex(vault);
        /* 命中和上下文也要清:它们讲的是刚被删掉的那个索引,留着点进去会跳到
           一份可能已经变了的正文上。 */
        setHits([]);
        setContext(null);
        setSearched(false);
        setVectorsMissing(true);
      } catch (failure: unknown) {
        setError(messageOf(failure));
      } finally {
        refreshStats();
      }
    })();
  }, [refreshStats, vault]);

  return {
    stats,
    progress,
    query,
    hits,
    searched,
    searching,
    degraded,
    vectorsMissing,
    context,
    contextBusy,
    copied,
    error,
    setQuery,
    search,
    buildContext,
    copyContext,
    index,
    cancel,
    clear,
    refreshStats,
  };
}
