# 新建会话原生保真 01 — 指定交付包的监工复核

日期：2026-10-22，Asia/Shanghai。关联流程：NS-FC01。
**包身份和静态完整性已核实；不是视觉通过，不是整批界面完成。**
本记录承接用户提供的实际产物，原校准三文件与历史回执均保留原样。

## 1. 本次核对的唯一产物

```text
/Users/macbook/Downloads/同步空间/LYX/Aeroric/.artifacts/native/approved-ui-integration/20261022-115525-main-shell-closeout/Debug/AeroricNative.app
```

显示名 `Aeroric Native Preview`，ID `com.aeroric.native.dev`，
`0.1.0 / build 1`，普通 Debug、arm64、本地 ad-hoc。
打包回执时间为 **2026-10-22 12:02:30 +08**；不是此前的同名旧包。
本次独立静态观察时间为 **12:47:46 +08**。

| 对象 | SHA256 |
| --- | --- |
| 已签主程序 | `43a2ffce8a65daa5d3dd6ffb1d165a24054917cd7db30d601b7c344f825fbeb7` |
| 已签 `aeroric-cli` | `2700f5332f14fcc25248396a614500dcc0b015abbcadfd77c9055de7ec954ff5` |
| 交付方最终源码清单文件 | `65f96853e0050f2f8309a121337f72c7f6c23b57396ace67dad61fab911c8987` |

不能只凭显示名或 build 1 认包。交付方本轮是复制既有 compile-only
产物、离线编译当前 helper、签名和静态验签，并非新一轮保真修正。

## 2. 独立核验结果与证明边界

不是仅采信执行者的“完成”报告。本线程实际做了以下只读检查：

| 检查 | 结果 |
| --- | --- |
| 当前 bundle 文件树/字节 hash/文件模式对签名收据 | 8 文件全部匹配，无新增或缺失文件 |
| 交付方最终源码清单对当前工作区 | 263 项匹配，0 差异 |
| helper 本地编译输入对当前源码 | 1010 项匹配，0 差异 |
| 收尾证据清单、前轮验证引用 | 分别 86 项、15 项匹配，0 差异 |
| 原 compile-only 包的文件 hash | 6 项匹配；未签主程序 hash 与打包收据一致 |
| `codesign --verify --strict --verbose=2` | App 和 helper 分别 exit 0 |
| `codesign --display --verbose=4`、`file` | ad-hoc、无 TeamIdentifier；两程序 arm64 |
| `assetutil --info` | 编译资产含 HarnessCodex/Claude/OMP/DSH 四个名字 |
| JetBrains Mono 与 OFL | 源文件、bundle 与交付收据 hash 一致 |
| 前轮 Swift 原始日志逐方法计数 | 82 passed / 0 failed / 0 skipped；**本次未重跑** |
| 前轮普通 Debug 编译原始日志 | exit 0、`BUILD SUCCEEDED`；**本次未重建** |

这是源码清单、原构建记录和具体包之间的追溯核对，不是本线程重做了
可复现构建。资源存在不证明字体注册、中文回退、图标显示、液态滑块或
实际尺寸正确；签名完整不证明 host/四 Harness/真实任务可用。
该本地包也不是 Developer ID、公证或 App Store 验收。

本线程证据：
`.artifacts/native/new-session-fidelity-review/20261022-124746-delivery-01/`，
包括 `static-review.json`、实际静态命令日志及 `original-log-review.json`。
原交付证据位于 `approved-ui-integration/20261022-115525-main-shell-closeout/`，
没有执行其中的打包脚本，也没有重新签名或改动交付包。

## 3. G0–G4 状态：不再把“有包”当“还原通过”

