/* 笔记列表的拖动排序。
 *
 * 从 NotebookPanel 抽出来,逻辑逐字未改。
 *
 * 走 Pointer Events 而不是 HTML5 drag-and-drop:后者在 Tauri 的 WebView 里
 * 会被窗口拖动抢掉(macOS 上尤其明显),而且拿不到跟手的位置反馈。
 *
 * 判定落点靠「命中测试列表项的 DOM 矩形」,不靠事件冒泡 —— 指针被 capture
 * 之后所有事件都打在按下的那个元素上,`event.target` 永远是源项。
 */

import { useRef, useState } from "react";
import type React from "react";

/** 抖动容差。低于这个位移算点击,不算拖动 —— 否则轻点会触发一次无意义的重排。 */
const POINTER_DRAG_MOVE_TOLERANCE = 5;

type NoteDragState = {
  id: string;
  pointerId: number;
  startY: number;
  hasMoved: boolean;
};

export type NoteDragReorder = {
  /** 正在拖的项 / 当前悬停的项。列表用它们画高亮。 */
  draggedNoteId: string | null;
  dragOverNoteId: string | null;
  /** 列表项挂 ref 用,命中测试要读它们的矩形。 */
  setNoteItemRef: (noteId: string) => (element: HTMLDivElement | null) => void;
  /** 拖动结束后要吞掉紧跟的那次 click,否则松手会顺带切换笔记。 */
  suppressNextClickRef: React.RefObject<boolean>;
  onPointerDown: (event: React.PointerEvent<HTMLButtonElement>, noteId: string) => void;
  onPointerMove: (event: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerCancel: (event: React.PointerEvent<HTMLButtonElement>) => void;
};

export function useNoteDragReorder(
  reorderNote: (draggedId: string, targetId: string) => void,
): NoteDragReorder {
  const noteItemRefs = useRef<Map<string, HTMLElement>>(new Map());
  const notePointerDragRef = useRef<NoteDragState | null>(null);
  const suppressNextClickRef = useRef(false);
  const [draggedNoteId, setDraggedNoteId] = useState<string | null>(null);
  const [dragOverNoteId, setDragOverNoteId] = useState<string | null>(null);

  const setNoteItemRef = (noteId: string) => (element: HTMLDivElement | null) => {
    if (element) {
      noteItemRefs.current.set(noteId, element);
    } else {
      noteItemRefs.current.delete(noteId);
    }
  };

  const noteIdAtClientY = (clientY: number) => {
    let fallback: string | null = null;
    let fallbackDistance = Number.POSITIVE_INFINITY;
    for (const [noteId, element] of noteItemRefs.current) {
      const rect = element.getBoundingClientRect();
      if (clientY >= rect.top && clientY <= rect.bottom) return noteId;
      const center = rect.top + rect.height / 2;
      const distance = Math.abs(clientY - center);
      if (distance < fallbackDistance) {
        fallback = noteId;
        fallbackDistance = distance;
      }
    }
    return fallback;
  };

  const resetNotePointerDrag = () => {
    notePointerDragRef.current = null;
    setDraggedNoteId(null);
    setDragOverNoteId(null);
  };

  const onPointerDown = (event: React.PointerEvent<HTMLButtonElement>, noteId: string) => {
    if (event.button !== 0) return;
    const currentTarget = event.currentTarget;
    notePointerDragRef.current = {
      id: noteId,
      pointerId: event.pointerId,
      startY: event.clientY,
      hasMoved: false,
    };
    setDraggedNoteId(noteId);
    setDragOverNoteId(noteId);
    currentTarget.setPointerCapture?.(event.pointerId);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = notePointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    if (Math.abs(event.clientY - drag.startY) > POINTER_DRAG_MOVE_TOLERANCE) {
      drag.hasMoved = true;
    }
    setDragOverNoteId(noteIdAtClientY(event.clientY));
  };

  const onPointerUp = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = notePointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const targetId = drag.hasMoved ? noteIdAtClientY(event.clientY) : null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    resetNotePointerDrag();
    if (!targetId) return;
    suppressNextClickRef.current = true;
    event.preventDefault();
    reorderNote(drag.id, targetId);
  };

  const onPointerCancel = (event: React.PointerEvent<HTMLButtonElement>) => {
    const drag = notePointerDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    resetNotePointerDrag();
  };

  return {
    draggedNoteId,
    dragOverNoteId,
    setNoteItemRef,
    suppressNextClickRef,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
  };
}
