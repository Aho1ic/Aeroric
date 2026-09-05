import type { Task } from "./types";

type SaveProjectTasks = (projectId: string, tasks: Task[]) => Promise<unknown>;

type PersistOptions = {
  onError?: (msg: string) => void;
  formatError?: (error: string, projectId: string) => string;
};

type QueuedSnapshot = {
  tasks: Task[];
  options: PersistOptions;
};

type ProjectPersistState = {
  latestSnapshot: QueuedSnapshot | null;
  timer: ReturnType<typeof setTimeout> | null;
  flushPromise: Promise<void> | null;
};

export type ProjectTaskPersister = ((
  projectId: string,
  allTasks: Task[],
  options?: PersistOptions,
) => void) & {
  /** 立即写入指定项目当前排队中的最新快照。 */
  flush: (projectId: string) => Promise<void>;
  /** 立即写入所有项目当前排队中的最新快照。 */
  flushAll: () => Promise<void>;
};

export function createProjectTaskPersister(
  saveProjectTasks: SaveProjectTasks,
  { debounceMs = 350 }: { debounceMs?: number } = {},
) {
  const states = new Map<string, ProjectPersistState>();

  function stateFor(projectId: string): ProjectPersistState {
    let state = states.get(projectId);
    if (!state) {
      state = {
        latestSnapshot: null,
        timer: null,
        flushPromise: null,
      };
      states.set(projectId, state);
    }
    return state;
  }

  function startFlush(projectId: string, state: ProjectPersistState): Promise<void> {
    if (state.flushPromise) return state.flushPromise;
    state.flushPromise = (async () => {
      while (state.latestSnapshot) {
        const snapshot = state.latestSnapshot;
        try {
          await saveProjectTasks(projectId, snapshot.tasks);
        } catch (e) {
          console.error(e);
          snapshot.options.onError?.(
            snapshot.options.formatError
              ? snapshot.options.formatError(String(e), projectId)
              : String(e),
          );
          throw e;
        }
        if (state.latestSnapshot === snapshot) {
          state.latestSnapshot = null;
        }
      }
    })().finally(() => {
      state.flushPromise = null;
      if (!state.latestSnapshot && !state.timer) {
        states.delete(projectId);
      }
    });
    return state.flushPromise;
  }

  const persist = ((projectId: string, allTasks: Task[], options: PersistOptions = {}) => {
    const state = stateFor(projectId);
    state.latestSnapshot = {
      tasks: allTasks.filter((t) => t.projectId === projectId),
      options,
    };
    if (state.timer) clearTimeout(state.timer);
    state.timer = setTimeout(() => {
      state.timer = null;
      void startFlush(projectId, state).catch(() => {});
    }, debounceMs);
  }) as ProjectTaskPersister;

  persist.flush = async (projectId: string) => {
    const state = states.get(projectId);
    if (!state) return;
    if (state.timer) {
      clearTimeout(state.timer);
      state.timer = null;
    }
    await startFlush(projectId, state);
  };

  persist.flushAll = async () => {
    const attemptedSnapshots = new Map<ProjectPersistState, QueuedSnapshot | null>();
    let hasFailure = false;
    let firstFailure: unknown;

    // A flush can cause another project to enqueue a snapshot (for example,
    // while an exit handler is waiting on a slow disk write). Process newly
    // created state objects and newer snapshots too, but do not immediately
    // retry the exact snapshot that failed: it remains queued for the next
    // explicit flush.
    while (true) {
      const pendingStates = [...states.entries()].filter(([, state]) => {
        if (!state.latestSnapshot && !state.timer && !state.flushPromise) return false;
        return attemptedSnapshots.get(state) !== state.latestSnapshot;
      });
      if (pendingStates.length === 0) break;

      pendingStates.forEach(([, state]) => {
        attemptedSnapshots.set(state, state.latestSnapshot);
      });
      const results = await Promise.allSettled(
        pendingStates.map(async ([projectId, state]) => {
          if (state.timer) {
            clearTimeout(state.timer);
            state.timer = null;
          }
          await startFlush(projectId, state);
        }),
      );
      const failure = results.find(
        (result): result is PromiseRejectedResult => result.status === "rejected",
      );
      if (failure && !hasFailure) {
        hasFailure = true;
        firstFailure = failure.reason;
      }
    }

    if (hasFailure) throw firstFailure;
  };

  return persist;
}