| 闸门 | 当前结论 |
| --- | --- |
| G0 版本/批准范围/D01–D06 回执 | **PARTIAL**。已有实施批准与版本材料，但在本次检索的任务研究和交付报告中未找到完整 NS-FC01 接收/逐项对账回执；不能由此断言执行者未阅读 |
| G1 同源接入 | **静态部分已证实**。普通链路消费新主壳及原 NewSessionComposer，不是另一个漂亮 demo；标题、宽度等仍需绑定版本 |
| G2 检查/具体产物 | **指定包身份与静态完整性已核实**。原编译/82 项结果按原批次保留，不转记成本轮执行或保真通过 |
| G3 实际原生对照 | **NOT RUN / DEFERRED**。本线程没有启动、获取截图或执行任何 UI |
| G4 用户确认/原生基准 | **PENDING**。用户给出产物路径，不等于确认该包的具体页面与状态 |

普通路径为：
`AeroricEntryPoint → AeroricNativeApp.mainContent → WorkspaceRootView →
WorkspaceDetail → TaskWorkspaceView → NewSessionComposer`。
App 普通入口默认到全部任务，并非自动显示一个新建会话 fixture。

## 4. 上轮差异账的当前观察

以下是静态对账，不是重开所有已批准设计，也不是指定执行者一律改成新值。

| ID | 本次观察 | 后续最小动作 |
| --- | --- | --- |
| D01 批准记录 | `main-shell-reconstruction-review-v1.md` 仍记四屏及 24 样本 PENDING；既有 `native-implementation-approval-2026-10-22.md` 记四屏获确认。交付报告保留了两者，未给出各局部的采用范围 | 保留后续已记批准，不因旧 PENDING 判定四屏全未获批。现有 owner 用已有回执说明 V6/MSR01 的局部叠加；不能把 24 个样本补记为全通过 |
| D02 主壳几何 | **原静态接线缺口已推进**：具名 mainShell 是 25/7/6，普通 Root 消费同一预算，默认 1260×800、下限 1000×640；旧 18/8/8 review 值保留 | 不再重复要求“把旧 token 全改掉”。实际窗口、安全区、材料衔接仍待指定包核对 |
| D03 输入圆角 | 实际保留 V6 的 32/28，提示词最小 108/104；MSR01 CSS 的 16 没被静默抄入 | 将“保留 V6 输入几何”的适用依据写入 G0；不能因为后来的 HTML 为 16 就自动要求改小 |
| D04 标题/字体 | 正文/辅助等改用 AeroricFont；**引言仍为 Songti SC/serif、宽版 40 / 紧凑 32**，英文引导仍为系统 8/7。新稿则写统一 Mono/中文回退和 16/22 小标题、不再巨大欢迎标题；已读批准回执未见该处豁免 | **P2，剩余契约差异**。落实所采用的新标题角色，或补交明确保留 V6 艺术标题的适用回执；不得把 40/32 追认为 V6 CSS 原值 |
| D05 宽度坐标 | 930 是 padding 后的容器上限，不是输入框宽度。足够宽、非 compact 时，按 modifier 顺序推算内部列 930−34×2＝862；context 再扣 6×2 后为 850，而输入框外缘没有这层扣减。MSR01 的 850 共同父列则同时包住 context 和 input | **P2，采用 MSR01 时的静态契约差异**。明确共同列/输入外缘的采用规则，落实实际消费或登记批准例外；不能借 context 恰为 850 销账，不是误报 80pt，也不是实际量测 |
| D06 Harness | 四顺序、对应资源、选中带名称/其余图标的系统桥接路径保留；已编译资源也齐全 | 公开系统选择器不等于真实玻璃滑块/图文/动画/辅助功能已通过；必须另核具体包，不手画 active 底来掩盖系统差异 |

关键源码定位：

- `NativePanelGeometry.swift`：`MainShell`、`mainShellBudget`；
  `WorkspaceRootView.swift`：预算、GlassPanel 和外侧 padding 消费；
  `AeroricApp.swift`：普通入口、unifiedCompact 和默认窗口大小。
