import { useRef, useCallback, useEffect } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";

// ── Buffer constants ─────────────────────────────────────────────────────────

const MAX_BUFFER_SIZE = 10 * 1024 * 1024; // 10MB per task (in-memory limit)
const MAX_BUFFER_CHUNKS = 256; // compact when chunks array exceeds this
const DRAIN_FRAME_BUDGET = 128 * 1024; // 每帧最多处理 128KB，避免单帧写入时间过长

// ── Buffer types & helpers ───────────────────────────────────────────────────

interface TaskBuffer {
  chunks: string[];
  totalLen: number;
  droppedLen: number;
}

export type TerminalWriteFn = (data: string, callback?: () => void) => void;
export type TerminalResizeFn = (cols: number, rows: number) => void;

interface TerminalWriteState {
  pending: string[];
  ready: boolean;
  generation: number;
}

function createTaskBuffer(): TaskBuffer {
  return { chunks: [], totalLen: 0, droppedLen: 0 };
}

function createTerminalWriteState(generation = 0): TerminalWriteState {
  return { pending: [], ready: false, generation };
}

function pushToBuffer(buf: TaskBuffer, data: string): void {
  buf.chunks.push(data);
  buf.totalLen += data.length;
  while (buf.totalLen > MAX_BUFFER_SIZE && buf.chunks.length > 0) {
    const dropped = buf.chunks.shift()!;
    buf.totalLen -= dropped.length;
    buf.droppedLen += dropped.length;
  }
  if (buf.chunks.length > MAX_BUFFER_CHUNKS) {
    const merged = buf.chunks.join("");
    buf.chunks.length = 0;
    buf.chunks.push(merged);
  }
}

function getBufferAbsLen(buf: TaskBuffer): number {
  return buf.totalLen + buf.droppedLen;
}

function joinBufferFrom(buf: TaskBuffer, absOffset: number): string {
  const relOffset = absOffset - buf.droppedLen;
  if (relOffset <= 0) return buf.chunks.join("");
  let cum = 0;
  for (let i = 0; i < buf.chunks.length; i++) {
    const len = buf.chunks[i].length;
    if (cum + len > relOffset) {
      const parts = buf.chunks.slice(i);
      parts[0] = parts[0].slice(relOffset - cum);
      return parts.join("");
    }
    cum += len;
  }
  return "";
}

// ── Hook ─────────────────────────────────────────────────────────────────────

