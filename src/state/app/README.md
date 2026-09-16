# `src/state/app` 边界

桌面端应用级状态与任务编排。**不要**在这里放组件级 UI 状态（弹窗开关、hover 等）。

## 双权威模型（当前）

| 数据 | 权威源 | store 角色 |
|------|--------|------------|
| `projects` | `AppShell` `useState` + `persistProjects` | **只读镜像** `useProjectsStore.syncFromHost` |
| `tasks` | `AppShell` `useState` + `persistProjectTasks` | **只读镜像** `useTasksStore.syncFromHost` |
| 外观（主题/字号） | `useAppAppearance` | `appearanceStore` 可独立演进 |

写盘只走 App 的 persister（带 toast），store 内 persist 仅给「未来真正换权威」用，**避免双写**。

`AppOpsProvider` 可注入宿主 `projectOps`/`taskOps`：有注入时用宿主实现（接 App persist/toast），否则回落 store。

## 模块类型

### Pure（无 React）

| 文件 | 职责 |
|------|------|
| `taskLaunch.ts` | local/ssh/wsl/dsh 的 run/resume/cancel invoke |
| `taskStatus.ts` | 状态机迁移 + 落盘副作用 |
| `taskSwitch.ts` | 切换 agent 配置编排 |
| `taskDeleteDone.ts` | 删除 / 标记完成 / 重连 |
| `taskNaming.ts` | AI 生成任务名、todo 更新 |
| `taskMutations.ts` / `projectMutations.ts` | 列表纯函数 |
| `worktreeOps.ts` | worktree merge/discard/cleanup |

Pure 模块**通过 deps 注入**回调（`setTasks`、`showToast`、`tasksRef`…），不 import 组件。

### Hooks / Providers

| 文件 | 职责 |
|------|------|
| `useAppShellHooks.ts` | store 镜像、生命周期、SSH 持久化、快捷键 |
| `useAppTauriEvents.ts` | 核心 Tauri 事件 |
| `useAppStartup.ts` | 启动加载、DSH 宿主订阅 |
| `useAppMaintenance.ts` | 降级 toast、自动清理、SkillHub 同步 |
| `useRemoteTaskRequests.ts` | 手机远程 create/resume |
| `AppOpsProvider.tsx` / `AppProviders.tsx` | ops context |

### Stores（zustand）

`projectsStore` / `tasksStore` / `appearanceStore` — 模块级单例（单窗口桌面）。

## 调用约定

1. 组件**不**直接 `@tauri-apps/api/core`（eslint 强制）；用 `lib/api/invoke` + 域常量。
2. 新编排优先放 pure 模块；App 只接线 deps。
3. 改 `syncFromHost` 时确认不会引入第二套 persist。
