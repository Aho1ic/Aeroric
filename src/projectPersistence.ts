import type { Project } from "./types";

type SaveProjects = (projects: Project[]) => Promise<unknown>;

type PersistOptions = {
  onError?: (msg: string) => void;
  formatError?: (error: string) => string;
};

export type ProjectPersister = ((projects: Project[], options?: PersistOptions) => void) & {
  /**
   * 等队列排空。
   *
   * 退出/重启前必须能等它 —— 后台队列里排着的快照是用户刚做的改动,进程走了就没了。
   *
   * 排空时若仍有一次**未被更晚成功取代**的失败,这里 reject。项目文件是整份覆盖写,所以
   * 更晚的成功已经包含更早那份的内容,把它当作已修复是正确的;反过来把失败一律吞掉,退出
   * 流程就会在磁盘满的时候照样退出。
   */
  flush: () => Promise<void>;
};

export function createProjectPersister(saveProjects: SaveProjects): ProjectPersister {
  let pending = Promise.resolve();
  /** 最近一次写入失败,被更晚的成功清掉。 */
  let lastFailure: unknown = null;

  const persist = ((projects: Project[], options: PersistOptions = {}) => {
    const snapshot = projects.map((project) => ({ ...project }));
    pending = pending.then(async () => {
      try {
        await saveProjects(snapshot);
        lastFailure = null;
      } catch (error) {
        lastFailure = error;
        console.error(error);
        options.onError?.(options.formatError ? options.formatError(String(error)) : String(error));
      }
    });
  }) as ProjectPersister;

  persist.flush = async () => {
    /* 等到「等的那一条就是队尾」为止。只 await 一次的话,`await` 让出的这一拍里新排进来的
       快照会被漏掉 —— 而那正是退出前最后一次编辑。 */
    let awaited: Promise<void>;
    do {
      awaited = pending;
      await awaited;
    } while (pending !== awaited);
    if (lastFailure !== null) throw lastFailure;
  };

  return persist;
}