export function useTerminalManager() {
  const taskBufferRef = useRef<Record<string, TaskBuffer>>({});
  const terminalSnapshotRef = useRef<Record<string, { snapshot: string; bufferLength: number }>>(
    {},
  );
  const terminalWriteRefs = useRef<Record<string, TerminalWriteFn>>({});
  const terminalResizeRefs = useRef<Record<string, TerminalResizeFn>>({});
  const terminalWriteStateRef = useRef<Record<string, TerminalWriteState>>({});
  const terminalSizeRef = useRef<{ cols: number; rows: number }>({ cols: 220, rows: 50 });

  // ── Write state management ───────────────────────────────────────────────

  const resetTerminalWriteState = useCallback((taskId: string) => {
    const prev = terminalWriteStateRef.current[taskId];
    const next = createTerminalWriteState((prev?.generation ?? 0) + 1);
    terminalWriteStateRef.current[taskId] = next;
    return next;
  }, []);

  const enqueueTerminalWrite = useCallback(
    (taskId: string, data: string) => {
      const state = terminalWriteStateRef.current[taskId] ?? resetTerminalWriteState(taskId);
      if (!state.ready) {
        state.pending.push(data);
        return;
      }
      const writeFn = terminalWriteRefs.current[taskId];
      if (writeFn) {
        writeFn(data);
      }
    },
    [resetTerminalWriteState],
  );

  // ── Agent output ingestion ───────────────────────────────────────────────
  // 通过 tauri::ipc::Channel 直投单订阅者，绕过 emit/listen 的全局事件总线。
  // pendingOutputs / RAF 仍在 hook 级共享，保留原批量写入节奏与每帧字节预算。

  const pendingOutputsRef = useRef<Map<string, string[]>>(new Map());
  const rafIdRef = useRef<number>(0);

  const drainPendingOutputs = useCallback(() => {
    rafIdRef.current = 0;
    if (
      (
        navigator as unknown as {
          scheduling?: { isInputPending?: () => boolean };
        }
      ).scheduling?.isInputPending?.()
    ) {
      rafIdRef.current = requestAnimationFrame(drainPendingOutputs);
      return;
    }
    const pendingOutputs = pendingOutputsRef.current;
    let bytesThisFrame = 0;
    for (const [taskId, chunks] of pendingOutputs) {
      const joined = chunks.length === 1 ? chunks[0] : chunks.join("");

      if (terminalWriteRefs.current[taskId]) {
        enqueueTerminalWrite(taskId, joined);
      }
      if (taskId in taskBufferRef.current) {
        pushToBuffer(taskBufferRef.current[taskId], joined);
      }

      pendingOutputs.delete(taskId);
      bytesThisFrame += joined.length;
      if (bytesThisFrame >= DRAIN_FRAME_BUDGET) {
        break;
      }
    }
    if (pendingOutputs.size > 0 && !rafIdRef.current) {
      rafIdRef.current = requestAnimationFrame(drainPendingOutputs);
    }
  }, [enqueueTerminalWrite]);

  const ingestAgentChunk = useCallback(
    (taskId: string, data: string) => {
      const pendingOutputs = pendingOutputsRef.current;
      let arr = pendingOutputs.get(taskId);
      if (!arr) {
        arr = [];
        pendingOutputs.set(taskId, arr);
      }
      arr.push(data);
      if (!rafIdRef.current) {
        rafIdRef.current = requestAnimationFrame(drainPendingOutputs);
      }
    },
    [drainPendingOutputs],
  );

  const createOutputChannel = useCallback(
    (taskId: string): Channel<string> => {
      const channel = new Channel<string>();
      channel.onmessage = (data) => ingestAgentChunk(taskId, data);
      return channel;
    },
    [ingestAgentChunk],
  );

  useEffect(() => {
    return () => {
      if (rafIdRef.current) cancelAnimationFrame(rafIdRef.current);
    };
  }, []);

  // ── Public API ───────────────────────────────────────────────────────────

  const resetTaskTerminal = useCallback((taskId: string) => {
    taskBufferRef.current[taskId] = createTaskBuffer();
    delete terminalSnapshotRef.current[taskId];
  }, []);

  const removeTaskBuffers = useCallback((taskIds: string[]) => {
    for (const taskId of taskIds) {
      delete taskBufferRef.current[taskId];
      delete terminalSnapshotRef.current[taskId];
      delete terminalWriteRefs.current[taskId];
      delete terminalResizeRefs.current[taskId];
      delete terminalWriteStateRef.current[taskId];
    }
  }, []);

  const writeErrorToTerminal = useCallback((taskId: string, errMsg: string) => {
    const writeFn = terminalWriteRefs.current[taskId];
    if (writeFn) {
      writeFn(errMsg);
    }
    const buf = taskBufferRef.current[taskId] ?? createTaskBuffer();
    pushToBuffer(buf, errMsg);
    taskBufferRef.current[taskId] = buf;
  }, []);

  const handleInput = useCallback((taskId: string, data: string) => {
    invoke("send_input", { taskId, data }).catch(console.error);
  }, []);

  const handleResize = useCallback((taskId: string, cols: number, rows: number) => {
    terminalSizeRef.current = { cols, rows };
    invoke("resize_pty", { taskId, cols, rows }).catch(console.error);
  }, []);

  // 远程手机调整的是共享 PTY。桌面端只同步本地 xterm 的逻辑网格，不能再调用
  // handleResize 回写 PTY，否则会和手机端互相触发 resize，最终把 TUI 打散。
  const handleRemoteResize = useCallback((taskId: string, cols: number, rows: number) => {
    if (!Number.isInteger(cols) || !Number.isInteger(rows) || cols < 2 || rows < 2) return;
    terminalResizeRefs.current[taskId]?.(cols, rows);
  }, []);

  const handleRegisterTerminal = useCallback(
    (taskId: string, fn: TerminalWriteFn | null, resizeFn?: TerminalResizeFn): number => {
      const state = resetTerminalWriteState(taskId);
      if (fn) {
        terminalWriteRefs.current[taskId] = fn;
        if (resizeFn) {
          terminalResizeRefs.current[taskId] = resizeFn;
        } else {
          delete terminalResizeRefs.current[taskId];
        }
      } else {
        delete terminalWriteRefs.current[taskId];
        delete terminalResizeRefs.current[taskId];
      }
      return state.generation;
    },
    [resetTerminalWriteState],
  );

  const handleTerminalReady = useCallback((taskId: string, generation: number) => {
    const state = terminalWriteStateRef.current[taskId];
    if (!state || state.generation !== generation) return;
    state.ready = true;
    if (state.pending.length > 0) {
      const writeFn = terminalWriteRefs.current[taskId];
      if (writeFn) {
        const data = state.pending.length === 1 ? state.pending[0] : state.pending.join("");
        writeFn(data);
      }
      state.pending = [];
    }
  }, []);

  const handleSnapshot = useCallback((taskId: string, snapshot: string) => {
    const buf = taskBufferRef.current[taskId];
    const state = terminalWriteStateRef.current[taskId];
    const pendingLen = state?.pending.reduce((s, c) => s + c.length, 0) ?? 0;
    terminalSnapshotRef.current[taskId] = {
      snapshot,
      bufferLength: buf ? Math.max(0, getBufferAbsLen(buf) - pendingLen) : 0,
    };
  }, []);

  const getTaskRestoreState = useCallback((taskId: string) => {
    const buf = taskBufferRef.current[taskId];
    const snapshotState = terminalSnapshotRef.current[taskId];

    if (!buf) return { initialData: "", rawReplayData: "" };

    const rawReplayData = buf.chunks.join("");

    if (!snapshotState?.snapshot) {
      return { initialData: rawReplayData, rawReplayData };
    }

    const absLen = getBufferAbsLen(buf);
    if (snapshotState.bufferLength < 0 || snapshotState.bufferLength > absLen) {
      return { initialData: rawReplayData, rawReplayData };
    }

    return {
      initialSnapshot: snapshotState.snapshot,
      initialData: joinBufferFrom(buf, snapshotState.bufferLength),
      rawReplayData,
    };
  }, []);

  return {
    terminalSizeRef,
    resetTaskTerminal,
    removeTaskBuffers,
    writeErrorToTerminal,
    handleInput,
    handleResize,
    handleRemoteResize,
    handleRegisterTerminal,
    handleTerminalReady,
    handleSnapshot,
    getTaskRestoreState,
    createOutputChannel,
  };
}
