/**
 * 侧边栏的宽度与拖拽改宽:一份宽度、一个「正在拖」标志,加上按下拖拽条时记起点的那支回调。
 *
 * 从 `DatabaseView.tsx` 抽出。这一层是整个文件里最独立的一块 —— 它自己拥有全部状态
 * (宽度、是否正在拖、拖拽起点 ref),对外只暴露三样:宽度、是否正在拖、以及 pointerdown 回调,
 * 所以不需要任何 `deps`。
 *
 * 与原文逐字一致的地方有三处不能动:
 * - 只有 `resizingDatabaseSidebar` 为真时才挂 window 上那两个监听,并在清理里把
 *   `document.body` 的 cursor / userSelect 还原成进来时的值(不是写死的空串)。
 * - 新宽度先夹在 `[DATABASE_SIDEBAR_MIN_WIDTH, DATABASE_SIDEBAR_MAX_WIDTH]` 之间再 `Math.round`。
 * - `setDatabaseSidebarWidth` 里那句 `current === nextWidth ? current : nextWidth` 是拖拽时的
 *   一层短路,省掉同宽度的重渲染,不能简化成直接赋值。
 */

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";

import {
  DATABASE_SIDEBAR_DEFAULT_WIDTH,
  DATABASE_SIDEBAR_MAX_WIDTH,
  DATABASE_SIDEBAR_MIN_WIDTH,
} from "./databaseViewModel";

export interface DatabaseSidebarResize {
  databaseSidebarWidth: number;
  resizingDatabaseSidebar: boolean;
  startDatabaseSidebarResize: (event: ReactPointerEvent<HTMLButtonElement>) => void;
}

export function useDatabaseSidebarResize(): DatabaseSidebarResize {
  const databaseSidebarResizeStartRef = useRef({ x: 0, width: DATABASE_SIDEBAR_DEFAULT_WIDTH });
  const [databaseSidebarWidth, setDatabaseSidebarWidth] = useState(DATABASE_SIDEBAR_DEFAULT_WIDTH);
  const [resizingDatabaseSidebar, setResizingDatabaseSidebar] = useState(false);

  useEffect(() => {
    if (!resizingDatabaseSidebar) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => {
      const { width, x } = databaseSidebarResizeStartRef.current;
      const nextWidth = Math.min(
        DATABASE_SIDEBAR_MAX_WIDTH,
        Math.max(DATABASE_SIDEBAR_MIN_WIDTH, Math.round(width + event.clientX - x)),
      );
      setDatabaseSidebarWidth((current) => (current === nextWidth ? current : nextWidth));
    };
    const handlePointerUp = () => {
      setResizingDatabaseSidebar(false);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingDatabaseSidebar]);

  const startDatabaseSidebarResize = useCallback(
    (event: ReactPointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      databaseSidebarResizeStartRef.current = {
        x: event.clientX,
        width: databaseSidebarWidth,
      };
      setResizingDatabaseSidebar(true);
    },
    [databaseSidebarWidth],
  );

  return { databaseSidebarWidth, resizingDatabaseSidebar, startDatabaseSidebarResize };
}
