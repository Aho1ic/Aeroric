import { flushAllProjectTasks, flushProjects } from "./appProjectState";

export const TASK_FLUSH_TIMEOUT_MS = 10_000;

export function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message = `Timed out after ${timeoutMs}ms`,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

export function createTaskFlushCoordinator(flushAll: () => Promise<void>) {
  let inFlightOperation: Promise<void> | null = null;

  return (timeoutMs = TASK_FLUSH_TIMEOUT_MS): Promise<void> => {
    if (!inFlightOperation) {
      let operation: Promise<void>;
      try {
        // Start the shared save immediately so a second lifecycle request in
        // the same turn observes the same in-flight operation.
        operation = Promise.resolve(flushAll());
      } catch (error) {
        operation = Promise.reject(error);
      }
      inFlightOperation = operation;
      // A timeout only limits this caller's wait. Keep the underlying
      // operation tracked until it settles so a retry cannot overlap it.
      void operation.then(
        () => {
          if (inFlightOperation === operation) inFlightOperation = null;
        },
        () => {
          if (inFlightOperation === operation) inFlightOperation = null;
        },
      );
    }

    return withTimeout(
      inFlightOperation,
      timeoutMs,
      "Timed out while saving your work. The app was kept open.",
    );
  };
}

/**
 * 退出/重启前把两条保存队列都落盘。
 *
 * 任务和项目走的是两条独立队列(`taskPersistence` 防抖串行、`projectPersistence` 链式串行)。
 * 只等任务那条的话,项目侧排着的快照会随进程一起消失 —— 表现是「刚改的项目名/顺序退出后没了」。
 *
 * 并行等而不是串行:两者互不依赖,串行会把退出前的等待时间翻倍。任一失败则整体 reject,由调用方
 * 保持应用存活。
 */
export const flushPendingSavesBeforeExit = createTaskFlushCoordinator(async () => {
  await Promise.all([flushAllProjectTasks(), flushProjects()]);
});
