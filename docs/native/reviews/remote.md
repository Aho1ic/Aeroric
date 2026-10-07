# P8-03 评审记录 — remote/mobile 彻底脱离 React 任务请求中转（2026-10-06 收口）

## 1. 范围与实现（六刀）

- **刀一**：core `remote/` 纯逻辑半边 8 文件 2,846 行上收（protocol/crypto/
  orca_crypto/terminal_frames/terminal_hub/event_log/agent_keymap/auth）；
  hub 历史长度可注入 resolver（G24 读取面一个点）。
- **刀二**：task.create/resume **宿主原生化**——`execute_task_request` 直连
  core TaskService＋launch/resume_via_core；ready 闸＋per-project 串行队列
  （**G05 第三处 host 侧等价判决**）；remote.task_request 转 not-forwarded；
  React hook（useRemoteTaskRequests）退役。
- **刀三a**：core config/identity 纯逻辑（RemoteConfig＋端点校验＋
  host_name/lan_addresses/pairing_endpoints）＋6 测试。
- **刀三b**：core server/state/rpc/orca_rpc **全套上收**——WS 服务循环、
  E2EE 配对状态机、28 方法 dispatch；RemoteHostHandle trait（24 方法
  async-trait）注入宿主能力；ServerHandle de-tauri 化（post_shutdown 钩子）；
  宿主 bridge.rs 实现 trait；remote_headless.rs（auth_stress 8 条＋headless
  serve e2e：配对→list→create→cancel 全 WS 流程）。
- **刀四收尾**：aeroric-host `CliRemoteHandle` 真实现接 TaskService
  （open_local→create→start 链、cancel/complete 裁决、executor 短顶替 input）；
  cli serve 按 remote-config.enabled 拉起 core remote server。
- **刀五**：aeroric-host `register_remote_services`（remote.* 九方法进 UDS
  dispatcher——与桌面 Tauri 命令共用同一 core 控制面）；core control.rs
  控制面纯函数；Swift RemoteClient（九方法类型化客户端）＋RemoteStore＋
  四视图（RemoteAccessSettingsView/PairingView/DeviceListView 二段撤销/
  RemoteAuditView 只读）。
- **刀六**：4 条 remote topic **双发接线**（device_paired/pinned_changed/
  app_settings_changed/terminal_resized：legacy 通道＋v1 topic 同 payload）
  ＋五条 gap 收口＋`native:check remote` 门禁注册（4 case/30 用例）。

## 2. 验证实测

- core 全工作区 --all-targets **2031/0**（本任务首次全绿跑；含
  remote_headless 9＋protocol_roundtrip 11＋host 25）；宿主 lib **1277/0**
  （无代理；地板 1285→1277 双写）；remote 族 49/49。
- `native:check remote` **4 case/30 用例**（headless 9＋lan_roundtrip 7＋
  network_exposure 9＋ui 5，floors 双写）。
- Kit 全包 'All tests' passed（RemoteClientParsingTests 5＋金样计数
  mapped 9/pending 29 同批）。
- mobile：`pnpm --dir mobile typecheck` rc 0；`pnpm --dir mobile test`
  **234/234**（32 文件）；remote-protocol 2/0、remote-relay 21/0（CI 同形态
  --all-targets --all-features）。
- smoke rc 0、doctor 18/18、contracts 12 份逐字节、inventory consistent、
  digest verified、typecheck 干净。

## 3. 真发现与根修

- **revoke 重入死锁**：`revoke_device_for_test` 持 lifecycle 锁调 acquire 版
  reconcile → 改 `_locked` 变体（调用方持锁语义显式化）。
- **resolver 假绿**：core `new_in_memory` 不再装 history resolver（lib.rs
  setup 才装）→ hub 水位回填错 → live 帧丢；测试 setup 显式补装。
- **serde_json preserve_order 特性统一**：dbx-core（P7-04 进）的 bson/mongodb
  栈启用 preserve_order → `ErrorDetail::allowed_keys()` 从 BTreeMap 序漂到
  声明序，破坏 P2-01 契约对账——修＝`allowed_keys()` 显式 sort（词表与特性
  开关无关）；protocol_roundtrip 11/11 复绿。这是 core 全工作区 --all-targets
  首次全绿跑暴露的第 4 个潜伏断言（前三条：INV-02 注释、objc2、extraction
  register）。
