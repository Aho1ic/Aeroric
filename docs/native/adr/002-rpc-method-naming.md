# ADR-002：本地 RPC 方法名的口径 = 服务面分组 + 推导落表

- 状态：**accepted（用户在 Phase 1 Gate 拷问中拍板）**
- 日期：2026-09-22
- 关联：`MIGRATION_PLAN.md` Task P2-01（冻结 `contracts/native/v1/methods.schema.json`）、ADR-001 §命令类别→归属表、`docs/native/command-map.json` 的 `rpc_method` / `core_service` 字段
- 实测依据（2026-09-22 重算，全部命令名唯一性均已核对）：579 条有 `rpc_method` 提案；**三种候选写法都零碰撞**，因为 660 个旧命令名本来就全局唯一；提案前缀共 **22 组**，与 `core_service` 对齐（database 95、notebook 73、git 72、dsh 65、ide 62、settings 44、files 34、extensions 28 …），其中只有 **229/579** 的前缀等于该命令的 Rust 模块首段，另有 11 行使用 `task/terminal.` 这种分组风格。

## 决策

`rpc_method` 采用**服务面分组**：`<service>.<legacy command name>`，即保持 `command-map.json` 现有提案字符串不变。

其前缀**必须由一张可核对的表推导**，不允许停留在"当初是手工定的"：在 `command-map.json` 增加 `meta.rpc_resolution`，内容为

1. `module_to_service`：56 个 Rust 模块 → 22 个服务面名的映射；
2. `overrides`：按 `rust_path` 具名、且**每条必须带 `why`** 的例外（当前已知 11 条 `task/terminal.*`）；
3. `inventory.mjs --check` 逐条断言 `rpc_method == override ?? module_to_service[模块] + "." + rust_path 尾段`，并拒绝：新模块没有服务面归属、例外失效或无理由、表里列着已不存在的模块/命令。

这与上一轮（第十三次补记）把 `category` 从不可核对的手工字段变成 `meta.category_resolution` + 推导断言是同一手法。**规则**：凡"归属"类字段（能否进 host allow-list、走哪个服务、由谁实现），值必须由已登记的推导可复现，否则门禁失败。

## 为什么不是另外两种

- **A2 源模块前缀**（`pty.run_task`）虽然零映射表、纯规则自证，但线名会绑定**源码文件布局**，而本迁移要改的正是布局：`pty::run_task` 计划拆进 `tasks/` 与 `terminal/`。模块一拆分，要么规则失效（名称与 `rust_path` 首段脱钩却仍宣称可推导），要么被迫改协议名——而清单已实测确认"改任何一个旧命令名都是协议变更，不是重构"。因此 A2 把不可逆的线名绑在了最容易变的东西上。
- **B 去冗余前缀**（`git.git_status` → `git.status`）实测同样零碰撞，但它让 leaf 不再逐字等于旧命令名，三个选项里只有它丢掉这条可追溯性质；尾名与冻结清单逐字相等是本次全部对账工作的锚点，不轻易放弃。

## 后果与约束

- 579 个线名的**可读前缀表达的是"由哪个 core service 实现"**，与 ADR-001 的类别→归属表同一维度；host 的 mutation allow-list 因此可以按服务面成组授权（`git.*`、`notebook.*`），但**不得**把"前缀相同"当作 capability 已登记的证据——登记仍以 schema + capability 清单为准。
- 线名与旧 Tauri 命令名的双向换算需要查 `meta.rpc_resolution`，因此**该表是协议的一部分**：任何改动的评审等级等同于协议变更。
- 未来若某个 service 拆分或改名，属于**协议变更**，需要新的 ADR；不允许"顺手跟着源码重构改线名"。

## 本次拷问中被推翻的两个说法（留档，避免后来者复述）

1. 我最初给"保留 `domain.name`"的理由是"尾名与 `rust_path` 逐字相等，因此可由规则推导"。实测只有 229/579 成立，故该理由作废，选项被重新定义为你现在看到的 A1′/A2/B。
2. 我第一次量"去冗余前缀是否碰撞"时报出 30 个碰撞（`git.status`、`git.list_branches`、`git.log`）——那是脚本自身把两种改写混用造成的假阳性，重算后为 0。结论沿用第十三次补记之前的更正：**三种写法都不碰撞**。
3. 文档里"20% 的命令带冗余前缀"也是未回源的旧数：按模块口径实为 **184/579 ≈ 31%**。
