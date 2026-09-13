/**
 * 通用「拖拽改宽」监听骨架:挂 window 的 pointermove/pointerup,拖拽期间把
 * body 的 cursor / userSelect 锁成 col-resize / none,清理时还原成进入前的值
 * (不是写死的空串)。宽度怎么算、写回哪个状态由调用方在 beginResize 时通过
 * onMove 闭包决定,起点(x / 起始宽度)也由调用方记下再闭包捕获。
 *
 * 从 RedisBrowser 抽出 —— 那里成员详情面板与 hash/zset 列各有一份只差
 * 宽度算式的同构 effect;DatabaseSidebarResize 是另一个只管自身宽度的
 * 特化版本,两侧共用时可以考虑合并,这里不强行归一。
 */
import { useCallback, useEffect, useRef, useState } from "react";

export function usePanelResize<K extends string | boolean>() {
  const [resizingKind, setResizingKind] = useState<K | null>(null);
  const moveRef = useRef<((event: PointerEvent) => void) | null>(null);

  useEffect(() => {
    if (resizingKind === null) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => moveRef.current?.(event);
    const handlePointerUp = () => setResizingKind(null);
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingKind]);

  const beginResize = useCallback((kind: K, onMove: (event: PointerEvent) => void) => {
    moveRef.current = onMove;
    setResizingKind(kind);
  }, []);

  /** 主动终止拖拽(指针松开之外的场景,如切换连接时的整场重置)。 */
  const endResize = useCallback(() => setResizingKind(null), []);

  return { resizingKind, beginResize, endResize } as const;
}
