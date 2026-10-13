# ADR-039 — 分发枢纽转型:纯 macOS/iOS 原生(舍弃 Windows/Linux 客户端)

- 状态：**policy + sequencing accepted（2026-10-12，本轮 Q1–Q12 及总评审）；规划已批准、尚未实施**。当前以 §7 与 `MIGRATION_PLAN.md` 顶部 Apple-only 基线为准；§3/§6 保留提案时记录，不是待答问题。
- 日期:2026-10-12
- 关联：`MIGRATION_PLAN.md` 当前 §A–G / AP0–AP10；原 INV-09/10/11、§0.1/§6.1/§6.2 的冲突记录留在旧正文；`docs/native/reviews/ide.md` §8–9。
- 决策证据：原声明与策略表保留于 §1/§4；本轮 grilling + Trellis 访谈及用户“确认，按此完成计划修订”记录在 §7。仅批准规划，不授权执行删除、CI/签名变更或发布。

## 1. 用户声明原文(逐字)

> 经过我慎重考虑,我决定彻底舍弃windows和Linux客户端,只做macos和ios端,前端只使用swiftUI,删除wsl等windows独占的功能,我现在买了Apple developer,将当前项目转型成纯macos原生的项目,后面我要上架Apple store

## 2. 这条声明实际改变了什么(逐条,按对象)

| 对象 | 今天的实况 | 声明要求 | 冲突的既有约束 |
|---|---|---|---|
| Windows/Linux 的 **Tauri 跨平台客户端** | 存在且被当作回退 UI 与正式客户端(`§0.1` 第 3 条、`§6.1` 第 12 框) | 彻底舍弃 | **INV-10**「迁移不得以『macOS 不用』为由删除跨平台后端」;§6.1「原 Tauri Windows/Linux 与 mobile/relay 协议保持可用」 |
| **WSL 等 Windows 独占功能** | core 有 `platform/{unix,windows}` + `connections/{wsl,wsl_files,wsl_git}`,P7-02 已 accepted,含 5 条门禁用例 | 删除 | INV-10;P7-02 的用户验收记录(删除属改写已验收范围) |
| **iOS 端** | 计划里只有 macOS 原生;mobile 是 React/Expo 目录,不是 iOS 原生 | 新增 iOS 原生 | 无既有约束,属**新增范围** |
| **前端技术栈** | React 保留(INV-11);SwiftUI 只做 macOS | 「只使用 swiftUI」 | **INV-11**(React 修改仍走 pnpm/Vite/TS 与 `AnimatedSelection.tsx`) |
| **分发** | 当时为 ad-hoc 本地基线，非商店路径 | App Store 上架 | **INV-09** 需显式修订。更正：Developer ID/notarization 是站外分发路径，不是商店签名的别名；商店签名、App Sandbox、权限与审核必须分别验证，见 §7 |
| **remote / relay / mobile 协议** | F18、P8-03 accepted、`remote-protocol`/`remote-relay` 保留 | 未直接提及 | 若「只做 macOS/iOS」被读成删除 remote 面,则与 §6.1 冲突;若只是客户端形态,则不冲突 |

## 3. 历史：提案时为何未直接 accepted

1. **它不是 slice110 的边界选择**。原问题是「宿主是否最终接管远端 LSP」;作答是更大的范围。
   把一次架构转向登记成某个切片的设计决议,会让后来的读者以为切片里裁决过它。
2. **它与已冻结的 INV 集直接冲突**,而 INV 集按 `§0.2` 是「不可变条件」。`§0.2` 的改法只有一条:
   显式评审。agent 不能单方把 INV 改掉。
3. **「彻底舍弃」不是一个可一次执行完的动作,而是一串有依赖的决策**:
   - CI(`.github/workflows/checks.yml` 的 ubuntu/windows 矩阵)、
   - `src-tauri/` 的 Windows/Linux 专属代码与 `cfg` 分支、
   - remote/mobile/relay 是否随之删除、
   - 已 accepted 的 P7-02(WSL)与 P8-03(remote)的用户验收如何回退或改派、
   - iOS 原生是新工程(新 ADR:技术栈、最低系统、与现有 core/IPC 的边界)、
   - 商店上架与 INV-09 的 ad-hoc 基线是两条签名路径,需要显式切换规则。
   每一条都要自己的 ADR 与影响清单。

## 4. 逐行确认(2026-10-12,grill-me 首轮,用户逐题作答)

