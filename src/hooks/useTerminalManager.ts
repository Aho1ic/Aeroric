import { useRef, useCallback, useEffect } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import {
  createTerminalRingBuffer,
  joinTerminalBuffer,
  joinTerminalBufferFrom,
  pushTerminalChunk,
  terminalBufferAbsLength,
  type TerminalRingBuffer,
} from "../terminalRingBuffer";

// ── Buffer constants ─────────────────────────────────────────────────────────

const DRAIN_FRAME_BUDGET = 128 * 1024; // 每帧最多处理 128KB，避免单帧写入时间过长

// ── Buffer types & helpers ───────────────────────────────────────────────────

export type TerminalWriteFn = (data: string, callback?: () => void) => void;
export type TerminalResizeFn = (cols: number, rows: number) => void;

interface TerminalWriteState {
  pending: string[];
  ready: boolean;
  generation: number;
}

function createTerminalWriteState(generation = 0): TerminalWriteState {
  return { pending: [], ready: false, generation };
}

// ── Hook ─────────────────────────────────────────────────────────────────────

export function useTerminalManager() {
  const taskBufferRef = useRef<Record<string, TerminalRingBuffer>>({});
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
  const stoppedTaskOutputsRef = useRef<Set<string>>(new Set());
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
        pushTerminalChunk(taskBufferRef.current[taskId], joined);
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
      if (stoppedTaskOutputsRef.current.has(taskId)) return;
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
      stoppedTaskOutputsRef.current.delete(taskId);
      const channel = new Channel<string>();
      channel.onmessage = (data) => ingestAgentChunk(taskId, data);
      return channel;
    },
    [ingestAgentChunk],
  );

  const stopTaskOutput = useCallback((taskId: string) => {
    stoppedTaskOutputsRef.current.add(taskId);
    pendingOutputsRef.current.delete(taskId);
  }, []);

  const resumeTaskOutput = useCallback((taskId: string) => {
    stoppedTaskOutputsRef.current.delete(taskId);
  }, []);

  useEffect(() => {
    return () => {
      if (rafIdRef.current) cancelAnimationFrame(rafIdRef.current);
    };
  }, []);

  // ── Public API ───────────────────────────────────────────────────────────

  const resetTaskTerminal = useCallback((taskId: string) => {
    taskBufferRef.current[taskId] = createTerminalRingBuffer();
    delete terminalSnapshotRef.current[taskId];
  }, []);

  const removeTaskBuffers = useCallback((taskIds: string[]) => {
    for (const taskId of taskIds) {
      delete taskBufferRef.current[taskId];
      delete terminalSnapshotRef.current[taskId];
      delete terminalWriteRefs.current[taskId];
      delete terminalResizeRefs.current[taskId];
      delete terminalWriteStateRef.current[taskId];
      stoppedTaskOutputsRef.current.delete(taskId);
      pendingOutputsRef.current.delete(taskId);
    }
  }, []);

  const writeErrorToTerminal = useCallback((taskId: string, errMsg: string) => {
    const writeFn = terminalWriteRefs.current[taskId];
    if (writeFn) {
      writeFn(errMsg);
    }
    const buf = taskBufferRef.current[taskId] ?? createTerminalRingBuffer();
    pushTerminalChunk(buf, errMsg);
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
      bufferLength: buf ? Math.max(0, terminalBufferAbsLength(buf) - pendingLen) : 0,
    };
  }, []);

  const getTaskRestoreState = useCallback((taskId: string) => {
    const buf = taskBufferRef.current[taskId];
    const snapshotState = terminalSnapshotRef.current[taskId];

    if (!buf) return { initialData: "", rawReplayData: "" };

    const rawReplayData = joinTerminalBuffer(buf);

    if (!snapshotState?.snapshot) {
      return { initialData: rawReplayData, rawReplayData };
    }

    const absLen = terminalBufferAbsLength(buf);
    if (snapshotState.bufferLength < 0 || snapshotState.bufferLength > absLen) {
      return { initialData: rawReplayData, rawReplayData };
    }

    return {
      initialSnapshot: snapshotState.snapshot,
      initialData: joinTerminalBufferFrom(buf, snapshotState.bufferLength),
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
    stopTaskOutput,
    resumeTaskOutput,
  };
}
