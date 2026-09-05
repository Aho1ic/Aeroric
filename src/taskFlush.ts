import { flushAllProjectTasks } from "./appProjectState";

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
      "Timed out while saving tasks. The app was kept open.",
    );
  };
}

export const flushTasksBeforeExit = createTaskFlushCoordinator(flushAllProjectTasks);