| 行 | 问题 | **用户决定** |
|---|---|---|
| R1 | INV-10 处置(删除 vs 保留 vs 降级) | **彻底删除 Windows/Linux 客户端代码与 CI 矩阵** |
| R2 | WSL 等 Windows 独占功能 | **删除 wsl 模块与 P7-02 验收范围** |
| R3 | CI 矩阵(x86_64-apple-darwin / ubuntu / windows) | **随 R1 一并删 ubuntu/windows CI job** |
| R4 | INV-11(React UI 工具链) | **改写 INV-11:前端全面转 SwiftUI,逐步废除 React** |
| R5 | 分发/签名(INV-09 ad-hoc 基线) | **直接改写 INV-09 转为商店签名基线** |
| R6 | remote/relay/mobile 协议面 | **保留协议,把 iOS 客户端接入 remote 面** |

**策略逐行确认已完成。** 原执行排序的两轮记录见 `reviews/ide.md` §9；当前更完整的范围、顺序与退出门槛见 §7，不再等待 §6 的旧问题。

## 4b. 决策落实与实施状态

- [x] 原 R1–R6 策略逐行确认，本轮 Q1–Q12 与总评审完成。
- [x] 主计划顶部显式修订 INV-09/10/11、目标/验收与 AP0–AP10；旧正文作为历史保留，不回填旧 accepted。
- [x] 明确商店/开发/站外签名差异、iOS 伴随边界、Liquid Glass 最低系统、Intel 上限及后台/推送/relay 责任。
- [x] `docs/native/rollback.md` 增加当前 Apple-only 退场前/后边界。
- [x] AP0 商店沙箱、本地执行、许可与实际发布前置获得真实证据 (见 `.artifacts/native/ap0/`、`batch1-results.md`、`batch2-results.md`、`license-and-gaps.md`)。
- [x] AP1 规则、任务、清单与工具重定基线规范完成 (见 `docs/native/ap1-rebaseline.md`)。
- [x] AP6 APNs 与自托管公网使用闭环完成(发送端 `remote-push/`、core 策略与派发、
  宿主出口与 relay 诊断、iOS/macOS 客户端与 entitlement、门禁模块 `push`);私钥注入与
  真机投递按 `GAP-STORE-06` 具名阻塞,见 `docs/native/adr/040-push-plane.md`。
- [ ] 工程验收、真实迁移演练和双商店审核通过；实际清退另获授权后执行。

勾选的规划项不代表运行时、签名、CI、用户数据或商店操作已经改变；本 ADR 尚非 fully-executed。

## 5. 当前执行边界

- 本轮仅修订计划，不删除现存实现、不改 CI/签名配置、不操作生产数据、不提交或发布。
- G43 的**目标 owner**由本轮 Q3/Q4 及 INV-02 明确为 Mac host/core 的 SSH/凭据平台适配；不能把保留远端业务永久留在待删除的 Tauri。**已由 AP2 (`connection_services.rs` + `ap2_host_remote_owner.rs` 运行证据) 正式闭环**。
- 旧前端冻结新功能但保留必要安全/回退维护；退场必须满足 §7 的 Q12，不再以“窗口能跑”作为充分条件。
## 6. 历史：第二轮执行排序提问

以下保留原提问，包含已过时的阶段描述；不是当前源码事实或未决清单。后续两轮排序见 `reviews/ide.md` §9，当前裁决见 §7。

R1–R6 是**策略**决定;把它们变成代码删除与 CI 变更之前,有一串依赖必须先排:

1. **没有可交付客户端的时间窗**:今天的 macOS app 就是 Tauri+React,而 SwiftUI 原生客户端
   只到 Phase 4–10(pending);iOS 是全新工程。若按 R1+R4 立刻删 React 与 Windows/Linux,
   产品将**没有任何可用客户端**直到 SwiftUI 完成。⇒ R4 那句「**逐步**废除」是关键限定词。
2. **删 CI 与保门禁的矛盾**:R3 删掉 ubuntu/windows job 后,跨平台回归就再没有机器看;
   而 `core/` 的很多用例当前**假设 macOS 交接语义**(见 `MIGRATION_PLAN.md` 切片110 记账
   的「第七层」7 条失败)。删除顺序决定这 7 条是被 gate 掉、还是随 CI 一起消失。
3. **已 accepted 任务的回退**:P7-02(WSL)整条验收按 R2 作废;P8-03(remote)按 R6 保留。
   作废已 accepted 的验收记录属改写历史范围,必须写明改的是哪一份工件。

**本段原先等待的排序已获答复并由 §7 更新。** 历史提问不构成删除授权，也不能覆盖本轮确认的双商店退场门槛。

## 7. 当前决策：Apple-only 总评审（2026-10-12）

本轮用户要求用 grill-me 与 Trellis 优化主计划；使用 `grilling` 与 `trellis-brainstorm`，先核对仓库事实，再逐轮询问产品取舍。取消的 Q3 首问没有被当作答案，用户“继续”后重新提问并获答。

