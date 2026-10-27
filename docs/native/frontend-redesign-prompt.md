# Aeroric 原生前端重整 — 交给下一个模型的提示词

> 用法：把下面 `---` 之间的全部内容复制给新会话的模型。它自带完整上下文，不需要你补充背景。

---

## 你的任务

重整 Aeroric 的 macOS 原生客户端前端。**方法不是"我猜你要什么然后实现"，而是用 grill-me 逐界面、逐按钮地和我确认，每个界面先用浏览器渲染示例图给我看，我指认之后你再写 SwiftUI。**

用户原话：「当前的 UI 实在是太乱了」。上一轮已经把 12 个平铺 tab 改成四组侧栏，仍然不满意，所以这次要**从头重整**，不是打补丁。

---

## 1. 项目坐标

| 项 | 路径 |
|---|---|
| 仓库根 | `/Users/macbook/Downloads/同步空间/LYX/Aeroric` |
| 原生客户端 | `macos/`（SPM 包 `AeroricKit` + Xcode 工程 `AeroricNative.xcodeproj`） |
| UI 源码 | `macos/AeroricKit/Sources/AeroricUI/` |
| App 接线 | `macos/AeroricApp/Sources/` |
| UI 测试 | `macos/AeroricUITests/` |
| **样式参照** | `/Users/macbook/Downloads/同步空间/LYX/harness/codeg`（Tauri/Next.js，**只取样式/排版/交互，绝不抄代码**） |
| 原生设计决策 | `docs/native/adr/001-native-boundary.md`、`docs/native/adr/042-navigation-is-a-grouped-sidebar.md` |

`harness/` 下还有 `opencode`、`deepseek-harness`、`ZCode`、`orca`、`LiveAgent`、`MiMo-Code`、`minimax-code`，都可作参照，但 **codeg 是主参照**。全部**只读**。

---

## 2. 硬约束（违反即返工）

1. **SwiftUI + AppKit 材质**，部署目标 **macOS 14.0**。产品 UI 不得用 WebView 渲染。
2. `glassEffect` 等 macOS 26+ API 必须 `if #available` 门控，且只用于可交互控件。
3. **材质基座**是 `NSVisualEffectView` / SwiftUI `Material`（`macos/AeroricKit/Sources/AeroricUI/Design/NativeMaterial.swift`）。不得用普通半透明色冒充材质。
4. **颜色只从语义角色取**（`Design/ThemeTokens.swift` 的 `Color.ds*`），**不得把某张截图的 RGB 写进 View**。
5. **相邻互斥控件必须用系统选中态**：`Picker(selection:)` + `.pickerStyle(.segmented)`、`List(selection:)`、`Toggle`、`Menu`。**禁止**手绘 active 背景或下划线做选中态。共享封装在 `Design/NativeSelection.swift`。独立开关/复选框/折叠按钮/树展开器不在此列。
6. **`.accessibilityIdentifier` 只放在叶子上。** 容器上的标识符会**替换**其下所有叶子的标识符，导致测试查不到明明渲染了的元素（已踩三次，见 `feature-matrix.json` 的 G51）。
7. `AeroricUI` **不得依赖 client 包**，不得解析 wire 格式（INV-02）。UI 读的是 app 侧注入的 read model。
8. `glass/classic` 是**外观偏好**，与 `interactionMode`（native/advanced）是两枚独立开关，不得合并。

---

## 3. 工作方法（核心，别跳）

### 3.1 用 grill-me 逐轮确认

读 `skill://grilling`。要点：

- 把设计当成一棵**决策树**，每个决定分出下游决定。
- 按**轮次**推进。每轮只问**前沿**问题——前置已定的那些。前置未定的问题留到后面的轮次。
- 每轮把所有前沿问题一次问完，**编号 + 给出你的推荐答案**，然后**停下等我回答**。
- 格式：
  ```
  ❓ **Q1** - **<问题标题>**：<问题正文，可以多段，可以给多选>

  ➡️ <你的推荐答案>
  ```
- **事实你自己查**（派 subagent 读代码/读参照），**决策问我**。不要问我你能自己查到的东西。
- 前沿空了才算谈完。**在我确认之前不要动手实现。**

### 3.2 每个界面先给浏览器示例图

我说的是「用浏览器做示例图给我演示」，具体做法：

1. 写一份 **HTML/CSS 静态示例图**，模拟 macOS 原生外观（窗口圆角、材质质感、系统字体、深浅色）。
   - 目的是让我**判断布局与样式**，不是做产品原型。
   - 要**像 macOS 原生**，不要像网页：用系统字体栈、正确的控件尺寸与间距、真实的 macOS 控件外观（segmented control、sidebar、toolbar、popover）。
2. 用 `browser` 工具（见 `xd://eval/browser`）打开它，**截图**，把图给我看。
3. 对着图问我：这个按钮放哪？这个尺寸对不对？这个信息该不该出现在这里？
4. 我指认 → 你改 → 再截图。**迭代到我说"就这样"。**
5. 一个界面确认完，再进下一个。**不要一次给我十个界面。**

示例图文件放 `.artifacts/native/mockups/`（该目录已被 `.gitignore` 忽略），不要污染仓库。

### 3.3 顺序

1. **先定全局骨架**：窗口结构、导航模型、信息架构、密度刻度（间距/字号/行高）、状态栏。这些定了，界面才有共同语言。
2. **再逐界面**：外壳 → 每个工作面 → 设置。
3. 每个界面走完 3.2 的循环再动 SwiftUI。

---

## 4. 要逐个过的界面

### 4.1 外壳（先做）

