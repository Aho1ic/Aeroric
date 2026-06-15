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
  flushing: boolean;
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
        flushing: false,
      };
      states.set(projectId, state);
    }
    return state;
  }

  async function flush(projectId: string, state: ProjectPersistState) {
    if (state.flushing) return;
    state.flushing = true;
    try {
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
    } finally {
      state.flushing = false;
      if (!state.latestTasks && !state.timer) {
        states.delete(projectId);
      }
    }
  }

  return (projectId: string, allTasks: Task[], options: PersistOptions = {}) => {
    const state = stateFor(projectId);
    state.latestTasks = allTasks.filter((t) => t.projectId === projectId);
    state.latestOptions = options;
    if (state.timer) clearTimeout(state.timer);
    state.timer = setTimeout(() => {
      state.timer = null;
      void flush(projectId, state);
    }, debounceMs);
  };
}
