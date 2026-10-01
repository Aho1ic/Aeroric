# tauri → native 迁移评审记录:创建玻璃材质和窗口外观组件(Task P4-03)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**:`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 转交 `grilling`（委托链见 `docs/native/tooling.md` §2/§3）。本会话 `skill://grilling` 加载成功，委托链全程成立（与 P4-02 收口同日同链）。
- **实际调用入口**:① `skill://grilling` 按其轮次格式发出 frontier（§3 两条 Q94–Q95，用户不在场，按 P3-02 起的先例记账）；② 五轴自查（§1，每条事实当场可复验）；③ 当日全矩阵（§2 表，退出码逐条单读）。
- **审查 commit/diff hash**（G31 新规矩，**当场记**）:`git rev-parse HEAD`＝`628afd1429c2446216f00a5b98083a54e374d880`（628afd1，2026-10-01，P4-02 收口的 docs(plan) 提交）；`git status --porcelain`＝**17 条目**（11 个 digest shard＋index 重生成、`BootstrapView.swift` 接线、`project.yml` AeroricUI 依赖、`checks.json` native-design 模块、`Design/` 五文件、`ThemePolicyTests.swift`；另有 `?? .zcodeignore` 是用户未跟踪文件、非本任务、不入 diff）；`git diff | git hash-object --stdin`＝`ac87f7712ed7c427f6a5c55774f4ff11e8c3d520`。
- **问答**:§3 frontier 两条（Q94–Q95）；无产品决策悬置——G22 在本轮闭环（NativeSelection.swift 创建使 AGENTS.md/spec 的"现在时"描述转为事实）。
- **证据**:§1 五轴的当日读数、测试名与行级引用；§2 验收对照表；补记三百零一的逐片实测段（见 `MIGRATION_PLAN.md` Task P4-03 小节与续跑区）。
- **问题级别**:**三处编译面真发现（当场根修），一处并发撞车（人为、已定性）**——① `NativeSegmentedPicker.Segment` 声明 `Hashable` 但 `LocalizedStringKey` 不可哈希（编译错）；② `WindowAppearance.ConfiguratorView` 是 internal 嵌套类而 NSViewRepresentable 方法签名是 public（编译错）；③ `Picker(selection:)` 缺 title 参数——AppKit/SwiftUI 的 macOS Picker 需要显式 title，收为 `groupTitle` 参数由调用点提供（VoiceOver 组名语义本就该由调用点知道）；④ 本轮 16:45 的 launchservices 冒烟 app 与后台 ui-test 并发撞车——AX snapshot 打到冒烟实例 pid 83238，runner 挂 403s 后超时重启、Executed 0 tests，exit 65；杀净残留后干净重跑 7.3s 全绿（教训：**ui-test 前必须确认无 app 实例残留**，与 P4-02 收尾纪律一致）。
- **修复后复验**:本文写毕前当日矩阵全绿（§2 表），全部为当日实测。AppKit 无 UIKit 式 `resolvedColor(with:)`（本 SDK 探针证实 no member），动态色求值改用 `performAsCurrentDrawingAppearance { usingColorSpace(.sRGB) }` 惯用法，两条动态性测试绿。

## 1. 收尾拷问轮（2026-10-01，五轴）

**轮次输入**:P4-02 收口（补记三百）→ 读 primuse 参照（`PrimuseTheme.swift::NSVisualEffectBackdrop` :325／`PMWindowChromeConfigurator` :547／`PMTitleBar.swift`）＋React `tokens.ts` → Design 五文件（ThemeTokens/NativeMaterial/GlassPanel/WindowAppearance/NativeSelection）→ ThemePolicyTests 10 例 → checks.json native-design 模块（unit＋双 absent＋floor 14）→ app 接线（project.yml＋BootstrapView）→ generate/build/ui-test 全绿。

**五轴自查结论（事实，非问题——逐条当场可复验）**:

