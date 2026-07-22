import type { Project } from "./types";

type SaveProjects = (projects: Project[]) => Promise<unknown>;

type PersistOptions = {
  onError?: (msg: string) => void;
  formatError?: (error: string) => string;
};

export function createProjectPersister(saveProjects: SaveProjects) {
  let pending = Promise.resolve();

  return (projects: Project[], options: PersistOptions = {}) => {
    const snapshot = projects.map((project) => ({ ...project }));
    pending = pending.then(async () => {
      try {
        await saveProjects(snapshot);
      } catch (error) {
        console.error(error);
        options.onError?.(options.formatError ? options.formatError(String(error)) : String(error));
      }
    });
  };
}