| 部件 | 现状 |
|---|---|
| 窗口 / 标题栏 | `Shell/TitleBar.swift`、`Design/WindowAppearance.swift` |
| 侧栏 | `Shell/ProjectSidebar.swift`（已分组：工作/项目/工具/系统） |
| 详情列标题 | `Shell/WorkspaceDetail.swift` 的 `DetailTitleBar` |
| 状态栏 | `Shell/StatusBar.swift`（24pt） |
| 命令面板 | `Shell/CommandPalette.swift`（⌘K） |

### 4.2 codeg 里有的（对齐它的做法）

| Aeroric 工作面 | codeg 对应 |
|---|---|
| 任务 `tasks` | to-dos / tasks（`src/components/tasks/`） |
| 文件 `files` | file tree（`aux-panel-file-tree-tab.tsx`） |
| Git `git` | changes / git log / diff / merge / stash（`aux-panel-git-*.tsx`、`src/components/diff/`） |
| 扩展 `extensions` | skills / MCP（`settings/skills`、`settings/mcp`） |
| Agent 工具 `agentTools` | agents / session controls（`settings/agents`、`session_details`） |
| IDE `ide` | 部分（codeg 有 browser / computer-use，没有独立 IDE 面） |

### 4.3 **codeg 里没有的**（用 grill-me 从零讨论布局）

这几个是重点——没有参照可抄，必须和我讨论：

| 工作面 | 内容 |
|---|---|
| **笔记 `notebook`（随手记）** | 文件夹树 / 笔记 / 回收站 / 同步状态 / RAG 搜索 |
| **设施 `infrastructure`（Docker）** | 容器列表 / 镜像 / 端口占用 / conda 环境 |
| **数据库 `database`** | 连接列表 / 表结构 / SQL 编辑器 / 结果网格 / Redis / Mongo |
| **SSH `connections`** | 连接列表 / 主机密钥确认 / SFTP 浏览 / 传输队列 |
| **远控 `remote`** | 设备列表 / 推送设置 / 配对 |
| **运维 `operations`** | 用量面板 / 诊断 / 通知中心 / 系统权限 |

### 4.4 设置

`Settings/SettingsRootView.swift`：通用 / Agents / 关于。codeg 的设置分 17 个分区，可参照它的**信息架构**（分组、侧栏式导航），但 Aeroric 的分区集合要和我定。

---

## 5. 起点状态（不是终点，可以推翻）

- 侧栏已分组：`工作`(任务) / `项目` / `工具`(文件·Git·IDE·数据库·笔记·Agent 工具) / `系统`(SSH·设施·扩展·远控·运维)
- 侧栏可折叠成 56pt 图标栏（⌃⌘S）
- 详情列有标题栏显示当前工作面
- 每个面板有空态（图标 + 标题 + 下一步提示）
- 状态栏 24pt，显示连接相位 + 进行中任务数
- 12 个工作面的内容都在 `macos/AeroricApp/Sources/TaskWorkspaceView.swift` 的 `sectionContent`

**这些布局决定都可以改。** 用户要的是重整，不是保留现状。

---

## 6. 交付要求

1. 每个界面确认后，用 SwiftUI 实现，放在 `AeroricKit/Sources/AeroricUI/` 对应子目录。
2. 保持既有分层：UI 不碰 wire 格式；app 侧（`AeroricApp`）负责接线。
3. **标识符只放叶子**（约束 6）。
4. 更新受影响的门禁 floor（`scripts/native/checks.json` 的 `runner.floors`）与 UI 测试。
5. 通过门禁：
   ```bash
   node scripts/native/check.mjs shell
   node scripts/native/check.mjs native-design
   node scripts/native/check.mjs swift-client
   node scripts/native/check.mjs accessibility   # 真窗口 UI 测试，13 例
   ```
6. **真窗口核对**：改完启动应用看实际效果（截图 + AX 树），不要只跑测试。

---

## 7. 实测陷阱（别重踩，都是真金白银换来的）

| 陷阱 | 现象 | 处理 |
|---|---|---|
| **LaunchServices 注册过期** | 重建 bundle 后直接启动 → **零窗口**，进程活着、主线程不卡、数据目录为空 | 先 `open` 一次刷新注册；`ui-test.mjs` 已内置预热 |
| **TCC 同意书** | 数据目录放 `~/Downloads` 内 → 弹「想访问下载文件夹」→ 宿主永远不就绪 | `AERORIC_DATA_DIR` 必须在 `~/Library/Caches/` 之类不受保护的位置 |
| **中文输入源** | XCUITest 合成不出 ASCII 逗号（`⌘,` 不触发） | 用菜单项驱动，或换 ASCII 输入源 |
| **点击需要命中点** | 别的应用窗口压上来 → `Unable to find hit point` | 跑 UI 测试时别占用屏幕；或改用键盘路径 |
| **容器标识符吞叶子** | 叶子查不到，AX 树里全读作容器 id | 标识符只放叶子（约束 6） |
| **布局结论要量不要估** | 估算得出的"会被裁切"实测不成立 | 用 `sizeToFit()` / AX 树实测，别算 |

---

## 8. 现在就开始

第一步：**读代码建立事实基础**（派 subagent 并行读：外壳三件套、`TaskWorkspaceView`、`Design/` 全部、codeg 的 `sidebar.tsx` + `status-bar.tsx` + `aux-panel.tsx` + `globals.css`）。

第二步：**给我第一轮 grill 问题**，主题是**全局骨架**（窗口结构 / 导航模型 / 密度刻度 / 状态栏），每个问题带推荐答案，并配**至少一张浏览器渲染的示例图**。

在我回答之前，不要写 SwiftUI。

---

