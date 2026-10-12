# ADR-039 — 分发枢纽转型:纯 macOS/iOS 原生(舍弃 Windows/Linux 客户端)

- 状态:**policy accepted(2026-10-12 逐行确认,见 §4);执行排序未定(§6)**——此前为 proposed
- 日期:2026-10-12
- 关联:`MIGRATION_PLAN.md` INV-01 / INV-02 / INV-10 / INV-11、§0.1(交付三个入口)、§6.1(Windows/Linux 与 mobile/relay 保持可用)、§6.2;`docs/native/reviews/ide.md` §8(grill-me [ide/slice110] Q1)
- 决策证据来源:用户在本会话 grill-me 首轮的**书面作答**(原文见 §1);本 ADR 只登记,不改写任何既有记录

## 1. 用户声明原文(逐字)

> 经过我慎重考虑,我决定彻底舍弃windows和Linux客户端,只做macos和ios端,前端只使用swiftUI,删除wsl等windows独占的功能,我现在买了Apple developer,将当前项目转型成纯macos原生的项目,后面我要上架Apple store

## 2. 这条声明实际改变了什么(逐条,按对象)

| 对象 | 今天的实况 | 声明要求 | 冲突的既有约束 |
|---|---|---|---|
| Windows/Linux 的 **Tauri 跨平台客户端** | 存在且被当作回退 UI 与正式客户端(`§0.1` 第 3 条、`§6.1` 第 12 框) | 彻底舍弃 | **INV-10**「迁移不得以『macOS 不用』为由删除跨平台后端」;§6.1「原 Tauri Windows/Linux 与 mobile/relay 协议保持可用」 |
| **WSL 等 Windows 独占功能** | core 有 `platform/{unix,windows}` + `connections/{wsl,wsl_files,wsl_git}`,P7-02 已 accepted,含 5 条门禁用例 | 删除 | INV-10;P7-02 的用户验收记录(删除属改写已验收范围) |
| **iOS 端** | 计划里只有 macOS 原生;mobile 是 React/Expo 目录,不是 iOS 原生 | 新增 iOS 原生 | 无既有约束,属**新增范围** |
| **前端技术栈** | React 保留(INV-11);SwiftUI 只做 macOS | 「只使用 swiftUI」 | **INV-11**(React 修改仍走 pnpm/Vite/TS 与 `AnimatedSelection.tsx`) |
| **分发** | ad-hoc 本地构建(INV-09),无商店 | Apple Store 上架 | **INV-09**(`signingIdentity: "-"`、`hardenedRuntime: false` 是刻意基线);上架需要 Developer ID / notarization / App Sandbox 的另一条显式路径 |
| **remote / relay / mobile 协议** | F18、P8-03 accepted、`remote-protocol`/`remote-relay` 保留 | 未直接提及 | 若「只做 macOS/iOS」被读成删除 remote 面,则与 §6.1 冲突;若只是客户端形态,则不冲突 |

## 3. 本 ADR 的意见:为什么是 proposed 而不是 accepted

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

**状态:policy accepted(逐行已确认);执行(sequencing)见 §6。**

## 4b. 生效条件(满足后本 ADR 才转 fully-executed)

- [ ] 用户对 §2 表格**逐行**确认(尤其 INV-10/INV-11 的处置:改写、还是保留),
- [ ] `MIGRATION_PLAN.md` §0.2 的 INV 表按确认结果显式修订(不静默),
- [ ] 新开 ADR:分发与签名(ad-hoc → Developer ID/notarization/sandbox 的切换与回退),
- [ ] 新开 ADR:iOS 原生的技术栈与 core/IPC 边界,
- [ ] 影响清单落盘(CI 出口、`wsl*` 模块、remote/relay/mobile、已 accepted 任务的回退或改派),
- [ ] `docs/native/rollback.md` 的 Switch inventory 增加「转型后回退」一行。

## 5. 立即生效的部分(不依赖上面的执行排序)

- **不把 Linux/Windows 的任何现存实现静默删掉**。本会话不做删除、不改 CI 矩阵、不改签名基线。
- `G43`(headless SSH executor/凭据 owner)保持 open;grill-me Q1 的原问题(宿主是否接管远端 LSP)
  **仍是悬置决策**,归 G43,不因本声明被当成已定。
- 后续切片若触及 §2 任一行的代码,先回本 ADR 走 §4 的门。
## 6. 执行排序(尚未定,grill-me 第二轮)

R1–R6 是**策略**决定;把它们变成代码删除与 CI 变更之前,有一串依赖必须先排:

1. **没有可交付客户端的时间窗**:今天的 macOS app 就是 Tauri+React,而 SwiftUI 原生客户端
   只到 Phase 4–10(pending);iOS 是全新工程。若按 R1+R4 立刻删 React 与 Windows/Linux,
   产品将**没有任何可用客户端**直到 SwiftUI 完成。⇒ R4 那句「**逐步**废除」是关键限定词。
2. **删 CI 与保门禁的矛盾**:R3 删掉 ubuntu/windows job 后,跨平台回归就再没有机器看;
   而 `core/` 的很多用例当前**假设 macOS 交接语义**(见 `MIGRATION_PLAN.md` 切片110 记账
   的「第七层」7 条失败)。删除顺序决定这 7 条是被 gate 掉、还是随 CI 一起消失。
3. **已 accepted 任务的回退**:P7-02(WSL)整条验收按 R2 作废;P8-03(remote)按 R6 保留。
   作废已 accepted 的验收记录属改写历史范围,必须写明改的是哪一份工件。

**这三条已在 grill-me 第二轮提出,等用户定序。** 在此之前**不执行任何删除**。
