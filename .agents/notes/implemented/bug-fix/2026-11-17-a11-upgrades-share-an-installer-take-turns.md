# Agent Note: A11 共用一个安装程序的 Harness 升级轮流来

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A11，批次笔记要求动手前复核。2026-11-17 复核 `core/crates/aeroric-core/src/agents/upgrade.rs`（未改过），问题成立：

- `UpgradeCenter::start` 只在同一个 Harness 已有升级在跑时直接返回，`jobs` 按 Harness 存。
- 不同 Harness 的升级各开一个线程，立刻执行各自的命令。
- 升级命令来自 `settings::plan_agent_upgrade`，按当前安装的渠道选：
  - npm 渠道：`npm install -g <包>@latest`，`program` 是那份安装用的 npm；
  - Homebrew 渠道：`brew upgrade`；
  - 官方安装：Harness 自己的二进制。
  
  没装的 Harness 用 `npm install -g`（`npm_install_steps`）。
- 所以在设置里连着点 Claude Code 和 Codex 的升级，两个 `npm install -g` 会同时写同一个全局前缀，可能互相覆盖 `node_modules` 或 `bin` 下的链接。Homebrew 同时跑两个会被它自己的锁拒绝，表现成"升级失败"。

## Decision

**按安装程序排队：一个升级要用的安装程序（npm、brew、官方二进制）被别的升级占着时，先等它装完；等的时候界面写"等 X 装完"。**

- 锁的对象 `installer_key(step)` 是步骤的 `program`，取规范化后的真实路径；取不到就用原样。
  - 同一份 npm，对应同一个全局前缀；同一份 brew，对应同一个 Cellar；官方安装各是各的二进制，互不等待。
  - 没有去跑 `npm prefix -g` 求真正的前缀：那要在点升级时多跑一个进程。现在用的是"同一份 npm"这个更保守的近似，见 Alternatives。
- `Installers` 是一张"安装程序 → 占着它的 Harness"的表，加一个 `Condvar`：
  - 一个升级启动后，在自己的线程里一次拿齐它所有步骤要用的安装程序；任何一个被占着就整体等，不会出现各拿一半、互相等的情况。
  - 跑完（无论成败）全部还回去，唤醒等着的线程。
- 等的时候，任务状态仍是 `running`，`JobView` 多一个 `waitingFor`（占着的那个 Harness 的名字）；拿到后清空。
- 排队只在内存里：宿主重启后，等着的升级和正在跑的一样不再存在，界面回到"有新版本"。这和原来正在跑的升级在重启后的表现一致，批次笔记要求的"不恢复排队"自然满足。
- App：
  - `AgentUpgradeClient.Job` 解码 `waitingFor`，`waitingText` 给出"等 Claude Code 装完"（只在 `running` 时有）；
  - 设置 › Agent 升级的状态胶囊在等待时显示它，图标是沙漏，悬停说明"和它用的是同一个安装程序，同时写会互相覆盖；它装完后自动开始"；
  - 会话页的升级提醒胶囊不变，等待时也显示"升级中"。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补 A11 的落地情况；A11 状态和待目检项回写路线文档 §8 |

## Alternatives considered

**用 `npm prefix -g` 求出真正的全局前缀再加锁。** 最强的理由是两份不同的 npm 指向同一个前缀时也能排上队。没选：每次点升级要先多跑一个 npm 进程，而 Homebrew 和官方安装又各有各的求法；"同一份 npm"已经覆盖常见情况（一份 Node 装着几个 Harness），两份 npm 共用一个前缀的配置很少见。

**所有升级全局排队，一次只跑一个。** 最强的理由是最简单，一定不会撞。没选：官方安装的 Harness（比如 Claude Code 自带的更新器）和 npm 的升级本来互不相干，没必要互相等。

**第二个升级直接拒绝，提示"等前一个装完再点"。** 最强的理由是不用排队机制。没选：用户还得记着回来再点一次；排队等着、到点自动开始，体验一样清楚。

## Consequences

- **收益**：共用 npm 或 Homebrew 的几个 Harness 一起升级时不会互相覆盖，等着的那个说清楚在等谁。
- **代价**：
  - 两份不同的 npm 指向同一个前缀时不会排队，这种配置很少见。
  - 等待中的升级占着一个线程（阻塞在条件变量上），最多四个。
- **验证**：
  - `upgrade_tests`：
    - `upgrades_through_one_installer_take_turns_and_say_whom_they_wait_for`：两个 Harness 都用 `/bin/sh` 跑假安装，假安装把进入和退出写进同一个文件、中间停 0.4 秒。第二个在等待时 `waitingFor` 是"Claude Code"；记录是"进 1、出 1、进 2、出 2"，没有重叠；跑完后 `waitingFor` 清空。
    - `upgrades_through_different_installers_run_side_by_side`：一个用 `/bin/sh`、一个用 `/bin/bash`，两个都在任何一个结束前开始。本机两个名字指向同一个程序时这个测试直接返回。
    - 原有 6 个测试照常通过。
  - Swift `AgentUpgradeTests.testAWaitingUpgradeSaysWhomItWaitsFor`：解码 `waitingFor`，"等 Claude Code 装完"只在 `running` 时出现；旧宿主不带这个字段时没有。
  - 2026-11-17 在 Mac 上：
    - 只格式化本次改动的 Rust 文件，`cargo clippy -p aeroric-core --all-targets -D warnings` 通过；
    - `cargo test --workspace --no-fail-fast` 3757 通过、0 失败；
    - `swift test`（AeroricKit）968 个（跳过 1 个）、110 个，0 失败；
    - App 在临时 DerivedData 里构建成功；
    - `pnpm verify-notes` 通过。
  - 没有跑真实的 npm 升级。界面没有目检，记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：安装程序锁改成 RAII 守卫、线程意外结束时任务记失败、`start` 在一把锁里认领（B1-5），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