1. **唯一 owner**:外观偏好唯一 owner＝`SurfacePreference`（glass/classic 两枚举，`SurfaceChoice.resolve(preference:reduceTransparency:)` 纯函数——reduce transparency 强制 classic，优先级无歧义）；材质唯一 owner＝`NativeMaterial`（NSVisualEffectView 桥，`state = .followsWindowActiveState`、`isEmphasized = false`）；selection 唯一 owner＝`NativeSelection.swift`（ADR-001 §4 第 4 条落地，只封装系统控件）；窗口 chrome 唯一 owner＝`WindowAppearance.apply(to:)`（静态幂等 apply）。React 侧 `AnimatedSelection.tsx` 原规则不动。
2. **反例**:`native-selection-no-hand-drawn-state` absent 门禁在 NativeSelection.swift 内禁 `.background(/.overlay(/isActive`——自绘选中态一出现即红；`design-native-and-semantic-colour` absent 门禁在 Design 目录禁 `class_getInstanceMethod|method_exchangeImplementations|.subviews`（不 swizzle、不碰私有子视图——与 primuse 的按钮对齐修复面划清界限）与 `Color(red:|Color(hex|NSColor(srgbRed|NSColor(calibratedRed`（禁 RGB 字面量——颜色必须走语义 role）。两条门禁首跑即绿且正控四条防恒真。
3. **边界**:数字层对齐 React `tokens.ts`（spacing 7 档/radii 5 档/typography 4 档/motion 3 档，ThemePolicyTests 逐组断言——React 侧是 authoring source，漂移即红）；颜色层全部动态系统色（`Color(nsColor:)` 12 个 role＋`Color.accentColor`），light/dark 动态性由两条 resolved-color 断言证明（aqua vs darkAqua 求值不等）；GlassPanel 是 chrome 级壳（侧栏/检查器/浮卡），终端/编辑器文本区由 `Color.dsTextBackground`（`textBackgroundColor`，天然不透明）承担——**不逐行叠玻璃**是 API 形状约束（文档注释明示）；`interactionMode` 与 `SurfacePreference` 是两个互不相干的 enum，glass/classic 只是外观偏好（AGENTS.md 原生约束的"外观偏好"条款落地）。
4. **回归**:AeroricUIUnitTests **14/14** 绿（ThemePolicy 10＋PackageSmoke 4）；`swift test` 全包绿（AeroricClientTests 28/28 同日复验）；`pnpm native:check native-design` 3 case/14 用例绿（unit＋双 absent）；`pnpm native:build` OK（app 链 AeroricUI product，BootstrapView 接线真实渲染）；`pnpm native:ui-test` **OK**（AppLaunchTests，7.3s——**testmanagerd 已恢复**，P4-02 的 blocked(external) 在本轮开工前补跑解除）；skeleton 24/188（digest 14 shards 重生成后）；swift-client 3/56；inventory consistent（660 mapped）；doctor 18/18；launchservices 冒烟（app 起＋`com.aeroric.native.dev` 数据目录 host.sock/host.token 生成——材质无 crash）。
5. **回滚**:Design 五文件独立可 revert（删除即回 P4-02）；ThemePolicyTests 独立；checks.json native-design 模块＋floor 两条目；BootstrapView 接线回滚＝删 import AeroricUI＋三处 modifier＋GlassPanel 包裹；project.yml 依赖回滚需重跑 generate（project.yml 是提交源、.pbxproj 是派生物）。

## 2. 当日验收对照

| 门禁 | 读数 | 出处 |
|---|---|---|
| `swift test --filter AeroricUIUnitTests` | 14 tests, 0 failures（ThemePolicy 10＋PackageSmoke 4） | 当日 `swift test` |
| `swift test`（全包） | AeroricClientTests 28/28 复验绿 | 当日 `swift test` |
| `pnpm native:check native-design` | 3 case(s) passed, 14 test cases executed（unit 14＋design absent＋selection absent） | 当日 `pnpm native:check` |
| `pnpm native:check swift-client` | 3 case(s) passed, 56 test cases executed | 当日 `pnpm native:check` |
| `pnpm native:build` | OK——AeroricNative.app（AeroricUI 链接＋BootstrapView 玻璃渲染） | 当日 `pnpm native:build` |
| `pnpm native:ui-test` | **OK**（AppLaunchTests 7.3s——testmanagerd 恢复） | 当日 `pnpm native:ui-test` |
| `pnpm native:check skeleton` | 24 case(s) passed, 188 test cases executed | 当日 `pnpm native:check` |
| `node scripts/native/inventory.mjs --check` | consistent（registered=660 mapped=660） | 当日 `inventory --check` |
| `node scripts/native/doctor.mjs` | all 18 checks satisfied | 当日 doctor |
| launchservices 冒烟 | app 起＋host.sock/host.token 生成＋无 crash | 当日 `open` 冒烟 |

## 3. frontier 两条（产品决策，用户不在场，按先例记账）

- ❓ **Q94 - 实机材质视觉验证的通道**:小节 Acceptance"实机将窗后放彩色窗口，激活/失活时材质确有变化"是手工眼验步骤；XCUITest 的 AX 树不携带材质信息，可自动化的部分（app 启动、AX identifiers、无 crash）已全绿。➡️ 推荐:**P4-03 以"原语落地＋自动化面全绿＋材质视觉变化待用户眼验"收口**；P4-04 主窗口实装玻璃面后一并请用户过目（一次眼验覆盖两个 Task 的视觉面）。**标注:用户未答，按推荐项记账。**
- ❓ **Q95 - eyecare palette 的 macOS 侧落位**:React `tokens.ts` 有 light/eyecare/dark 三套 palette；macOS 侧颜色全部走系统语义色（`windowBackgroundColor` 等），没有 eyecare 对应物。eyecare 是内容主题偏好，属 app preferences 范畴。➡️ 推荐:**P4-04 设置窗口决定**——若要做，作为偏好驱动的自定义 NSColor 集合进 Design 目录（仍禁 RGB 字面量则需先立语义层，工程量在 P4-04 评估）；本 Task 只交付 token 基座，不预设。**标注:用户未答，按推荐项记账。**

## 4. G09/G22 判决（本 Task 小节要求的逐条落地）

- **G09（AeroricClient 唯一 RPC 出口反例门禁）→ 已于 P4-02 收口闭合**（`client-socket-io-single-exit` absent case，本日 `pnpm native:check swift-client` 3 case 全绿复验）。P4-03 名下无义务。
- **G22（AGENTS.md/spec 的 NativeSelection.swift 现在时错述）→ 本轮闭环**:文件已创建于 `macos/AeroricKit/Sources/AeroricUI/Design/NativeSelection.swift`（NativeSegmentedPicker＋NativeSelectionList，系统选中态封装，absent 门禁护住"不新绘活动底纹"），AGENTS.md 与 spec 的描述从"将来时错述"转为事实描述，无需再改用户说明书（decision_item 7 不再需要）。P4-03 名下的全部具名义务清零。
