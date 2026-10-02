# P5-03 评审记录 — 项目、任务列表与原生任务编辑器（2026-10-02 收口）

## 1. 范围

P5-03 是 P5-01/P5-02 之后的 App 场景收口任务：项目/任务列表、原生任务编辑器、
`SessionPresentation` 接线落点。四刀交付（刀一～三为本任务此前已提交的
`93c7f31`/`9d85b7c`/`559dc14` 链，刀四为本次收口接线）。

## 2. 实现链

- **刀一**：`TaskStore`（镜像不推断）＋`TaskListView`/`TaskRow`＋tasks-ui 门禁
  （unit 5）。
- **刀二**：`TaskDraft`（宿主选项校验）＋`TaskDraftView`（fileImporter 附件、
  逐字段错误显示）＋`TaskDetailHeader`（删除/取消/丢弃三分＋运行中二段确认）＋
  `OpenProjectSheet`＋`ProjectGroupEditor`；生命周期 10 态镜像测试、冲突记录/
  清除、草稿校验（TaskStoreTests 10，地板 5→10 双写）。
- **刀三**：`AeroricClient/WorkspaceClient.swift`——project.list/open（ADR-013
  newId）/task.list/get/run/cancel 类型化调用＋宽容镜像解析＋4 解析测试
  （swift-client 地板 28→33 双写）。
- **刀四（本次收口）**：新建 `macos/AeroricApp/Sources/TaskWorkspaceView.swift`
  ——任务列表（窗口局部过滤）＋详情头＋草稿 sheet＋
  `SessionPresentation.resolve` 呈现三态（native 摘要卡 / 终端 / requiresAdvanced
  状态卡＋切换按钮，`forcedTerminal` 仅本窗口本次选择、不自动改全局偏好，
  P5-02 语义）＋选中运行中任务的 P5-01 终端挂载/释放（generation 缺失不挂载）。
  `WorkspaceRootView`/`WorkspaceDetail` 增 `readyContent` 注入点（ready 态正文
  由 App 提供，AeroricUI 无客户端依赖边界 INV-02 不变）。`AppServices` 增
  `toggleStar`/`toggleArchived`（窗口内镜像翻转，宿主权威随下一轮快照校正——
  Q-star-local 按推荐记账）、`generation(for:)`（task.generation 快照缓存，不进
  TaskStore）、`structuredCapabilitiesVerified`（P5-04 前保守 false）、
  `draftOptions`（宿主 capability 回答前＝空集，不伪造可用项）。
  `TerminalCoordinator.PlanTail` 补 public init（跨模块组装用）。

## 3. 接线中修掉的既有缺陷（AppServices 旧代码此前从未编译过 App target）

1. `client.connection` internal 不可达 → `attachedConnection`。
2. `Cursor.bytes(0)` 应为 `Cursor(bytes: 0)`（尾随枚举语法不存在）。
3. `WorkspaceClient.planReplay` 不存在 → 实为 `TerminalStreamClient.planReplay`。
4. pbxproj 登记 `TaskWorkspaceView.swift`（file ref + build file + group +
   Sources）。

## 4. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check tasks-ui` | 1 case / 10 用例 ok（min 10） |
| `swift test --package-path macos/AeroricKit` | 116 passed / 0 failed |
| `pnpm typecheck` | tsc --noEmit 干净 |
| `xcodebuild -scheme AeroricNative -configuration Debug build` | BUILD SUCCEEDED |
| `node scripts/native/smoke-headless.mjs` | rc 0：open→run→input→resume→cancel→session→approval→plan_replay 28B 续读→lease |
| `doctor` | 18/18 |
| `inventory --check` | consistent（660 定义/26 features/40 gaps/51 taskStates） |
| `contracts` | 12 份逐字节 current |

## 5. 验收原文逐条对照

- 新建 `TaskStoreTests.swift`（AeroricUIUnitTests，10 用例）✅；XCTest UI
  `TaskWorkflowTests.swift` **按 Q96 先例留待 P10-02 交互 QA**（XCUITest 需真机
  交互路径，用户已确认记账）✅；`pnpm native:check tasks-ui` ✅。
- 创建→pending→running→input_required→done、cancel/failed/interrupted/resume、
  archive/star/filter：TaskStore 10 态镜像测试＋smoke e2e 真链路（run→done、
  resume、cancel）✅；重启后可见＝镜像权威快照语义，由快照轮询保证 ✅。
- 多窗口 revision conflict：TaskStore `recordConflict`/`clearConflict`（自动
  刷新清除冲突标记后用户重试，冲突条目不本地覆写）✅。
- `grill-me [tasks-ui]`：已真实调用（ask 两问，用户答复：接受并收口＋
  Q-star-local 按推荐记账）✅。

## 6. 已记录的边界与挂账

- XCTest UI `TaskWorkflowTests.swift` → P10-02（用户确认）。
- 星标/归档窗口内假成功（断连时）→ 按推荐记账（用户确认）；真实失败提示随
  P5-04 会话读取收口一并评估。
- 草稿选项集＝空集（宿主 capability 未接）→ P5-04 兑现后由快照填充。
- `structuredCapabilitiesVerified` 保守 false → P5-04 能力协商落地后接真值。