- `NewSessionGeometry.swift:5–14`：930、padding、34/28/16、
  32/28 与 108/104；`NewSessionComposer.swift:32–64`：实际 modifier 顺序，
  `:77–110`：引言字体与字号，`:112–164`：框外上下文栏和输入形状。
- `ThemeTokens.swift:52–62`：AeroricFont 角色；
  `NativeSelection.swift`：NativeHarnessPicker 系统装配。
- MSR01 资产 `style.css:113–123`、`view.js:80–94`：
  850 的 readable-column 是 context 和 input 的共同父列。

上述尺寸推算只在足够宽、同一坐标层等前提下成立；不是截图量尺或整个
SwiftUI 布局求解的替代。颜色也应随最终采用版本逐项核对，不能因标题
目前中性就单凭旧稿判为已证实回归。

独立只读审查员 Carson 复核了本节普通调用链与 D01–D06，识别同样两项
P2；回报另存本线程证据的 `independent-source-review.txt`。
该审查未运行 App、构建或测试，不取代 G3/G4。

## 5. 不在这份包里自动完成的内容

交付方报告自己也明确保留：

- 四 Harness 的完整执行、配置和凭据装配尚不齐全。
- 账号与连接等整页、新版工具库及部分后续屏幕没有由本次交付实现；
  这不改变其他设计文档各自的批准状态。
- 八组隔离原生样例不是已落地的生产能力；独立协作 B02 仍缺失。
- 安全完整 Git diff、事件 freshness、宿主权威输入票据/多窗口终端、
  品牌资源分发许可等仍未完成；不能靠本次签名销账。
- 工程记录 macOS 14 与产品 26 基线差异仍交平台 owner。

因此本线程不接受“之前确认的所有界面已一模一样做完”的结论，也不替
既有执行任务归档、修改状态或取消其他工作授权。

## 6. 给现有执行者的下一次短回报

先补对账，不先重做全部页面、不复制第二套新建会话 View：

1. 回 `NS-FC01 接收`，列采用稿件/修订/指纹和已有批准依据；
   对 D01–D06 分别写保留、已接入、待核对及理由，尤其标题和 850 的边缘定义。
2. 指明哪些现有参数已经由普通入口消费。保留本次正确接入的
   25/7/6、字体资源及草稿/typed 提交，不把测试数字改成设计答案。
3. 若既有回执确实不能解决某一视觉局部，只把该局部交用户确认。
   实质改版仍按逐屏审批规则走，不能据本监工记录新增实施/运行权限。
4. 需要修正时，按现有任务和文件 owner 的授权推进；新包另交
   hash/patch/检查结果，不原地换掉此包或改写旧收据。

本记录没有联系到实际执行终端的指令通道，不声称已向它发出命令或得到 ACK。

## 7. 接下来怎样看到真实效果

本线程下一步聚焦这个包的**新建会话**，不是继续批准更多没有原生对照的页面。
优先只看同内容、同壳尺寸的浅色默认态，再看石墨深色；其余状态逐项扩展。
用户若已自行查看并提供该包的截图，可先做静态分区对照。
截图也不能证明系统玻璃动画、IME、键盘、AX 或真实发送。

**本包不是离线查看器。** 普通启动的 `.task` 会启动或附着 host；
`AppServices.swift:131–150` 会建数据根、恢复书签和读取策略。
独立 data root 仍不等于隔离真实 HOME、共享 Keychain、偏好或网络。
交付 README 还披露了远控全局根、附着 host 退出和长 socket 三项风险。
不得直接照抄旧启动脚本，尤其不能使用带 `pkill` 的测试变量。

由 Agent 代开仍需**针对这个具体包的新明确授权、资源/副作用审查和已审政策更新**。
本次没有代开，也没有改政策、创建运行根、操作真实账号/Agent/API 或 VM。
App/Preview、浏览器、截图采集、UI/AX/输入、原生视觉和业务验收均为
**NOT RUN / DEFERRED**。没有产品源码修改、构建/测试重跑、提交/推送/发布。
