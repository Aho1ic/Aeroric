/**
 * 自定义主题的生命周期编排:列表、应用、导入、删除、启动时恢复、应急停用。
 *
 * 与 `customThemes.ts` 分开是因为那一层没有 React 依赖(纯命令包装 + DOM 注入 + 快捷键
 * 判定),可以无 hook 直接测。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  customThemeDir,
  deleteCustomTheme,
  disableCustomTheme,
  importCustomTheme,
  isCustomThemePanicKey,
  listCustomThemes,
  readCustomTheme,
  readStoredThemeId,
  setInjectedCss,
  writeStoredThemeId,
  type CustomTheme,
} from "../customThemes";

export interface UseCustomThemesResult {
  themes: CustomTheme[];
  /** 生效中的主题 id;null 表示只用内置主题。 */
  activeId: string | null;
  error: string | null;
  busy: boolean;
  apply: (id: string | null) => Promise<void>;
  importFrom: (sourcePath: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  openDir: () => Promise<string>;
}

function errorText(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}

export function useCustomThemes(): UseCustomThemesResult {
  const [themes, setThemes] = useState<CustomTheme[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // DOM injection happens before React commits the matching state update. Keep
  // the active id synchronous so an older render cannot miss a delete.
  const activeIdRef = useRef<string | null>(null);
  // A later apply/remove invalidates older async reads. Otherwise a slow read
  // can resurrect a theme after the user has removed or disabled it.
  const operationRef = useRef(0);
  // 卸载之后不再 setState。启动链是两段 await,中途卸载是可能的。
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      operationRef.current += 1;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const listed = await listCustomThemes();
      if (alive.current) setThemes(listed);
    } catch (err) {
      if (alive.current) setError(errorText(err));
    }
  }, []);

  const apply = useCallback(async (id: string | null) => {
    const operation = ++operationRef.current;
    if (id === null) {
      activeIdRef.current = null;
      setInjectedCss(null);
      writeStoredThemeId(null);
      if (alive.current) {
        setActiveId(null);
        setError(null);
      }
      return;
    }
    try {
      const css = await readCustomTheme(id);
      if (operation !== operationRef.current) return;
      activeIdRef.current = id;
      setInjectedCss(css);
      writeStoredThemeId(id);
      if (alive.current) {
        setActiveId(id);
        setError(null);
      }
    } catch (err) {
      // 读不回来(文件被手工删了)就把持久化一起清掉,否则每次启动都失败一次。
      if (operation !== operationRef.current) return;
      activeIdRef.current = null;
      setInjectedCss(null);
      writeStoredThemeId(null);
      if (alive.current) {
        setActiveId(null);
        setError(errorText(err));
      }
    }
  }, []);

  const importFrom = useCallback(
    async (sourcePath: string) => {
      setBusy(true);
      try {
        const imported = await importCustomTheme(sourcePath);
        await refresh();
        // 导入完直接应用 —— 用户点导入就是想看效果。
        await apply(imported.id);
      } catch (err) {
        if (alive.current) setError(errorText(err));
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [apply, refresh],
  );

  const remove = useCallback(
    async (id: string) => {
      setBusy(true);
      try {
        // 删的是生效中的那套,先撤掉注入 —— 否则界面上会留着一份已经不存在的样式。
        if (id === activeIdRef.current) await apply(null);
        await deleteCustomTheme(id);
        await refresh();
      } catch (err) {
        if (alive.current) setError(errorText(err));
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [apply, refresh],
  );

  const openDir = useCallback(() => customThemeDir(), []);

  // 启动:列一次,再把记住的那套应用上。
  useEffect(() => {
    void (async () => {
      await refresh();
      const stored = readStoredThemeId();
      if (stored) await apply(stored);
    })();
  }, [apply, refresh]);

  // 应急停用。装在 document 上且用捕获阶段 —— 界面被 CSS 藏起来时,焦点可能在任何地方,
  // 而这条路必须无论如何都能走通。
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isCustomThemePanicKey(event)) return;
      event.preventDefault();
      operationRef.current += 1;
      if (disableCustomTheme() && alive.current) setActiveId(null);
      activeIdRef.current = null;
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, []);

  return { themes, activeId, error, busy, apply, importFrom, remove, openDir };
}