- **两条环境性假失败甄别**：ALL_PROXY 残留（代理劫持家族）；task-parity
  表时区漂移（offsetMinutes 480→-420，按 AERORIC_WRITE_TASK_EXPECTATIONS=1
  自带流程再生成）。

## 4. 验收原文对照

- core remote 模块＋tasks_rpc/files_rpc **直接接 core service**（create/
  start/cancel/complete 走 TaskService；不再 emit 到 React 等待应答）✅。
- relay v1 保持（remote-protocol 不动）；capabilities 只公布有 dispatcher 的
  方法（RPC_CAPABILITIES 白名单不变）✅；本地 host 方法不自动全开放到 remote
  （28 方法窄面）✅。
- 同一 terminal lease owner/审批 registry 服务本地与远端（core state 单份）
  ✅；多设备抢控制权＝TerminalLeaseRegistry 语义不变 ✅。
- Swift 四视图＋配对/撤销/relay status 复用 core ✅；二维码/ token 不进测试
  日志（审计面只读文本；测试无 token 输出）✅。
- golden fixtures 未新增字段 ✅；mobile 只动契约必要部分（typecheck＋234 用例
  证明）✅。
- tests/remote_headless.rs 迁移 auth_stress/lan_roundtrip ✅；`pnpm
  native:check remote` 4 case/30 ✅；headless serve 下 fake 配对客户端
  list/create/cancel 全通（core e2e）✅。
- **调用 grill-me [remote]**：已真实调用（grilling 链，三问全按推荐获答：
  收口裁定/双面注册设计/G29 挂 P10-05）✅。

## 5. v1 event topic 接线（5 条全决，0 挂账）

- **remote.device_paired / project.pinned_changed / aeroric.app_settings_
  changed / remote.terminal_resized**：4 条 mapped（verbatim，legacy 通道＋
  v1 topic 同 payload 双发——core serve_ws/rpc 与宿主 agent_config_rpc/
  terminal_streams 落点）。event-topics 计数 mapped 5→9、pending 29；
  Swift/TS/Rust 金样三面同批。
- **remote.task_request**：not-forwarded（刀二原生化——RPC 应答即结果，
  legacy 中转整体退役；G05 host 侧等价判决）。
- 无头面 v1 topic 的 has-consumer 路径：events.since 补发面（event log
  512 环）——原生客户端重连补发；非静默丢弃。

## 6. grill-me [remote]（§4.2 门禁，真实调用）

入口 `Skill(grilling)`，frontier 经 AskUserQuestion 提交（2026-10-06），
三问全按推荐获答：

- **Q-R1（收口裁定）**：**接受并收口**——P8-03 accepted，进 P8-04。
- **Q-R2（双面注册设计）**：**认可**——remote.* 九方法在 aeroric-host
  dispatcher 注册与桌面 Tauri 命令共用 core 控制面；无头面不 serve 的方法
  显式报错（不猜协议）。
- **Q-R3（G29 三收法）**：**按台账挂 P10-05**（①部分覆盖：mobile 两条命令
  本 Task 出口真跑；②③归 P10-05，task_ids 已含）。

**G31 三件套（当场记）**：HEAD＝`e045c856f09a875c7b9ced9688cfaa8025f56a76`
（e045c85，2026-10-06）；porcelain＝0 条目（全绿树）；diff hash＝空（无
未提交 diff）。

**五轴自查**：①唯一 owner——remote 命令面收敛到 core control.rs＋
dispatcher 注册一处；②反例——headless e2e（auth→list→create→cancel）、
network_exposure 9 条、lan_roundtrip 7 条、protocol_roundtrip 排序回归；
③边界——audit 落 data-dir（G19 不复活真实家写入）、SSH resume 保持
"无会话拒绝"、无头面显式报错面清单在册；④证据——本文 §2 全部当日实测；
⑤悬置——无产品决策悬置（G29 处置已获答）。

## 7. 挂账

- 刀六后**仍开放**的 v1 topic：无（remote 5 条全决；P8-04 名下 2 条 usage
  面随 P8-04）。
- 会话读取/files/git/agentConfig 的 UDS 面：桌面壳已 serve（原位转发），
  cli 无头面显式报错——headless 完整面归 P8-04/P10。
- mobile 词表单一来源＋parity case → **P10-05**（Q-R3 获答）。
- remote-relay 服务端 E2E（真 relay 进程）→ P10-01（真环境 e2e 家族）。
