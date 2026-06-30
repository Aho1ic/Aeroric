import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef } from "react";

export const REMOTE_IDE_COMMAND_TIMEOUT_MS = 60_000;

export type SafeInvokeOptions = {
  timeoutMs?: number;
  timeoutMessage?: string;
};

export class InvokeTimeoutError extends Error {
  readonly command: string;
  readonly timeoutMs: number;

  constructor(command: string, timeoutMs: number, message?: string) {
    super(message ?? formatInvokeTimeoutMessage(command, timeoutMs));
    this.name = "InvokeTimeoutError";
    this.command = command;
    this.timeoutMs = timeoutMs;
  }
}

export function formatInvokeTimeoutMessage(command: string, timeoutMs: number): string {
  const seconds = Math.max(1, Math.ceil(timeoutMs / 1000));
  return `Remote command "${command}" timed out after ${seconds}s. The remote operation may still finish; check the SSH connection and refresh.`;
}

export function formatInvokeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export async function invokeWithTimeout<T>(
  promise: Promise<T>,
  command: string,
  options?: SafeInvokeOptions,
): Promise<T> {
  const timeoutMs = options?.timeoutMs;
  if (!timeoutMs || timeoutMs <= 0) return promise;

  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => {
          reject(new InvokeTimeoutError(command, timeoutMs, options.timeoutMessage));
        }, timeoutMs);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

export function remoteInvokeOptions(timeoutMs = REMOTE_IDE_COMMAND_TIMEOUT_MS): SafeInvokeOptions {
  return { timeoutMs };
}

/**
 * Wraps Tauri invoke calls to automatically ignore results after component unmounts.
 * Not a true cancellation (Tauri doesn't support that), but prevents setState on unmounted components.
 */
export function useCancellableInvoke() {
  const cancelledRef = useRef(false);

  useEffect(() => {
    cancelledRef.current = false;
    return () => {
      cancelledRef.current = true;
    };
  }, []);

  const safeInvoke = useCallback(
    async <T>(
      cmd: string,
      args?: Record<string, unknown>,
      options?: SafeInvokeOptions,
    ): Promise<T | null> => {
      const result = await invokeWithTimeout(invoke<T>(cmd, args), cmd, options);
      if (cancelledRef.current) return null;
      return result;
    },
    [],
  );

  const isCancelled = useCallback(() => cancelledRef.current, []);

  return { safeInvoke, isCancelled };
}
