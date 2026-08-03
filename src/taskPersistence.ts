import type { Task } from "./types";

type SaveProjectTasks = (projectId: string, tasks: Task[]) => Promise<unknown>;

type PersistOptions = {
  onError?: (msg: string) => void;
  formatError?: (error: string, projectId: string) => string;
};

type ProjectPersistState = {
  latestTasks: Task[] | null;
  latestOptions: PersistOptions;
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
        latestTasks: null,
        latestOptions: {},
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
      while (state.latestTasks) {
        const tasks = state.latestTasks;
        const options = state.latestOptions;
        state.latestTasks = null;
        try {
          await saveProjectTasks(projectId, tasks);
        } catch (e) {
          console.error(e);
          options.onError?.(
            options.formatError ? options.formatError(String(e), projectId) : String(e),
          );
        }
      }
    })().finally(() => {
      state.flushPromise = null;
      if (!state.latestTasks && !state.timer) {
        states.delete(projectId);
      }
    });
    return state.flushPromise;
  }

  const persist = ((projectId: string, allTasks: Task[], options: PersistOptions = {}) => {
    const state = stateFor(projectId);
    state.latestTasks = allTasks.filter((t) => t.projectId === projectId);
    state.latestOptions = options;
    if (state.timer) clearTimeout(state.timer);
    state.timer = setTimeout(() => {
      state.timer = null;
      void startFlush(projectId, state);
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

  return persist;
}