| 问题 | 用户选择 | 生效含义 |
|---|---|---|
| Q1 分发优先级 | C | 两端商店是硬目标；不默认站外完整版、不绕审，不兼容能力逐项裁决。 |
| Q2 本地产品底线 | A | Mac 不依赖其他执行机器完成本地 Agent 编码闭环；四族/自定义范围不靠单族探针偷减。 |
| Q3 iOS 独立性 | A | iOS 是在线 Mac 伴随端；远端服务器由 Mac 接入，不新增手机直连执行宿主。 |
| Q4 对端系统 | A | 保留既有 Linux/Unix、macOS、Windows SSH/SFTP；退场的是非 Apple 客户端/WSL/Android，Linux relay 与安全审计仍保留。 |
| Q5 首发范围 | B | 原 Phase 9 新增能力也必须首发完成，不自行延期；真实上游不支持项具名阻塞并再裁决。 |
| Q6 后台宿主 | B | 显式 opt-in 后 UI 退出仍可运行/远控；有状态与停止入口，不默认登录自启、不承诺防睡眠或关机可达。 |
| Q7 手机通知 | A | 首发 APNs 提醒及受控发送端，敏感业务不上推送，通知不是审批授权，不承诺必达。 |
| Q8 iOS 功能 | A | 完整任务伴随与相关高级操作，不复制完整桌面 IDE/DAP、DBX 网格、基础设施/笔记工作区。 |
| Q9 系统与架构 | 两次自定义答复 | 原生 Liquid Glass 为最低条件：macOS/iOS/iPadOS 26 起，Apple Silicon/移动端验证到 27；保留官方可运行 macOS26 的 Intel，不建立 Intel/macOS27 组合。 |
| Q10 服务运维 | B | relay 用户自备，产品方仅维护 APNs 发送端；无默认官方 relay/云端 Agent，私钥不分发给用户或第三方 relay。 |
| Q11 数据升级体验 | A | 接受停任务/备份/授权导入/核对与必要重新登录/配对；源数据保留，不接管旧存活 PTY。 |
| Q12 旧前端删除 | B | 工程与迁移验收、两端商店审核通过后才删除；共享业务/vendor 先迁出，审核不是发布授权。 |

Q9 原文先要求“mac和ios放大兼容性,最新支持到最新到macos27和ios27,同时也要兼容Intel的mac”，随后明确“最低的需要支持苹果原生的液态玻璃”。依据 Apple API availability，最低版本据此定为26；此前14/15.1/17等备选不再有效。系统减少透明度/动态效果等可访问性不因玻璃要求而失效。

**总评审批准原文：**“确认，按此完成计划修订”。批准的是主计划/关联规划文档，不是实现、代码清退、商店提交或发布。

### 与旧策略的精确衔接

1. R1/R2/R4 的最终清退目标保留，但最终删除门槛由 Q12 明确强化；不以旧“SwiftUI 能跑”代替完整验收和双商店审核。
2. R3 的 Ubuntu/Windows **客户端** job 退场，不意味着删除 Linux relay 部署验证或跨产品安全审计。先拆分/迁移检查，再改矩阵；不能靠平台退场消失红灯。
3. R5 改的是目标分发基线，不是本轮已修改 `tauri.conf.json`。开发/测试签名与商店身份分离，发布不能 ad-hoc fallback；本地签名成功不证明审核可过。
4. R6 保留协议并新增原生 iOS 网络实现，不是把本地 UDS `RemoteClient` 误当手机客户端；AP2 补完整 headless remote，AP5 做原生端到端。
5. P7-02 的旧验收是历史事实，新产品范围将其退休；不篡改旧 accepted、不删除用户旧平台记录，也不把退休计作新增迁移交付。

### 前置门禁与来源

执行顺序及可观察验收见主计划 AP0–AP10。首要未证事项是“商店 sandbox＋本地四族/工具＋现有扩展”能否共同成立，以及本仓 GPL/第三方分发条件；必须先实证，不写成可行性承诺。

- [Apple 商店规则](https://developer.apple.com/app-store/review/guidelines/)、[证书类型](https://developer.apple.com/help/account/certificates/certificates-overview/)、[sandbox helper](https://developer.apple.com/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app)。
- [Liquid Glass availability](https://developer.apple.com/documentation/swiftui/view/glasseffect(_:in:))、[Intel 支持上限](https://developer.apple.com/documentation/macos-release-notes/macos-26_4-release-notes#Rosetta)。
- [APNs 发送端与投递机制](https://developer.apple.com/documentation/usernotifications/setting-up-a-remote-notification-server)。
