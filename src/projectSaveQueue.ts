import { invoke } from "@tauri-apps/api/core";
import { createProjectPersister } from "./projectPersistence";
import { PERSISTENCE_COMMANDS } from "./lib/api/sftpCommands";

/**
 * 全进程唯一的项目落盘链。
 *
 * `projects.json` 只有一个 `save_projects` 入口，而它是**整份覆盖写**：任务侧好歹按项目
 * 分文件，这份是所有项目一个文件。`createProjectPersister` 只在自己的 promise 链里
 * 排序，所以两个实例同时存在时，"谁最后落盘"取决于两条链各自的调度，而退出前的 flush 只能
 * 等其中一条 —— 另一条里排着的快照是用户刚做的改动。
 *
 * 这里放单例，而不是在 `appProjectState.ts` 里：store 也要用它，而 store 不该为了拿队列去
 * 反向依赖宿主状态模块。
 */
export const queuedProjectPersist = createProjectPersister((projects) =>
  invoke(PERSISTENCE_COMMANDS.saveProjects, { projects }),
);
