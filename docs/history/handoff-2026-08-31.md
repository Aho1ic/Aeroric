# Handoff — 代码优化与组件整合任务

> 交接时间:2026-08-31。写作时分支 `main` 上**没有任何 commit**,全部改动都在工作区;
> 这些改动已于 2026-09-02 分类提交到 `main`,所以下文凡说「在工作区」的,现在都指
> 「已提交」——正文保留原样,当作那天的快照读。
> 本文按「已验证 / 未验证 / 待做」严格分开写。凡是没有实际命令输出支撑的,都标了未验证。

## 0. 一句话现状

原始任务四条验收标准(无上帝结构、无冗余代码、无明显 bug、高覆盖率、实机/爆破/高并发)
**均未达成**,但计划 2 的**后端部分已经做完** —— 禁区外**再没有 3000 行以上的 Rust 文件**:
`dsh_webui.rs` 5429 → 3795、`app_settings.rs` 4678 → 3516、`dap.rs` 4222 → 3395、
`session.rs` 3973 → 3300、`local_router/server.rs` 3621 → 2834、`lsp.rs` 3733 → 2711。
六个文件共抽出 17 个子模块,每一批都逐行证明是纯搬动。
拆法与「证明是纯搬动」的做法都记在 §5.2,前端和后续几段照抄。
**下一站是前端**:`ProjectPage.tsx` 3096 行 / 35 useState 是计划 2 明确点名的目标,一行没动。
覆盖率阈值已从 60/55/60/60 抬到 **73/68/70/76**(实测 74.43/69.27/71.75/77.28,留 1 点余量)。
已完成的还有:计划 3(覆盖率)禁区外 12 个薄弱文件做完 11 个,
第 12 个 `App.tsx` 开了头(`remote-task-request` 这一片已 22 测试 / 24 变异全杀)
(每个文件都做了变异测试,存活的逐条定性),1 处真实 bug 修复、2 处重复代码合并,
以及上一轮(压缩前)的样式去重和一个 Rust 大文件拆分。
**前端上帝组件(`ProjectPage.tsx` / `App.tsx` / `FileViewer.tsx`)一个没动,
实机/爆破/高并发(计划 4)也一行没动。**

`App.tsx` 上新写的测试**刻意只钉边界**(渲染 `<App/>`,断言 IPC 命令名与事件契约),
不碰内部结构 —— 因为「先测后拆 vs 先拆后测」这一条至今没有用户确认(见 §2)。
边界级测试在两种顺序下都不会白写,所以在拿到确认前只往这个方向加。

## 1. 验收标准对照

| 标准 | 状态 | 说明 |
|---|---|---|
| 无上帝结构 | 🟡 部分 | **后端 3000 行以上的文件全部拆完(禁区外已归零)**:`dsh_webui.rs` 5429 → **3795**、`app_settings.rs` 4678 → **3516**、`dap.rs` 4222 → **3395**、`session.rs` 3973 → **3300**、`local_router/server.rs` 3621 → **2834**、`lsp.rs` 3733 → **2711**。十四批都逐行证明是纯搬动(行集合对账 + 属性归属 + 函数清点三项)。**前端 `ProjectPage.tsx` 3096/35 useState、`App.tsx` 2828/27、`FileViewer.tsx` 2601/43 一个没动** —— 现在最大的三个上帝结构全在前端 |
| 无冗余代码 | 🟡 部分 | 已合并:样式外壳(24 处重复)、文件后缀表(2 处重复)。另定性出 7 处冗余/够不着的代码(§4 第 5~9、11~13 条)但**未改**。全局重复扫描仍未做 |
| 无明显 bug | 🟡 部分 | 修掉 1 个(MCP 重名报错文案)。**新确认 1 个真 bug 未修**:React 19 下 `onBeforeInputCapture` 是死代码(§4 第 10 条,两处),修它要改运行时行为 + 真实输入法验证。另有 3 处不一致已固化在测试里但未改 |
| 单测全通过 + 高覆盖 | 🟡 部分 | 12 个薄弱文件做完 11,`App.tsx` 开了头(边界级)。全局基线(311 文件 / 4326 测试全绿)语句 74.43% / 分支 69.27% / 函数 71.75% / 行 77.28%。**阈值已抬到 73/68/70/76 并实跑验证通过**(留 ~1 点余量:两次同配置跑的分支数会差 0.01,卡死在实测值会偶发翻红)。**那条悬着的全量失败已定位**:含禁区跑 310 文件 / 4557 测试,唯一失败是 `notebook-panel.test.tsx > 跨文件重命名 > 改完重扫`(禁区文件,归属证据见 §7);排掉它就是 309 文件 / 4556 全绿 |
| 实机/爆破/高并发 | 🟡 后端已覆盖 | **五块完成,新增 40 测试(§5.3)**:`remote/auth_stress_tests.rs` 8(限流闸门并发正确性、一次性 invite 竞态、限流表有界、报错文案不构成枚举预言机)+ `remote-relay/rate_limit.rs` 11(单/多 IP 并发额度、表淘汰策略、`x-forwarded-for` 畸形与伪造)+ `local_router/circuit_breaker.rs` 6(半开探针并发独占、令牌三条归还路径、并发失败不丢记账)+ `local_router/mod.rs` 9(`RuntimeMetrics` 四计数器守恒与中间态可见性、监听代号的重启重叠)+ `local_router/server.rs` 6(**真起 loopback 服务发真请求**:64 路并发账目守恒、在途可见且 `/health` 不被卡、并发转移不丢账、并发失败全记、两层 `Drop` 守卫各自的断开路径)。**变异测试 55 个:50 杀、2 等价、3 冗余闸门(已定位承重点并说明为何保留)**。**先修正了本项的目标**:invite 与 device token 都是 256 位 CSPRNG,没有短码可爆,真正的风险在"猜不中那条路径"。**剩真实 GUI 交互**,在 vitest 范围外,需人工清单 |

## 2. 用户已确认的边界(必须遵守)

- **禁区**:`src/components/notebook/` 与 `src-tauri/src/notebook/` **全部**不许动;
  `src/test/notebook-panel.test.tsx`、`src/test/notebookVaultHarness.ts` 也不许动。
  这两处是另一个并行会话在改的。
- 只改实现,不动别人的测试文件;避开导出相关文件。
- 已落地的改动**保留**,继续往下做(用户明确选的)。
- 未经明确要求不 commit / push / 改写历史;只向 `main`。
- 新增用户可见文案必须同时补 `src/i18n/en.ts` 和 `zh.ts`。
- `pnpm` only;`pnpm lint` 必须零警告;禁止 `console.log`。
- 不许删 `tauri.conf.json` 的 `bundle.macOS`(ad-hoc 签名基线,删了系统权限会失效)。
- `App.tsx` 的处理顺序:**这一条至今没有用户确认。** 早前有一条助手消息声称用户选了
  「先测后拆」,但回溯全部 user 轮次找不到依据 —— 按未定处理,动 `App.tsx` 之前先问。
  已确认的只是「前端上帝组件总体走 refactor-first」这个方向。
  我的判断同样是先拆再测(2828 行/27 useState 现在堆测试会把结构锁死)。

## 3. 已验证的改动(有实际命令输出)

### 3.1 本轮(压缩后)

**新增 `src/test/branch-bar.test.tsx`** — 34 测试全绿。
`BranchBar.tsx` 覆盖率 **0% → 84.37% 行 / 70.37% 函数 / 86.45% 分支**。
变异测试 25 个,杀 22。3 个存活已在测试注释里写明原因:
- `handleSwitch` 的 `|| switching`:与按钮 `disabled` 属性重复,UI 点不到
- `handleCreate` 的 `if (!name) return`:与按钮 disabled + `handleKeyDown` 的 trim 判断三重重复
- `!q ||` 短路:`"".includes("")` 恒真,等价变异

**新增 `src/test/mcp-panel.test.tsx`** — 41 测试全绿。
`McpPanel.tsx` 覆盖率 **0.8% → 99.13% 行 / 100% 函数 / 96.29% 分支**。
变异两轮共 50 个,存活的只剩 1 个等价变异(`formatEnv` 的空对象判断)
和 1 对互相兜底的 `setTestResult(null)`(两处同时摘掉**会**被抓到,已验证)。

**修复真实 bug**:`McpPanel.handleDialogSave` 里 server 重名时报的是
`mcpServerNameRequired`(「服务器名称不能为空」),与实际情况不符。
新增 `appSettings.mcpServerNameDuplicate` 到 `en.ts` + `zh.ts`(带 `{name}` 插值),
并把 add / edit 两条重名分支收敛成一个 `collidesWithAnother` 判断。
`i18n-keys.test.ts` 通过。

**合并重复的文件后缀表**:
- 新增 `src/lib/fileExtensions.ts` —— `PREVIEWABLE_IMAGE_EXTENSIONS`(7 个)、
  `SQLITE_DATABASE_EXTENSIONS`(3 个)、`MARKDOWN_EXTENSIONS`(3 个)、`fileExtensionOf()`
  和三个 `has*Extension()`。放 `lib/` 而非塞进任一侧,避免 `file-viewer` → `file-explorer`
  的跨功能依赖。
- `file-viewer/editorUtils.ts` 三个判定函数改为调用共享表(-23/+? 净减)。
- `file-explorer/fileEntryUtils.ts` 的 `fileExtension` 改为 `fileExtensionOf` 的别名,
  `fileIconKind` 的图片表/sqlite 表、`isSqliteDatabaseFileName` 全部改用共享表。
- **先写后改**:`src/test/file-extension-predicates.test.ts` 94 测试,
  在合并**之前**把两侧当前行为逐条钉住(含 `fileExtension("x","")` 返回 `""`、
  名字叫 `png` 的无后缀文件被当图片这类历史行为)。合并后 99 测试全绿(含
  `file-explorer-sort.test.ts`)。
- 对共享表做了 8 个变异(增删后缀、去掉小写化、`??` 改 `||`),**8/8 全杀**,
  证明这张表现在两侧都受保护。

**新增 `src/test/dsh-question-dialog.test.tsx`** — 31 测试全绿。
`DshQuestionDialog.tsx` 覆盖率 **30.4% → 100% 行 / 96.66% 语句 / 92.64% 分支 / 100% 函数**。
重点在 RPC 契约那一半:提交/取消都必须回 `respond_dsh_server_request`,回不成功
必须留在原地报错 —— 直接关掉会让 DSH 对端永远等下去。
变异 12 个,杀 11。存活 1 个是互相兜底:遮罩 `onClick` 的 `!submitting` 与
`handleCancel` 开头的 `submitting` 判断,单摘任一道全绿,两道一起摘才被抓到(已验证)。
另外证明了 4 个分支**不可达**而非漏测:两处 `next.get(id) ?? {…}` 兜底(effect 已把
每个 question.id 预置好)、两处 handler 里的 `!request`(`if (!request) return null`
在前面就拦掉了)。手法是替换成 `throw` 后全绿,并做了反向对照(强制走 fallback 会挂
10 条),排除「throw 被 jsdom 吞掉」这种假证据。

**新增 `src/test/ssh-terminal-panel.test.tsx`** — 53 测试全绿。
`SshTerminalPanel.tsx` 覆盖率 **36.4% → 100% 行 / 98.28% 语句 / 93.26% 分支 / 100% 函数**。
重点两块:host key 闸门(未登记主机在用户确认指纹**之前**一个 shell 都不能开)、
孤儿 shell 回收(重连先杀旧的、删连接连带杀、Disconnect、卸载 dispose、
50ms 内卸载不开 shell)。
子组件(连接列表/连接对话框/host key 对话框)各自已有测试文件,这里打桩成只暴露
回调的壳,守的是"面板在什么时机调了什么"。
变异 15 个,杀 12。存活 3 个已定性:1 个是 `clearTimeout` 与回调里 `if (cleaned)`
互相兜底(两道一起摘会被抓到,已验证),另 2 个见 §4 第 7 条。
`runInitTimer` 必须先冲 microtask 再推时钟 —— `startSession` 在
`check_ssh_host_key` 的 `.then()` 里,先推时钟会推到还不存在的定时器上。

**新增 `src/test/dsh-slash-palette.test.tsx`** — 57 测试全绿。
`DshSlashPalette.tsx` 覆盖率 **54.2% → 100% 行/语句/函数 / 95.32% 分支**。
守三件事:远端目录拉取失败或返回空必须回落到静态目录(不能变空面板)、
popup 类命令不能直接插入(先选参数)、`editorInsert` 返回 false 时不能关面板
(否则用户刚敲的东西原地消失)。
二级选择器的 document 级 capture 监听单独一组:焦点判定(class / keyboardTargetRef)、
输入法组合中不接管、候选未到或为空时不接管、卸载后摘监听。
变异 17 个,杀 17(其中 2 个是先补测试才杀掉的)。

**两个「测试写法本身让断言失效」的坑**(比结论更值得记):
- `disposed` 闸门:原先按「卸载后 setState 不告警」写,React 19 本来就不告警,
  是空断言。改成「换 sessionId 后旧响应晚到不能覆盖新的」才有可观测差异。
  但第一版仍杀不掉 —— 两次 resolve 之间夹了 `waitFor`,覆盖在随后的两个
  microtask 里落不下来,断言提前跑完。改成两段都用 `act` 才稳定观测到。
  过程用一次性探针文件确认了「同样的逻辑,probe 挂而用例过」,才定位到是写法问题。
- 手工塞进 `document.body` 的节点(模拟 PromptEditor)Testing Library 不回收,
  局外那个 `<input>` 活到后面的用例里把 `getByRole("textbox")` 撞成多个。
  已加 `strayNodes` + `afterEach` 统一收。

**新增 `src/test/ssh-workspace.test.tsx`** — 43 测试全绿。
`SshWorkspace.tsx` 覆盖率 **54.7% → 100% 行 / 100% 函数 / 97.11% 语句 / 91.66% 分支**。
守分组归桶(命名分组按名单顺序在前、空分组占位、未分组落末尾、分组名 trim、
纯空白等于没分组)与「卡片 / 终端」状态机(选中项从名单里消失要回落到卡片)。
`SshTerminalPanel` 打桩成只报告入参的壳(它自己有 53 条),`SshConnectionDialog` 同理。

**又一个 jsdom 手法失效的实例(重要)**:想证明 `deleteConnection` 里
`setShowCards(true)` 是恒等操作时,「换成 throw 后测试仍全绿」这个手法**完全无效**
——jsdom 吞掉事件处理器里的异常,连**无条件** throw 都是 43 条全绿。
改用计数器埋点才拿到真数据:该分支被走到 2 次,其中 `showCards === false` 的次数为 0。
以后判「够不着 / 恒等」时,只要目标在事件处理器里,就别用 throw,用可观测的埋点。

**新增 `src/test/prompt-editor.test.tsx`** — 175 测试全绿。
`PromptEditor.tsx` 覆盖率 **38.0% → 95.96% 行 / 95.46% 语句 / 92.74% 分支 / 100% 函数**。
未覆盖的 671-690 是死代码,见 §4 第 10 条。组件本身与 HEAD 逐字节一致(未改一行)。
守的是「用户刚敲的 prompt 不能丢、不能发错内容」:序列化(chip → `@path`、`<br>` → 换行、
nbsp 还原、ZWSP 剔除、跨项目退化成绝对路径)、触发词识别(`/` 只在行首、`@` 前须空白、
`indexOf` vs `lastIndexOf`)、chip 邻接删除(整块删而不是删半个)、
提交/换行的平台矩阵与组合中不提交。
`APP_PLATFORM` 在模块初始化时按 navigator 定死,jsdom 里算成 `"other"` ——
本文件 mock 成 `macos` 取一个确定平台,平台矩阵本身由 `send-shortcut.test.ts` 覆盖。
变异 ~55 个,杀 51,4 个记为等价/互相兜底(`parts.length > 0`、尾随空格的
`nodeType`、撤销守卫、`item.kind` 取反),原因逐条写在测试注释里。

**三个「jsdom 让断言失效」的坑(本文件实测,与 §7 已有条目互补)**:
- **jsdom 会吞掉事件处理器抛的异常**,转成 window 的 `error` 事件。于是「守卫生效、
  什么都没做」和「守卫失效、读了 `undefined.name` 崩掉」在 `expect(spy).not.toHaveBeenCalled()`
  上完全同构 —— 三个越界守卫的变异**因此全部存活**。已加 `watchErrors()` helper,
  凡断言「什么都没发生」的用例同时断言 `errors` 为空,三个变异随即全杀。
- **`focus()` 会折叠选区。** jsdom 对「还没聚焦」的 contenteditable 调 `focus()` 把选区
  折叠到 `(element, 0)`(真实浏览器同理),而 `insertSkill` / `insertText` 内部都会调
  `editor.focus()` —— 测试摆好的光标被冲掉,插入位置全跑到最前面(4 条用例同时挂)。
  解法:`setup()` 里先 `focus()` 一次,之后它就是空操作,这也正是真实场景
  (用户在已聚焦的编辑器里打字)。
- **jsdom 没实现 `contentEditable` 的 IDL 反射**,`el.contentEditable = "true"` 只落在普通
  JS 属性上、不写特性 —— 这样的元素在 jsdom 里不可聚焦,`closest("[contenteditable]")`
  也找不到它。**第一版探针正是因此给出了误导结论**(报 `rangeCount=0 active=BODY`)。
  用 `setAttribute` 才对。

**新增 `src/test/file-explorer-fs-actions.test.tsx`** — 126 测试全绿
(与既有的 `file-explorer-ui.test.tsx` 合跑 132)。
`FileExplorer.tsx` 覆盖率 **46.5% → 99.8% 行 / 96.23% 语句 / 91.45% 分支 / 100% 函数**。
唯一未覆盖的 433 行在文件末尾的不可达清单里给了理由。组件未改一行。
另起一个文件是因为 `file-explorer-ui.test.tsx` 是既有文件(图标/排序/面包屑/远程超时),
这里补它没覆盖的那一半:**删除 / 重命名 / 新建 / 粘贴**——直接改真实文件系统且没有撤销,
所以断言的重点是「对哪个路径调了哪个后端命令,以及什么情况下绝对不能调」。
关键设计是一棵**可变的虚拟目录树**(`Map<目录, 条目[]>`)做真实读目录回放,
否则「删完树里确实没了」「选中项回退」这类跨越一次刷新的行为根本断言不到。
变异 81 个(4 批 + 3 次重跑 + 1 次反向对照 + 2 个跨文件 ContextMenu 变体 + 收尾 8 个),
杀 77。存活 4 个逐条定性:2 个面包屑守卫如期存活(有 `disabled` / 上游推导兜底,
理由写在文件末尾的不可达清单里)、1 个无 path 的剪贴板项等价变异、1 个 `endIdx` 的
`slice` 自带钳位等价变异。

**这个文件贡献的两个新坑(§7 已收):派生状态钉不住它派生自的状态、活着的轮询会替你的
代码通过测试。** 另外 `sortTreeNodes` 的**递归排序**只在「重读失败」这一个窗口里
独自生效 —— 读成功时 refresh 每层都会自己 `sortFileEntries` 一遍,顺序对不对全归它。
所以那条用例是按「读目录失败时切换排序,已加载的树仍然就地重排」写的,
不是按「展开的目录切排序」写的(后者被 refresh 兜住,变异全绿)。

**新增 `src/test/app-remote-task-request.test.tsx`** — 22 测试全绿,零 act 警告。
守的是 `App.tsx:949-1118` 那个 `remote-task-request` 处理器,也就是手机端
`task.create` / `task.resume` 的桌面这一半。**变异 24 个,全部杀掉,零存活。**
`App.tsx` 本身未改一行(跑完按 sha 核对复原)。

为什么单独立文件:后端 RPC 校验完就把请求转给前端(`src-tauri/src/remote/tasks_rpc.rs`),
手机那边**阻塞等** `remote_complete_task_request`。这个 handler 有 8 条拒绝路径,
任何一条漏应答手机就永远转圈,而桌面上完全看不出来。
`app-event-wiring.test.tsx` 只断言了这条订阅**存在**,handler 体一行没跑过。
除「接受/拒绝」外还钉了两条顺序契约(App.tsx 里两处注释点名的):
接受时必须**先落盘再应答**(否则手机紧跟的 `tasks.list` 读到旧文件)、
应答里必须带任务快照(手机才能立刻渲染,不等下一次轮询)。

三个写这个文件时踩到的点:
- **`ssh://` 路径串不代表远程项目。** `resolveProjectLocation(project)` 只做
  `project.location ?? { kind: "local", path }` —— 只把 path 写成 `ssh://…`,项目
  仍然是本地的,SSH 那三条校验一条都走不到(2 条用例因此拿到错误的报错串)。
  测试里用专门的 `sshProject()` helper 显式设 `location`。
- **不等启动链路读完项目,错的不是「报错」而是「换一条错误路径」。** `create` 请求会
  一律拿到 "Project not found on the desktop",因为 `remoteRequestRef.current.projects`
  还空着。`get_active_task_ids` 是 `init()` 最后一次 invoke,它之后还差一次
  setState + 渲染,所以 `waitForBoot()` 要在 waitFor 后再补一个 `await act`。
- **`approval: undefined` 那半行断言不了,也不该断言。** 前端 `Task` 类型没有
  `approval`,`load_project_tasks` 反序列化到不含该字段的 Rust struct 时 serde 直接丢掉;
  手机看到的 `approval` 是 `remote/rpc.rs:161-171` 从 `RemoteState.approvals` 现场补的。
  要断言就得让 mock 交付一个真实后端产不出的键 —— 那条断言无论 handler 怎么改都会绿。
  我写过一版这样的断言,靠「这个字段真的能到前端吗」这一问才发现是空的
  (顺带被 `tsc` 拦下:`Property 'approval' does not exist on type 'Task'`)。

另外删掉了一个自己写的假用例:标题说「桌面侧创建失败(返回 null)时应答拒绝」,
断言里却是 `accepted === true`。查过 `!createdTask` 这条在本 handler 里够不着 ——
handler 已经用 `handleSubmitTask` 的同一个数据源预检了 SSH 连接,而
`launchMode` 硬编码 `"local"`、`immediate: true`、附件为空,`handleSubmitTask`
返回 null 的 5 个条件一个都触发不到。换成了一条真的 WSL 用例。

**拆 `dsh_webui.rs`:5429 → 3795 行**(计划 2 的第一个战果)。抽出三个子模块,
三批都用脚本按行号切、搬完逐行校验证明是**纯代码搬动**:

| 新文件 | 行数 | 内容 |
|---|---|---|
| `dsh_webui/startup.rs` | 602 | 拉起/复用 `dsh web` 进程:拼参数、抽干 stdout/stderr 并从里面认出启动 URL、健康检查、lifecycle 锁排队。连 5 个用例一起搬 |
| `dsh_webui/commands.rs` | 767 | 52 个薄命令壳(取 client → 转发一次 → 返回)+ 两个取 client 的 helper |
| `dsh_webui/dto.rs` | 318 | 25 个传输结构 + 3 个纯函数。原来它们夹在 `impl DshApiClient` 的两半之间 |

验证(全部有实际输出):`cargo build --lib` exit 0;`cargo fmt --check` exit 0;
`cargo clippy --all-targets` **exit 0**(仅剩 5 个警告,全在 `notebook/export.rs`,属禁区);
`cargo test --lib dsh` **137 通过 0 失败**。

**证明是纯搬动的做法**(值得照抄):写个脚本把子模块每一行拿去和搬前的备份逐字比对,
只放过四类必要胶水 —— 模块头注释、`use super::*;`、`pub(super)` 前缀、
以及 `cargo fmt` 因为加了前缀超过 100 列而换行的签名。三批下来「无法解释的行」
分别是 3 / 0 / 0 行,全部是第四类。反向也比一遍:备份里的行现在必须能在父子文件之一里找到。

**两个踩到的点**:
- `pub(super) struct` 不等于字段也 `pub(super)`。同模块内字段私有照样能访问,
  搬出去之后编译器才报出来(`DshSettingsDescription` 的两个字段)。
- 切两段都以 `impl DshApiClient {` 收尾时,`next(...)` 取到的是**全文第一个** ——
  两段拿到同一个终点。要限定 `i > start`。这个 bug 被脚本自己的首尾断言拦下了。

**拆 `local_router/server.rs`:3621 → 2834 行**(计划 2 的第二个战果,已破 3000)。
按 §5.2 量好的边界抽出三个子模块,放在 `src/local_router/server/` 下:

| 新文件 | 行数 | 内容 |
|---|---|---|
| `server/routing.rs` | 259 | 纯映射:认 agent、剥路径前缀、拼上游 URL(`SelectedRoute` + `select_route` + `build_upstream_url` 等 6 项)+ 3 个用例 |
| `server/guard.rs` | 247 | 进门安全闸门:常量时间比密钥、跨站识别、剥掉路由自己的凭据、hop-by-hop 头清理(10 项)+ 6 个用例 |
| `server/semantic.rs` | 340 | 上游回 200 却在 SSE 正文里吐错误的识别(两家协议各一套形状)+ `SemanticStreamObserver`(7 项)+ 5 个用例 |

验证(全部有实际输出):`cargo fmt --check` exit 0;`cargo build --lib` exit 0;
`cargo clippy --all-targets` exit 0(仍只剩 `notebook/export.rs` 那 5 个,属禁区);
`cargo test --lib local_router` **91 通过 0 失败**(搬测试前后都是 91,一个不多一个不少)。

逐行校验同上一套:112 行差异全部配平 —— 31 行是同一批声明加了 `pub(super) ` 前缀
(routing 10 / guard 10 / semantic 11,一一对应),1→2 行是下面那条 `super::` 路径修正,
其余 49 行是模块 wiring(13)、测试块脚手架(9 + 6 个 `use super::*;` + 3 条 import)
和模块头注释(16)。`#[derive]` 归属 **19/19 与备份一致**。

**这一批新踩到的三个点**:
- **`use super::*;` 救不了移动后代码里显式写的 `super::` 路径。** 搬进
  `server/guard.rs` 之后 `super` 从 `local_router` 变成了 `server`,
  `super::validate_listen_address` 静默指错一层。改成 `crate::` 绝对路径。
  这次编译器报出来了,只因为新父模块里恰好没有同名函数 —— 有的话就是静默改语义。
- **按行号切会把上一行的 `#[derive(...)]` 落在原地。** `SelectedRoute` 的
  `#[derive(Clone, Debug, Eq, PartialEq)]` 在切点上方一行,于是留在 `server.rs` 里
  贴到了**下一个** item(`RequestMetadata` 因此有两个 derive,`SelectedRoute` 丢了全部四个)。
  **这样能编译,91 个测试也全绿**,只有 `cargo fmt --check` 报出来。校验脚本现在单独
  核对 derive 归属这一项。切之前先看切点上方一行是不是空行。
- **搬测试会让原文件的 import 变多余。** 搬走 6 个 guard 用例后,`server.rs` 测试块里的
  `semantic_error_from_value` 和我给 `guard.rs` 猜多了的 `RouterAgentRuntime` 双双变成
  unused import(clippy 抓的)。顺带 `semantic_error_from_value` 的 `pub(super)` 也可以收回 ——
  它唯一的外部调用者就是那个搬走的用例,收回后签名恰好回到备份里那一行的原样。

一个用例**故意留在 `server.rs`**:`non_loopback_requests_require_the_dedicated_router_token`
用了测试 helper `runtime_with_target`,而那个 helper 还有 3 个集成用例在用。
搬它就得复制 helper,不划算。

**拆 `app_settings.rs`:4678 → 3516 行**(计划 2 的第三个战果)。这个文件本来就有 5 个子模块
(`agent_scripts` / `config_bundles` / `launch_spec` / `models` / `versions`),这轮再加 4 个,
分两批做:

| 新文件 | 行数 | 原行号 | 内容 |
|---|---|---|---|
| `app_settings/normalize.rs` | 349 | 699-1023 | 设置值归一化,全是纯函数(不读盘、不发请求、不碰全局缓存)。含安全相关的 `sanitize_custom_agent_id`(id 要拼进 `~/.aeroric/agent-homes/{id}`,必须拒路径穿越) |
| `app_settings/agent_env.rs` | 311 | 1025-1325 | 拼 Agent 启动环境变量(`append_*_env` 四个)+ 两处旧配置迁移 |
| `app_settings/proxy_test.rs` | 343 | 1137-1265 | 代理连通性测试,连 5 个用例和 `spawn_fake_proxy` helper 一起搬 |
| `app_settings/model_detect.rs` | 248 | 1841-2075 | 模型探测的网络部分:客户端构造、候选链、地址级故障转移 |

验证:`cargo build --lib` exit 0;`cargo fmt --check` exit 0;`cargo clippy --all-targets` exit 0
(非禁区**零告警**);`cargo test --lib` **1881 通过 0 失败**,`cargo test --lib app_settings`
搬前搬后都是 **138**。逐行对账:154 行差异全部配平 —— 36 行是加 `pub(super) ` 前缀
(一一对应),12 行是 fmt 因前缀超 100 列的换行,其余是模块 wiring、测试块脚手架和模块头注释。
**属性归属 93/93 与备份一致**(含 `#[cfg(windows)] expand_windows_env_vars` 带着门控一起走)、
**函数清点 191 → 191**。

**这批新踩到的两个可见性坑**(都归到「搬动不改可见性等级」这一条):

- **`pub fn` 搬进私有子模块后对外不可达。** 脚本只给 `fn ` 开头的加 `pub(super)`,
  `pub fn custom_agent_home` 原样搬走,于是 6 个模块的 `app_settings::custom_agent_home`
  全部 E0603。glob import(`use normalize::*;`)只把名字拉进父模块作用域,**不替父模块
  对外转发可见性**。要 `pub use normalize::custom_agent_home;` 原样转发。
  同类的还有两个 `pub(crate)`(`configured_agent_path` / `normalize_local_router_settings_for_update`,
  调用点在 `agent_tools.rs` 和 `local_router_commands.rs`)。
- **`#[tauri::command]` 按名字 re-export 带不走它生成的隐藏宏。**
  `pub use proxy_test::{test_proxy_connection, ProxyTestResult};` 编译报
  `macro import __cmd__test_proxy_connection is private` —— 那个宏还生成
  `__cmd__<名字>` 和 `__tauri_command_name_<名字>`,`generate_handler!` 要用。
  **必须用 glob** `pub use proxy_test::*;`。`dsh_webui` 那批 52 个命令一开始就写的是
  `pub use commands::*;`,所以没暴露出来。

**一个刻意不做的决定:`app_settings` 的测试块基本不用搬。** 24 个搬走的纯函数里,
测试块中**只有 1 个**有直接调用(`normalize_local_router_settings`),其余全是经
`load_settings` / `get_agent_launch_spec` 间接覆盖的集成用例 —— 归属在父模块才对。
只有 `proxy_test` 那 5 个是真单元测试,所以只搬了它们。
这和 `server.rs` 不一样(那边测试块里有成片针对被抽出纯函数的单元测试)。

> 顺带一个**覆盖率发现**(未修):`normalize.rs` / `agent_env.rs` 里 24 个纯函数只有 1 个
> 有直接单元测试。这些函数是「用户填什么 → 洗成什么」的唯一关口,又全是无副作用的纯函数,
> 属于最好写测试的一类。计划 3 若要继续,这里是禁区外性价比最高的一片。

**拆 `dap.rs`:4222 → 3395 行**(计划 2 的第四个战果)。原有 `dap/protocol.rs`,这轮加 2 个,
分两批(先实现再测试):

| 新文件 | 行数 | 原行号 | 内容 |
|---|---|---|---|
| `dap/paths.rs` | 611 | 308-611 | 调试配置的路径解析/校验/读写。**全是安全边界**:用户填的 program / cwd / 断点路径要钉死在项目根内,本地一套(`ensure_path_inside_root` 走词法归一)、远程一套(纯字符串判断,拿不到远端 fs)。连 11 个用例一起搬 |
| `dap/remote.rs` | 273 | 613-775 | 通过 SSH 在远端跑调试命令,4 个 `build_*_command` 拼命令串(每个插值都要先引号转义)。连 3 个用例一起搬 |

验证:`cargo build --lib` exit 0;`rustfmt --check`(我的 3 个文件)exit 0;
`cargo clippy --all-targets` exit 0(非禁区零告警);`cargo test --lib` **1904 通过 0 失败**,
`cargo test --lib dap` 搬前搬后都是 **33**。逐行对账:91 行差异全部配平 —— 27 行加
`pub(super) ` 前缀、11 行是 fmt 因前缀超 100 列的换行、其余是 wiring / 测试块脚手架 / 模块头注释。
**属性归属 59/59**、**函数清点 126 → 126**。

**这批的两个处理**:
- 段内**没有任何 `pub` / `pub(crate)` 项**(脚本加了断言),所以父模块只要 `use paths::*;` /
  `use remote::*;`,不需要 re-export。这是最省事的一种边界,值得优先挑这样的段。
- **共享测试 helper 用「提到模块层 + `#[cfg(test)]`」解决,不复制。** `unique_test_dir` 有
  3 个路径用例和 2 个留在父模块的断点解析用例都在用。搬进 `paths.rs` 时提到模块层写成
  `#[cfg(test)] pub(super) fn`,父模块测试块 `use super::paths::unique_test_dir;` 取回 ——
  和本仓库 `app_settings.rs` 对 `ensure_user_agent_script_executable` 的做法一致,一份定义。
  校验脚本会把这个报成「备份里没属性、现在有 `#[cfg(test)]`」,那是**必需的**改动(原来它在
  `#[cfg(test)] mod tests` 里面,提到模块层就得自带门控),不是缺陷。
- 搬走用例后父模块测试块的 `PathBuf` 和 `SystemTime`/`UNIX_EPOCH` 变成 unused import
  (clippy 抓的)—— 每次搬测试都要复查一遍原文件的 import。

**拆 `session.rs`:3973 → 3300 行**(计划 2 的第五个战果)。原有 `session/export.rs`,这轮加 2 个,
一批做完(实现 + 单元测试一起):

| 新文件 | 行数 | 原行号 | 内容 |
|---|---|---|---|
| `session/approval.rs` | 228 | 509-648 | 判断一次工具调用**要不要用户确认**。`tool_call_requires_confirmation` / `exec_command_requires_confirmation` / `looks_like_read_only_command` / `contains_shell_redirection` / `is_read_only_segment` / `apply_patch_requires_confirmation` / `extract_patch_path` / `patch_target_requires_confirmation` / `assistant_message_requests_user_input`。**判错的方向不对称**:漏判(该确认的没确认)会让 agent 直接写盘,所以这一段是安全边界。连它自己的 `// ── 权限判断 ──` 段头和 5 个用例一起搬 |
| `session/parse.rs` | 487 | 1366-1838 | JSONL → `SessionMessage` 的解析层,两家 agent 各一套形状:`parse_session_lines` / `parse_session_line` / `parse_claude_session(_line)` / `claude_user_content` / `claude_assistant_blocks` / `json_value_to_display` / `attachment_from_value` / `append_assistant_content` / `append_codex_user_message` / `parse_codex_session(_line)` |

验证:`rustfmt --check`(我的 3 个文件)exit 0;`cargo build --lib` exit 0;
`cargo clippy --all-targets` exit 0(非禁区零告警);`cargo test --lib session` 搬前搬后都是 **80**;
排掉禁区的全量 `cargo test --lib -- --skip notebook` **1179 通过 0 失败**。
逐行对账:**属性归属 61/61**、**函数清点 143 → 143**,且 `pub(crate) fn parse_session_lines`
在「丢失」和「新增」两张清单里都不出现 —— 可见性等级一字未改。

**这批的两个点**:
- **边界陷阱两次都在切点上方一行,而且第二次是文档注释形态。** 第一段的下界原本按 651 算,
  但 650 是**下一段的段头**(`// ── Claude Code 会话监视器 ──`);第二段按 1843 算,
  而 1842-1843 是 1844 行 `MAX_SESSION_BYTES_FOR_SUMMARY` 的 `///` 文档注释。
  后者就是 `server.rs` 那个 `#[derive]` 孤儿陷阱的**文档注释版本** —— 一样能编译、一样全绿,
  `cargo fmt --check` 连这个都不报(rustfmt 不管文档注释贴给了谁),
  只有逐行对账能看出来。改成 648 / 1838 之后才切。**切之前把切点上下各看三行。**
- **`parse.rs` 那 473 行纯解析零直接单元测试。** 搬动本身不改这一点(见下面的覆盖率发现),
  但它决定了「只搬真单元测试」这条规矩下这批只搬了 5 个用例(全是 `approval.rs` 的)。

> **覆盖率发现(未修)**:`session/parse.rs` 487 行、11 个函数,**没有一个有直接单元测试**,
> 全靠 `cargo test --lib session` 里读真实 JSONL 的集成用例间接覆盖。
> 它是「磁盘上的 JSONL → 用户在会话视图里看到的东西」唯一的一道转换,
> 输入完全来自外部进程(agent 自己写的文件),形状变了就静默显示错内容。
> 又全是纯函数(输入 `&str` / `&Value`,输出 `SessionMessage`),属于最好写测试的一类。
> 和 `normalize.rs` / `agent_env.rs` 那 24 个函数一样,是计划 3 在禁区外性价比最高的几片。

**动 `ProjectPage.tsx`:3096 → 2999 行(前端第一刀)。** 行数只降了 97,但这一批的价值主要在
**去重**,不在行数 —— 消掉了两处真实的重复实现,并把 3 段模块层代码搬出去变成可直接测的模块。

**① 七个「跳到某个文件的某一行」的入口收敛成一个基底 + 三个薄适配器。**
`handleSearchFileSelect` / `handleTextSearchMatchOpen` / `handleDiagnosticOpen` /
`handleTestFailureOpen` / `handleDebugLocationOpen` / `handleDefinitionOpen` /
`handleGitAdvancedFileOpen` 七个 `useCallback` 做的是同样四步(收起两个终端 → 选中文件 →
亮起 files 面板),其中**四个逐字节相同**,另三个只是先把各自的载荷拆成
`(path, name, selection)`。现在是一个 `openFileAtLocation` + 三个适配器,78 行 → 42 行。
**合并函数标识是安全的**:七个原本的依赖数组完全相同(`[handleFileSelect, openRightPanel]`),
所以标识变化的时机一模一样,对 `React.memo` 子组件只会更稳。

**② `escapeDraftHtml` 是 `syntaxHighlight.ts` 的 `escapeHtml` 的逐字节复制(只差 `export`)。**
已逐行比对确认(去掉函数名后 16 行全同)。改成直接引 `escapeHtml` ——
`syntaxHighlight.ts` 顶层只有一条会被擦除的 `import type`,shiki 全走动态 `import()`,
所以不会把高亮器拖进 `ProjectPage` 的模块图;`notebook/noteRender.ts` 等两处本来就这么引。
(第三份 `escapeHtmlText` 在 `notebook/noteExportHtml.ts`,属禁区,没动。)

**③ 三段模块层代码搬进 `project-page/`,搬完补了直接用例。**

| 新文件 | 行数 | 内容 |
|---|---|---|
| `project-page/auxiliaryLayout.ts` | 35 | `AUXILIARY_LAYOUT_STORAGE_PREFIX` + `AuxiliaryLayouts` + `readAuxiliaryLayouts` |
| `project-page/lspDiagnostics.ts` | 35 | `LspDiagnosticsEvent` + `mergeLspDiagnostics` |
| `project-page/AuxiliaryLayoutToggle.tsx` | 34 | 分屏⇄全屏切换按钮(本来就是独立组件,只是写在 3000 行文件顶部) |

这三段原先只能靠渲染整个页面间接覆盖。搬出来之后**新增 `src/test/project-page-helpers.test.ts`
14 测试**,专打渲染整页时很难构造的坏数据路径:localStorage 里半坏的 JSON
(`{"ssh":"full","file":123}` 只让坏字段回落)、`getItem` 抛 SecurityError、
`mergeLspDiagnostics` 那个 `||` 的两半(同文件的 eslint/tsc 条目不能被一次 LSP publish 清掉)。

**④ 新增共享 helper `src/lib/filePath.ts` 的 `fileNameFromPath`,替掉 5 处逐字节相同的写法**
(`ProjectPage` ×2、`TestExplorerPanel`、`ProblemsPanel`、`lspReferences`、`lspNavigation`、
`databaseViewModel:754`)。**新增 `src/test/file-path.test.ts` 12 测试。**

> **这里刻意只统一了三种写法里的一种。** `path.split(/[\\/]/)` 在仓库里有 16 处,
> 非禁区 10 处,分三组语义,差异只在边界上显现:
>
> | 写法 | 站点数 | `"/a/b/"` 的结果 |
> |---|---|---|
> | `.pop() ?? path` | 5 | `""` |
> | `.filter(Boolean).pop() ?? path` | 2(`runConfigState` / `debugState`) | `"b"` |
> | `.pop() \|\| path`(或自带兜底字面量) | 2(`CommandPalette` / `databaseViewModel:694`) | 原串 |
>
> 只合并了第一组 —— 它们的路径都来自后端给的诊断 / 测试失败 / LSP location,一律是具体文件。
> 后两组是**有意的**:`.filter(Boolean)` 要容忍用户手填的尾随斜杠,`||` 要让空串落到兜底上。
> 三组的差异已在 `file-path.test.ts` 后半逐条钉住,谁想再合并一步会先看到那几条挂掉。
> `git-diff/parse.ts:113` 还有个 `fileName` 只 `split("/")`(不认反斜杠),没并进来。

**验证**:`tsc --noEmit` 干净;`eslint src --max-warnings 0` 干净;
`prettier --check "src/**/*.{ts,tsx}"` 干净;
**全量 `vitest run --exclude notebook-panel.test.tsx`:313 文件 / 4352 测试全通过**
(此前基线 311 / 4326,新增的正好是我这两个文件的 26 条)。
**变异测试 13 个,全部杀掉,零存活** —— 9 个打 `mergeLspDiagnostics` / `readAuxiliaryLayouts`
(拆掉 `||` 的任一半、`&&` 互换、前缀少冒号、顺序颠倒、不逐字段校验、不按项目 id 隔离、
坏 JSON 回落方向),4 个打 `fileNameFromPath`(只认斜杠、加 `filter(Boolean)`、`??`→`||`、
`pop`→`shift`)。

**这一批踩到的两个点**:
- **脚本插 import 不能用「最后一条以 `import ` 开头的行」定位。** 多行 import 的首行
  (`import {`)也满足这个条件,于是 3 个文件的 import 被插进了别人的花括号里,
  报 `TS1003: Identifier expected`。正确锚点是那条 import 真正的收尾 `} from "...";`。
- **变异模式撞上文件头注释。** `filePath.ts` 的文件头把三种写法都写出来做对照,于是
  `path.split(/[\\/]/).pop() ?? path` 在全文命中 **2 次**,runner 的「恰好一次」断言直接
  拦下(报 BAD-PATTERN 而不是假「存活」)。改成带 `  return ` 前缀和 `\n}` 后缀锚到函数体。
  **这正是那条约束存在的理由** —— 见 [[mutation-perl-pattern-must-be-anchored]]。

> 顺带写了个跨文件的变异 runner `/tmp/mutmulti.py`:每个变异自带 `file`,一轮能同时打多个
> 源文件(这轮同时改了 `lspDiagnostics.ts` / `auxiliaryLayout.ts` / `filePath.ts` 三个)。
> 三条硬约束都在里面:先断言基线全绿、替换前断言命中恰好一次、汇总行缺失判 ERROR 不判存活。
> specs 以 list 传 `subprocess`,不经 shell —— zsh 不分词会把多个路径当成一个参数。

**⑤ 新增 `src/test/project-page-terminal-mount.test.tsx` 11 测试** —— 给 `ProjectPage.tsx`
2544-2666 那三个终端块**先补测试再抽组件**。

> **这里的顺序是被实测推翻后才定的。** 我原本把这三块排成「最接近纯搬运、先抽」,
> 理由是它们是三段互斥的条件渲染。真去 `coverage/coverage-final.json` 里按行号取覆盖率,
> 结论正好相反:
>
> | 块 | 行号 | 语句数 | 命中 |
> |---|---|---|---|
> | 本地终端 | 2544-2585 | 11 | **0** |
> | SSH 终端 | 2586-2614 | 0 | 0 |
> | WSL 终端 | 2615-2639 | 0 | 0 |
> | 文件预览 | 2640-2666 | 3 | **0** |
> | 右侧面板 | 2697-2938 | 12 | 1(8%) |
>
> `ProjectPage.tsx` 整体 66.51% 行 / **51.56% 函数** —— 45 个 `useCallback` 里约一半从没跑过。
> 抽一段零覆盖的代码,搬对搬错都没有信号,所以这一片翻成 test-first。

断言写在**「挂了哪个面板」这一层,而不是面板内部长什么样**:这三块接下来要被抽成子组件
(§5.4),抽的时候条件表达式会原样搬走,这些断言不用重写。

**这个文件本身踩了两个坑,都和「断言没出现」有关:**

- **11 条里 8 条是 `queryByTestId(...) === null`,而这个断言在「守卫正确地没渲染」和
  「渲染中途炸了所以什么都没有」两种情况下同样通过。** 先用一次性探针验证了
  SSH 面板在本 harness 下**确实挂得起来**(带匹配连接时 `data-connection-id="conn-1"`),
  然后把那条正例**收进正式文件当对照组** —— 靠一个已删除的探针来保证「不挂」有区分度
  是不成立的。见 [[jsdom-swallows-listener-exceptions]]。
- **整个 describe 挂了 window `error` 哨兵**(`beforeEach` 装 + `afterEach` 断言空数组),
  放在钩子里而不是每条用例自己调的 helper,是为了以后加用例不会漏。
  **哨兵自己也验过**:另写探针在 effect 注册的监听器里抛异常,DOM 断言照样绿,
  而哨兵抓到了 —— 空断言的场景被完整复现了一遍。
  (effect 直接抛的那种 React 19 会捅回 `render()`,是响的,不是这个哨兵管的静默情况。)

mock 用 `importOriginal` + 展开只替换组件本身,不手写整份:`ProjectPage` 还从
`ShellTerminalPanel` 那个模块引了 `deriveShellTerminalFontSize` 和
`SHELL_TERMINAL_MAX_SESSIONS` 两个纯值,手写 mock 猜漏一个就是
`No "deriveShellTerminalFontSize" export is defined on the mock`(已经踩过一次,10 条全红)。

**⑥ 合并 SSH / WSL 两块的包裹样式:`ProjectPage.tsx` 2999 → 2986 行。**
两处包裹 div 原先**逐字节相同**(`position` / `inset` / `display` / `zIndex` 四个字段各写一遍),
收敛成 `viewMode.ts` 的 `remoteTerminalLayerStyle(visible)`。纯搬运:helper 逐字段就是原字面量、
`visible` 对应 `remoteSshMainVisible`,且每次调用照样新建对象(和内联字面量一致,
React 的 style diff 行为不变)。

> **这里刻意只合并了远端那一对,没有再往前一步和 `shellCenterLayerStyle` 合并。**
> 本地 shell 是 `zIndex: 3`、远端是 `4`,本地那份还多带 `minWidth` / `minHeight` / `alignItems`。
> 合并会是 bug:WSL/SSH 项目下本地 shell 那块**也可能挂着**(它的条件
> `shellTerminalMounted && projectLocation.kind !== "ssh"` 只排除了 ssh,WSL 项目完全满足),
> 两层同时可见时靠这一级 z-index 差决定谁在上面。
> **新增 `src/test/project-page-terminal-layer-style.test.ts` 6 测试**,其中 2 条是专门的
> 防合并闸门(断言 `remote.zIndex > local.zIndex`、断言远端没有 `alignItems`),
> 谁想省掉一个函数会先看到它们挂掉。

**未做的一项及原因**:原计划把终端块里两个内联箭头函数(`onMinimize` / `onClose`)提成
`useCallback`,查证后**放弃了 —— 那不是纯搬运**。内联闭包每次渲染都是新标识,
`useCallback` 之后标识稳定;而 `ShellTerminalPanel.tsx` 里 `onClose` 会一路进到
`useImperativeHandle` 的依赖数组(`onClose` → `handleCloseShell:272` → `handleCloseActive:278`
→ `useImperativeHandle:286`)。标识稳定会让 `ref.current` 的重新赋值频率下降,
要做得先确认 `shellRef.current` 的标识没有被别处存住或比较。

**验证**:`tsc --noEmit` exit 0;`prettier --check`(4 个文件)干净;
`eslint --max-warnings 0` 干净;渲染 `ProjectPage` 的 3 个测试文件
(`project-toolbar` / `recursive-background` / `project-page-terminal-mount`)+
`project-main-view` + `command-palette-state` 全通过;
**变异测试 7 个全部杀掉,零存活**(z-index 降到与本地同级 / 降到本地之下 / 去掉可见性条件 /
`display` 恒 flex / `display` 两半颠倒 / 丢 `inset` / 丢 `position`),复原后与备份逐字一致。

**拆 `lsp.rs`:3733 → 2711 行**(计划 2 的第六个战果)。原有 `lsp/protocol.rs`,这轮加 2 个,
分两批(先实现 3733 → 3149,再搬测试 → 2711):

| 新文件 | 行数 | 原行号 | 内容 |
|---|---|---|---|
| `lsp/parse.rs` | 651 | 2214-2513 | 语言服务器响应 JSON → 强类型结构,14 个纯函数(`parse_signature_help` / `parse_code_actions` / `parse_inlay_hints` / `parse_document_symbols` / `parse_workspace_symbols` / `parse_workspace_edit` 等)。**输入完全不受我们控制**(另一个进程吐的),所以一律 `?` 短路 + 逐字段 `get()`,少字段就跳过不 panic。连 7 个用例一起搬 |
| `lsp/edit.rs` | 423 | 2515-2804 | 把 `WorkspaceEdit` 真写到盘上,11 项。**和 `parse.rs` 正相反,全是改用户源文件的副作用**:重命名符号一次写多个文件,所以两条不变量都在这儿 —— 路径钉死在项目根内(本地 `canonicalize`,远端只能字符串判断)、中途写失败必须回滚已写的。`lsp_position_to_offset` / `utf16_character_to_offset` 也在这:LSP 的 character 是 **UTF-16 码元**偏移,换算错就切在字中间。连 2 个用例一起搬 |

验证:`rustfmt --check`(我的 3 个文件)exit 0;`cargo build --lib` exit 0;
`cargo clippy --all-targets` **非禁区零告警**(仅剩 `notebook/export.rs` 那 5 个);
`cargo test --lib lsp` 搬前搬后都是 **26**;
`cargo test --lib -- --skip notebook` **1179 通过 0 失败**(与搬动前逐字相同)。
逐行对账:75 行新增全部配平 —— 25 行是加 `pub(super) ` 前缀(与 25 行「丢失」一一对应)、
3 行是 fmt 因前缀超 100 列的换行碎片、其余 39 行是模块 wiring / 测试块脚手架 / 模块头注释。
**属性归属 84/84**、**函数清点 154 → 154**。

**这批最省事,原因值得记:两段实测都是「整段私有」**(脚本里加了断言,`pub` / `pub(crate)` 各 0 处),
所以父模块只要 `use parse::*;` / `use edit::*;`,**一处 re-export 都不用写**。
挑段时优先找这种边界 —— `dap.rs` 那批也是,对比 `app_settings.rs` 那批为了可见性来回补了 4 处。

**这批新踩到的一个脚本 bug(会再咬人)**:删除用的 `del lines[start-1 : end+1]` 多删一行,
用意是带走段尾那个空行。但**最后一段的 `end+1` 不是空行,而是 `mod tests` 的收尾 `}`** ——
于是编译报 `this file contains an unclosed delimiter`,指向 2235 行的 `mod tests {`。
前两段没事只因为它们后面确实跟着空行。**搬动文件末尾的段时,`end+1` 要先判断是不是空行。**
这个错误好在是硬编译错误,不像属性孤立那样静默。

**共享测试 helper 又用了一次「提到模块层 + `#[cfg(test)]`」**:`temp_project` 有 7 个用户,
5 个留在 `lsp.rs`、2 个跟着 `edit.rs` 走。提到 `lsp.rs` 模块层,`edit.rs` 的测试块写
`use super::super::temp_project;` 取回 —— 和 `dap/paths.rs` 的 `unique_test_dir` 同一手法,
方向相反(那次是子模块定义、父模块取;这次是父模块定义、子模块取)。两个方向都成立,
因为**父模块的私有项对后代模块可见**。校验脚本会把它报成「备份里没属性,现在有 `#[cfg(test)]`」,
那是**必需的**改动(原来它在 `#[cfg(test)] mod tests` 里,提出来就得自带门控)。

**一个刻意留下的候选**:52 个 `#[tauri::command]` 薄壳在 **782-1395**(实测**连续**,
段内无任何非 command 顶层项),搬走能再降约 614 行到 ~2100。手法就是 `dsh_webui.rs`
那批 52 个命令的原样复制(`mod commands; pub use commands::*;`,**必须 glob**)。
没做的原因:`lsp.rs` 已经破 3000,而**计划 2 明确点名的 `ProjectPage.tsx` 还一行没动** ——
优先级在前端。

> 两次「不是我的失败」都靠 mtime 认出来:`cargo clippy` 报
> `notebook/sync/daemon.rs:402` 的 E0432(引了 `diff` 里还不存在的 `SyncSummary`),
> 该文件 mtime 17:15:53、我的 clippy 17:16、17:17 重跑 exit 0;
> 全 crate `cargo fmt --check` 报 `notebook/sync/store.rs` 需要重排,也是并行会话 16:48 写的。
> 判断禁区外的失败归属,先 `stat -f "%Sm"` 比时间。

> ⚠️ **我这轮踩到禁区一次(格式化,已发生、不可撤销)。** `cargo fmt` 是**整 crate**
> 生效的,我在写模式下跑了几次,其中一次(16:39:46 写完 `proxy_test.rs`,紧接着 fmt)
> 把并行会话新建的 `notebook/sync/schedule.rs` 一起重排了(该文件 mtime 16:39:47,
> 差 1 秒)。那个文件是 `??` 未入库状态,**没有基线可回滚**,而 rustfmt 只改排版不改语义,
> 所以没有去动它。
> **后续一律用 `rustfmt --edition 2021 [--check] <自己的文件…>`,不要再用 `cargo fmt`。**
> 已实测:我的 13 个文件 `rustfmt --check` exit 0、零 diff。
> 另外注意 `cargo fmt -- --check <文件>` **不能**限定范围(cargo 仍会把整个 crate 的文件
> 传给 rustfmt),必须直接调 `rustfmt`。
>
> 同理,全 crate 的 `cargo fmt --check` 现在会因为 `notebook/sync/store.rs`(并行会话
> 16:48 写的,还没格式化)报 exit 1 —— 那不是我的改动。

**跨平台**:`cargo check --lib --target x86_64-pc-windows-msvc` **跑不起来** ——
失败全在 C 依赖的 build script(`zstd-sys` / `ring` / `openssl-sys` / `psm` / `aws-lc-sys`,
报 `'stdlib.h' file not found`),本机没有 MSVC 头文件。`src/` 自身零错误。
所以**Windows 目标这轮没能验证**。这一批唯一的 cfg 门控是 `expand_windows_env_vars`,
它只被 `normalize.rs` 内部调用(已 grep 确认),搬动不可能改变它的可达性;
`#[cfg(windows)]` 也在属性归属核对里确认跟着走了。

**工具链**:`tsc --noEmit`、`eslint src --max-warnings 0`、
`prettier --check "src/**/*.{ts,tsx}"` 三项全干净。

> `tsc` 上踩到的一件事:本仓库 311 个测试文件里有 310 个**显式** `import { ... } from "vitest"`。
> `tsconfig.json` 没有配 `types: ["vitest/globals"]`,所以 vitest 的 `globals: true` 只在运行时
> 成立,`tsc` 不认 —— 新测试文件不写这行 import,单跑 vitest 全绿而 `pnpm build` 直接爆
> 559 个 `Cannot find name 'vi'`。新增测试文件时照抄邻居那行 import。

### 3.2 上一轮(压缩前,已验证)

- **样式去重**:新增 `src/styles/panelChrome.ts`(5 组具名导出),14 个组件迁移,
  消除 24 处重复常量定义,净 +155/−481。**刻意不并进 `styles/index.ts` 的扁平 `s`**
  —— 那里靠 spread 叠加,`header`/`input` 这种通用名进去会静默覆盖(`common.ts:95-98`
  记录了已经踩过的一次)。合并前用归一化比对逐属性证明等价;字面有差异的
  3 种 `label`、3 种 `input` **拒绝合并**,并在测试里钉住它们的差异。
  `src/test/panel-chrome-styles.test.ts` 18 测试,6/6 变异杀掉。
- **Rust 大文件拆分**:`app_settings.rs` 5161 → 4675,抽出
  `src-tauri/src/app_settings/launch_spec.rs`(528 行 + 8 个新测试,含 RAII `TempTree`)。
  11 测试通过,7/7 变异杀掉。
- **Rust 全量**:`cargo test` **1539 通过**;`cargo clippy --all-targets` 干净
  (仅剩的 2 个警告在 `notebook/rag/embed.rs`,属禁区);`cargo fmt --check` 干净。
- **Linux 交叉验证**(docker `rust:1-slim` + build-essential + GTK/webkit):
  `cargo check --lib` 我的 `launch_spec.rs` 零警告零错误(29 个警告全在 `notebook/rag`)。
- **Windows**:无法交叉编译(macOS 上没有 MSVC,psm/ring/zstd-sys/openssl-sys
  在 build script 阶段就挂)。改用静态符号核对 9 个 `#[cfg(windows)]` 项,无真实缺口。
- **全局覆盖率基线**(`--exclude notebook-panel.test.tsx`):293 文件 / 3399 测试全通过,
  **Stmts 70.48 / Branch 65.66 / Funcs 68.32 / Lines 73.23**,阈值 60/55/60/60。

## 4. 发现但**未修**的问题(现状已固化在测试里)

1. **`.markdown` 三处不一致** —— `isMarkdownFile` 认 `md|mdx|markdown`,但
   `loadLanguageExtension` 的 switch 和 `fileIconKind` 的图标表只认 `md|mdx`。
   结果:`.markdown` 文件能预览,但拿不到语法高亮、图标是通用 `file`。
   `file-extension-predicates.test.ts` 最后一条**故意钉住这个不一致本身** ——
   改的时候会看到它挂,那是提醒你在有意识地改行为,不是回归。
2. **`sqliteEndpointForFile` 不处理 WSL** —— 只判 `remote?.kind === "ssh"`,WSL 项目
   落到 `{kind:"local", path}`。但 `DbEndpoint`(`src/types/database.ts:15`)本身也只有
   local/ssh 两支,所以这是能力缺口而非纯 bug;修它要先扩类型再动后端。
3. **`.env.production` 不在 `loadLanguageExtension` 的 `nameMap` 里** ——
   `.env.local` / `.env.example` 都在,production 漏了。
4. **`styles/common.ts` 名字不实** —— 里面只有 error-boundary + usage-popover 两组样式。
5. **`DebugBreakpointGutterMarker.eq()` 是够不着的代码** —— 工厂里 activeMarker /
   spacerMarker 各只 new 一次,同配置内所有行共用同一实例,CodeMirror 先比引用就短路;
   reconfigure 时 gutter 整棵重建 DOM,也不经过 eq。改成恒 true / 恒 false 测试全绿。
   要么删,要么改成按值构造 marker。
6. **gutter `mousedown` 里显式的 `event.preventDefault()` 是第二道闸门** ——
   CodeMirror 在 `domEventHandlers` 返回 true 时自己也会 preventDefault。留着有自文档
   价值,而且删了 `event` 参数就变成未使用会触发 `noUnusedParameters`,不动。
7. **`SshTerminalPanel` 选中项回落有三套机制** —— 渲染只读派生出来的
   `selectedConnection`(自带 `?? connections[0] ?? null`),从不直接读 `selectedId`。
   所以同步 effect 里的 `setSelectedId(selectedConnection.id)` 和
   `handleDeleteConnection` 里的 `if (selectedId === connectionId) setSelectedId(...)`
   都是无可观测效果的状态卫生:单独摘、两个一起摘,53 条用例全绿(已验证)。
   收敛成一道即可,收敛后现有行为用例应照样绿。
8. **`SshWorkspace.deleteConnection` 里的 `setShowCards(true)` 是恒等操作** ——
   删除入口只存在于卡片视图,能点到删除时 `showCards` 必然已是 true(埋点实测:
   命中 2 次,`showCards === false` 0 次)。删掉不影响行为。
9. **复制密码有三道闸门** —— 按钮 `disabled`、onClick 的 `if (!canCopyPassword) return`、
   `copyConnectionPassword` 里的 `!password`。只有 `disabled` 可观测。后两道是兜底,
   其中函数内那道建议保留(模块级函数,将来别处调到时它是唯一防线)。
10. **React 19 下 `onBeforeInputCapture` 是死代码 —— 本轮唯一确认的真 bug。**
    `PromptEditor.tsx:668-695`(函数体 671-690)的 IME 重放去重层**永远不执行**。
    两路证据:
    - 源码级 —— `node_modules/react-dom/cjs/react-dom-client.development.js:27428` 是
      `registerTwoPhaseEvent("onBeforeInput", ["compositionend","keypress","textInput","paste"])`,
      原生 `beforeinput` **不在**订阅列表里;而能到达它的那几种事件
      (CompositionEvent / KeyboardEvent / TextEvent)都没有 `inputType` 字段,
      于是首行 `event.inputType !== "insertText"` 恒真、立即 return。
    - 实测 —— 派发原生 `beforeinput`,handler 调用 0 次;派发 `compositionend`,调用 1 次。

    后果:拼音残留清理实际只靠 `compositionend` 里的 `normalizeEditorCompositionText`
    那一路(已覆盖并变异验证过)。**同样的写法还在 `src/components/useTextInputIMEFix.ts:19`**,
    被 `SettingsDialog` / `GitChanges` / `AgentDetailModal` / `AgentConfigPanel` /
    `TaskEditDialog` 五处消费 —— 它的 `onCompositionEnd` 那一路是活的,只有
    `onBeforeInputCapture` 死。
    **没修的原因**:正确的修法是挂真正的原生 `beforeinput` 监听,这会**改变运行时行为**,
    必须在 Linux WebKitGTK / macOS WebKit 上做真实输入法验证 —— 我做不了这个验证。
    已用一条**断言现状而非期望**的用例钉住(`prompt-editor.test.tsx` 的
    「原生 beforeinput 到不了组件,去重层不生效」):谁把它修活了,这条会挂并提醒补配套用例。
11. **`serializeEditor` 的 `parts.length > 0` 与末尾的 `.trim()` 重复** ——
    空数组 `join("")` 就是 `""`,`"".trim()` 还是 `""`。等价变异。
12. **尾随空格那行的 `nodeType === Node.TEXT_NODE` 够不着** —— 要有可观测差异,
    chip 的下一个兄弟得是 `textContent` 恰好为 `" "` 的**元素**(如 `<span> </span>`),
    而插入 chip 的代码只造 Text 节点、粘贴走纯文本路径,编辑器里没有任何会生成 wrapper
    元素的格式化命令。防御性判断,不为它编造 DOM 状态。
13. **`handleKeyDown` 开头的撤销守卫今天是恒等操作** ——
    `if (!isComposingRef.current && isPromptUndoShortcut(e)) return;` 整行删掉,175 条全绿。
    因为 `"z"` 在下游所有分支里都不匹配(不是 Backspace/Delete/Escape/方向键,
    也过不了 `key === "Enter"`)。与下游那串 `key === …` 判断互相兜底。
    **建议保留**:零成本前置声明,一旦有人给 `handleKeyDown` 加一条覆盖面更宽的分支
    (比如统一拦某类组合键),它就是唯一防线。
14. **`FileExplorer.deletePath` 的重入闸门开晚了一步** ——
    `deleteInFlightRef.current = true` 写在 `await confirm(...)` **之后**
    (`FileExplorer.tsx:883`,闸门本身在 `:869`),所以确认框还没决的这段窗口里,
    第二次触发照样能进去,两次都会打到后端。真实后果有限(同一路径删两次,第二次
    后端报 not found → 一条错误 toast),但它是一个可观测的重入窗口。
    **没修的原因**:正确的修法是把置位挪到 `confirm` 之前 —— 那会改变「用户取消后
    闸门要不要放开」的语义(需要 try/finally),属于行为改动,不在「不影响功能」的范围内。
    已用一条**断言现状**的用例钉住(`file-explorer-fs-actions.test.tsx` 的
    「确认框还开着时重复触发,两次都会打到后端」):谁把它修了,这条会挂。
15. **`sortTreeNodes` 的递归在读成功的路径上是冗余的** ——
    换排序会同时触发 `sortTreeNodes` 与一次 `refresh()`,而 refresh 重读时每一层
    自己就 `sortFileEntries` 一遍;两条路都存在时顺序对不对全归 refresh。
    唯一让排序 effect 独自生效的窗口是**重读失败**(refresh 抛错后只写 `loadError`,
    一个节点都不动)。**建议保留**:它正是那个窗口里唯一的排序来源,
    已有用例覆盖(「读目录失败时切换排序,已加载的树仍然就地重排」)。
16. **`tsconfig.json` 缺 `types: ["vitest/globals"]`** ——
    vitest 配了 `globals: true`,但 `tsc` 不知道,靠 311 个测试文件里 310 个**手写**
    `import { describe, it, expect, vi } from "vitest"` 兜着。新测试文件漏这行 import:
    `vitest run` 全绿,`pnpm build` 爆几百个 `Cannot find name 'vi'`(我这轮就撞了一次,
    559 个错误)。**没修的原因**:加这一行 types 是全局配置改动,会影响所有测试文件的
    类型解析面(显式 import 与 globals 并存时的优先级、`expect` 扩展的类型合并),
    不属于本轮「只改实现」的范围。**建议单独一条 PR 处理**,顺带删掉 310 处冗余 import。

两条**不是 bug、但已被测试钉住**的历史行为,合并后缀表时别顺手「修」掉:
- `fileExtension("Makefile", "")` 返回 `""`(`??` 只挡 null/undefined,空串会盖掉名字)。
- 名字整体等于后缀的无后缀文件会命中,例如 `isPreviewableImageFile("png") === true`。

## 5. 待做

### 5.1 计划 3:覆盖率 —— 禁区外 12 个薄弱文件,已完成 11,第 12 个开了头

| 文件 | 行数 | 行覆盖 | 未覆盖≈ |
|---|---|---|---|
| App.tsx | 2828 | 见下 | 大头仍在 |
| ~~FileExplorer.tsx~~ | 1285 | **99.8%** ✅ | — |
| ~~PromptEditor.tsx~~ | 758 | **95.96%** ✅ | — |
| ~~editorUtils.ts~~ | 613 | **99.06%** ✅ | — |
| ~~ShellTerminalPanel.tsx~~ | 546 | **100%** ✅ | — |
| ~~SshTerminalPanel.tsx~~ | 465 | **100%** ✅ | — |
| ~~ThemePanel.tsx~~ | 454 | **96.55%** ✅ | — |
| ~~SshWorkspace.tsx~~ | 410 | **100%** ✅ | — |
| ~~DshSlashPalette.tsx~~ | 406 | **100%** ✅ | — |
| ~~DshQuestionDialog.tsx~~ | 401 | **100%** ✅ | — |

风险序(后果不可逆优先,已完成的保留在序列里做参照):
~~FileExplorer(直接删/改真实文件,且无撤销)~~ →
~~PromptEditor(丢用户刚敲的 prompt / 发错内容)~~ →
~~ShellTerminalPanel~~(孤儿 PTY;
`handleCloseShell` 里 `nextShells[i] ?? nextShells[i-1] ?? nextShells[0]` 是
off-by-one 温床)→ SshTerminalPanel → DshQuestionDialog → 其余。

**注意风险序与覆盖率收益序在末档打架**:ThemePanel 风险最低(最坏是配色难看),
但未覆盖行 330 比 SshWorkspace / DshSlashPalette(各 190)都多。按风险排它最后,
按数字排它靠前 —— 取决于当下要的是降风险还是推数字。

**`App.tsx` 已覆盖 / 未覆盖的分片**(全部按边界级写,见 §0 的理由):
- ✅ `remote-task-request` 处理器(949-1118):`app-remote-task-request.test.tsx`,
  22 测试 / 24 变异全杀。8 条拒绝路径 + 2 条顺序契约。
- ✅ 启动链路 / 事件订阅**存在性**:`app-boot.test.tsx`、`app-event-wiring.test.tsx`
  (后者只断言 20 个事件的订阅存在,handler 体基本没跑)。
- ❌ 仍然没测的大片:任务创建/启动、任务切换、项目打开(local / SSH / WSL)、
  skill hub、hub 模式、窗口生命周期、DSH webui 启动。
  下一片建议接着挑「阻塞手机端」这类外部可见契约,理由同上:
  这种断言不依赖内部结构,拆组件时不用重写。

`editorUtils.ts` 的可测面(我已读完全文,风险判断修正过:它不是偏移量/改写逻辑,
而是 CodeMirror 扩展工厂 + 懒加载语言表 + 主题常量):
`loadLanguageExtension` 的 nameMap 与 switch 各分支、`sqliteEndpointForFile`、
`diagnosticSeverityColor`、`createDebugBreakpointGutter` 与
`createInlineBlameExtension` 的早退分支(后两者要在 jsdom 里挂真 `EditorView`)。

### 5.2 计划 2:上帝结构 —— 后端最大那个已拆完,前端一个没动

**已完成 `dsh_webui.rs` 5429 → 3795**(详见 §3.1)。剩下的按行数:

| 文件 | 行数 | 备注 |
|---|---|---|
| ~~`app_settings.rs` 4678~~ | **3516** | ✅ 本轮拆完(§3.1)。累计 5161 → 3516,现有 9 个子模块。还能再抽两段(**行号是搬动后的现值**):DTO 段 ~92-600(25 个 struct/enum + 它们的 Default impl,`CustomAgentProfile` 在 115、`impl AgentFamily` 在 560)和设置读写段 ~721-990(目录/路径/指纹缓存/load-persist-update,`clear_cached_versions` 在 721、`update_settings_locked` 在 977) |
| ~~`dap.rs` 4222~~ | **3395** | ✅ 本轮拆完(§3.1)。还能再抽两段(**下面是搬动后的实测行号**):Node/CDP 引擎 **968-1476**(`send_request` 968 → `inspect_session_loop` 1268 → `wait_then_kill_child` 1451,约 509 行)和 Python 适配器 **1477-1981**(`send_debug_adapter_execution_command` 1477 / `python_debug_adapter_loop` 1520 / `start_python_session_actor` 1952,约 505 行)。两段搬完约到 2380。另外 `python_debug_adapter_loop` 本身**一个函数 432 行**(1520-1951),是这文件里真正的上帝函数,拆它属于改结构不是纯搬动,要单独一轮 |
| ~~`session.rs` 3973~~ | **3300** | ✅ 本轮拆完(§3.1),现有 3 个子模块。还能再抽三段(**下面是搬动后的实测行号**):两家会话监视器 **144-658**(`codex_sessions_roots` 146 → `process_claude_session_line` 614,约 515 行,含 4 个 `collect_*` 递归遍历)、会话文件查找与跨配置接管 **1468-1683**(`strip_ansi` 1471 / `find_claude_session_file` 1511 / `find_codex_session_file` 1563 / `adopted_session_target_path` 1596,约 216 行)、`/status` 发现与 watcher 编排 **1684-2250**(`extract_claude_status_session_id` 1688 → `spawn_resume_session_watcher` 2238,约 567 行)。三段搬完约到 2000。注意第三段里 `spawn_claude_lazy_session_attach`(1928-2089)和 `run_status_session_watcher`(2128-2237)都是**长函数**,搬动不解决它们本身 |
| ~~`lsp.rs` 3733~~ | **2711** | ✅ 本轮拆完(§3.1),现有 3 个子模块。**还剩一段现成的**:52 个 `#[tauri::command]` 薄壳在 **782-1395**(实测连续,段内无非 command 顶层项),搬走再降约 614 行到 ~2100,手法照抄 `dsh_webui.rs` 那批(`mod commands; pub use commands::*;`,**必须 glob**)。刻意没做 —— 已破 3000,优先级让给前端 |
| ~~`local_router/server.rs` 3621~~ | **2834** | ✅ 已拆(§3.1)。实现 ~1828 + 测试 ~1005 |
| ~~`ProjectPage.tsx` 3096~~ | **2426** / 26 useState / 40 useCallback / 13 useEffect | 🟢 相对 HEAD 降了 **670 行**。抽出 4 个模块:`ProjectTerminals.tsx`(157)、`ProjectRightPanel.tsx`(386)、`ProjectWorkspaceTabs.tsx`(289)、`src/hooks/useEditorRunDebugState.ts`(248)。新增 6 个测试文件共 **75 条**渲染级用例。本轮跑了 5 次变异:4 次直接被杀,第 5 次(run 草稿的竞态守卫)**存活** —— 那是搬移前就存在的覆盖缺口,已补用例并复验杀掉。剩下的候选见 §5.4 ④ |
| `App.tsx` | 2828 / 27 useState | 处理顺序未经确认(§2) |
| `FileViewer.tsx` | 2601 / 43 useState | 没动 |

`dsh_webui.rs` 那一轮验证下来的拆法,后面几个照抄就行:

1. **先量边界再动手**:用 `grep -n "^pub struct \|^impl \|^#\[tauri::command\]"` 打出顶层
   声明表,挑**连续**的一段。交错的 helper 会让「一刀切」变成来回补 `pub(super)`。
2. **搬动用脚本按行号切,不要手抄**。手抄一定会引入差异,而差异混在 2000 行 diff 里
   看不出来。脚本里先断言首尾行内容,错一行就是一个语法错误加一堆误导报错。
3. **搬完写个校验脚本证明是纯搬动**:子模块每一行都要能在搬前的备份里逐字找到
   (只放过模块头、`use super::*;`、`pub(super)` 前缀、以及 `cargo fmt` 因为加前缀
   超过 100 列而换行的签名)。这一步花 5 分钟,换来「行为不可能变」这个结论。
   **校验里必须单独核对 `#[derive]` 归属**:按行号切会把切点上方一行的属性落在原地,
   贴到原文件的下一个 item 上。`server.rs` 上真发生过 —— **能编译、91 个测试全绿、
   仍然是错的**,只有 `cargo fmt --check` 报出来。脚本现在按「struct 名 → 紧贴其上的属性行」
   两边对账。
   **同一个陷阱有三种形态,危险度递增**:`#[derive]`(fmt 会报)、
   `///` 文档注释(**fmt 不报** —— rustfmt 不管文档注释贴给了谁,`session.rs` 上遇到过,
   只有逐行对账能看出来)、`// ── 段头 ──`(下一段的标题被上一段带走,纯可读性损失)。
   所以**切之前把切点上下各看三行**,别只看是不是空行。
4. **tauri 命令可以 re-export,但必须用 glob**:`mod commands; pub use commands::*;` 之后
   `lib.rs` 的 `generate_handler![dsh_webui::foo]` 照旧解析(实测编译通过),
   那份只能手写的注册表一行都不用改。DTO 同理。
   **按名字 re-export 不行** —— `#[tauri::command]` 除了函数还生成
   `__cmd__<名字>` / `__tauri_command_name_<名字>` 两个隐藏宏,`generate_handler!` 要用它们,
   `pub use m::the_command;` 带不走,报 `macro import ... is private`。
4b. **搬动一律不改可见性等级**。脚本给私有项加 `pub(super)` 时要**跳过**已经带
   `pub` / `pub(crate)` 的行,并在父模块显式 `pub use` / `pub(crate) use` 原样转发 ——
   glob import 只把名字拉进父模块作用域,不替父模块对外转发可见性。
   漏了就是一串 E0603(`custom_agent_home` 一次报了 6 个调用点)。
5. 子模块开头 `use super::*;` —— 父模块的私有类型直接可见,省掉几十行 import。

**`local_router/server.rs` 已按下面量好的边界拆完(3621 → 2834,实现 ~1828 + 测试 ~1005,
`#[cfg(test)]` 现在在 1829 行)。** 落地时改了一处:三个文件放在
`src/local_router/server/` **子目录**下,不是 `local_router/` 同级 —— 同级的话
`use super::*;` 指向 `local_router` 而不是 `server`,拿不到 `server.rs` 的私有类型。

| 新文件 | 原行号 | 内容 |
|---|---|---|
| `server/routing.rs` | 412-586 | `SelectedRoute` + 它的 impl、`select_route`、`strip_agent_prefix`、`normalize_codex_path`、`build_upstream_url` |
| `server/guard.rs` | 587-722 | `filter_request_headers`、`request_is_authorized`、`request_is_cross_site`、`trimmed_header`、`origin_is_loopback`、`router_credentials`、`constant_time_secret_eq`、`strip_router_credentials`、`filter_response_headers`、`filter_hop_by_hop` |
| `server/semantic.rs` | 1384-1607 | `inspect_stream_start`、`inspect_sse_block`、`chat_chunk_has_output`、`semantic_error_from_bytes/_value`、`semantic_protocol_name`、`SemanticStreamObserver` + impl |

三段都验证过是**连续**的(中间没有夹别的东西),按行号切没问题。当时记下的几条注意:

- `routing.rs` 与 `guard.rs` 在原文里紧挨着(412-722),想少跑一轮验证可以合成一个
  `request.rs` 一次搬完;分开的好处是 `guard.rs` 里全是安全相关的判断
  (常量时间比较、跨站判断、凭据剥离),单独一个文件更容易审。
- `semantic.rs` 依赖 `SemanticProtocol` / `StreamStartInspection` 两个 enum
  (原 1150-1161),它们和主体之间隔着 `send_upstream` 等函数 —— **不连续**。
  要么把这两个 enum 单独搬过去(两处 `pub(super) enum`),要么留在父模块靠
  `use super::*;` 取。后者更省事。
- 测试块里有大量对应这三段的用例(`select_route` / 授权 / SSE 检查)。**实际是分两步做的**:
  先只搬实现(3621 → 3103),跑完四道闸门,再单独一轮搬 14 个用例(→ 2834)。
  分两步的好处是搬测试引起的 unused import 不会混在实现的报错里。

搬完实测 **2834**,比原先估的 3086 更低,因为连测试一起搬了 —— 已经破 3000,
这个文件可以先放下。真要继续压,剩下的候选还是 `attempt_target`(原 748-1037,290 行)
和 `ProxyBodyState` / `RequestCompletion` 那组 Drop 语义的状态机(原 1923-2222,300 行);
后者涉及请求收尾的副作用顺序,风险比前三段高,要单独一轮并且仔细看 `Drop` 的实现。
(这两处行号是**搬动前**的,现在得重新 grep。)

### 5.4 `ProjectPage.tsx` 剩下的部分怎么拆(**下一步就是这里**)

现状(**实测**,搬动后的现值,总 2999 行):

| 段 | 行号 | 行数 | 说明 |
|---|---|---|---|
| import | 1-160 | 160 | |
| props 解构 + 类型 | 162-219 / 220- | ~58 | **57 个 prop** |
| hooks 段 | ~300-1693 | ~1394 | 13 `useState` / 6 `useRef` / 14 `useEffect` / **45 `useCallback`** / 9 `useMemo` |
| **JSX return** | **1694-2999** | **1306** | **占 44%,是这个文件真正的上帝结构** |

JSX 的顶层结构(可抽子组件的天然边界,行号实测):

```
1704  <ProjectRail …/>
1741  <div>                          ← 主体
  1751    <AnimatedSelectionTrack>   (227 行) 任务标签条
  1981    <AnimatedSelectionTrack>   (67 行)  第二条
  2061    {remoteConnectionMissing && …}
  2118    {showTopRightIdeTools && …}
  2148    {actionFeedback && …}        ← 之后一路到 2693 是工作区主体
    2198      {projectTasks…}          任务视图
    2293      {auxiliarySplit && …}
    2544      {shellTerminalMounted && …}      本地终端
    2586      {showRemoteSshTerminal && …}     SSH 终端
    2615      {projectLocation.kind === "wsl" && …}  WSL 终端
    2640      {filePreviewTarget && …}
    2667    {sshMounted && …}
2697  {visibleRightPanel && …}       (242 行) 右侧面板
2939  <RightToolbar …/>
2968  {showFileSearch && …} / {commandPalette…} / {showSettings && …}
```

**建议的下一刀,按「收益 ÷ 风险」排**:

1. **三个终端块(2544-2666,约 123 行)抽成 `ProjectTerminals`。** 三个分支互斥
   (本地 / SSH / WSL),各自的 props 都已经在 hooks 段算好了,是最接近纯搬动的一块。
2. **右侧面板(2697-2938,242 行)抽成 `ProjectRightPanel`。** 它整块被
   `{visibleRightPanel && …}` 包着,只有一个入口条件。
3. **两条任务标签条(1751-2047,294 行)抽成 `ProjectTaskTabs`。**
4. **hooks 段按簇抽自定义 hook** —— 这一步**不是纯搬动**,要单独一轮:
   - ~~终端簇 → `useProjectTerminals()`~~ **不要按原计划抽。** 数过写入点:
     `setShowShellTerminal` 23 处、`setShowRemoteProjectTerminal` 19 处,散在约 15 个
     handler 里。这两个标志是**页面级导航状态**,几乎每个动作都要写它;聚成 hook 只会
     得到一个「存状态 + 暴露裸 setter」的壳,还要改 ~42 个调用点,不换来任何内聚。
     真正值得抽的是更小的一簇:`shellTerminalMounted` / `shellSessions` /
     `activeShellId` / `shellReadyRef`(约 13 处),它们有真实约束 —— 重置时必须同时
     清理,否则下次挂载显示上一轮的残留页签。建议命名 `useShellSessionLifecycle()`,
     把两处重复的重置逻辑合并成一个具名操作。
   - ~~编辑器运行/调试簇 → `useEditorRunDebugState()`~~ ✅ **已完成**,见
     `src/hooks/useEditorRunDebugState.ts`(248 行)。搬进去的是 8 个 state、4 个 ref
     (含 `previewOpenedForRunRef`)、换项目的重置 effect、6 个 handler,外加把
     `handleActivateIdeTool` 里三段派请求的逻辑收成 `requestTestRun` /
     `requestRunDraft` / `requestDebugDraft`。
     **导航(`openRightPanel` / 收终端)以参数注入,没有搬进去** —— 页面里另有十几个
     读写点。`hideShellTerminal` 必须是 `useCallback(…, [])`:它进了 hook 内几个
     handler 的依赖数组。
     `editorDiagnostics` **留在页面**(来自 LSP 推送,不是这一簇);原先和它同一个
     effect 的 `setEditorCoverage(null)` 合并进了 hook 的重置(两者都以
     `project.path` 为键,语义不变,并有用例钉住)。
     `handleActivateIdeTool` 也留在页面(它还要报动作反馈,依赖一堆页面级量)。

**开工前必读**:`ProjectPage.tsx` 现在**行覆盖 66.51% / 函数覆盖 51.56%**
(实测,来自 `project-toolbar` / `recursive-background` / `command-palette-state` /
`ssh-project` 四个文件、共 62 条渲染真组件的用例)。函数覆盖只有一半 ——
**45 个 `useCallback` 里约一半从没被触发过**。抽子组件时被动到的那些如果正好在未覆盖的一半里,
测试不会告诉你搞错了。所以每抽一块之前,先确认这块的入口在现有用例里跑得到;
跑不到就先补一条渲染级用例把它钉住,再动。

### 5.3 计划 4:实机 / 爆破 / 高并发 —— **五块完成,后端并发面已覆盖**

> **先修正一个前提。** 本节原来写着「爆破目标是 pairing-code 与 device-token」——
> 读了实现之后这个目标是错的:两者都是 `generate_token()` 出来的
> **32 字节 CSPRNG → base64url,256 位熵**,没有短数字码。写循环去猜是在演戏。
> 真正会出错的是**猜不中时的那条路径**:限流闸门有没有 TOCTOU、报错文案是否
> 构成枚举预言机、限流表会不会被撑爆、一次性 invite 在竞态下会不会被用两次。
> 已完成的两块都按这个重新定的方向写。

**① `src-tauri/src/remote/auth_stress_tests.rs`(新增,8 测试)**

| 用例 | 钉住的性质 |
|---|---|
| `concurrent_bad_invites_end_with_the_peer_blocked` | 64 路并发错 invite,零放行,收工处于封禁态 |
| `throttle_blocks_within_the_free_allowance_even_under_concurrency` | 凭据错误文案出现次数 ≤ 免费额度+1,其余全是限流文案 |
| `single_use_invite_survives_a_concurrent_stampede` | 32 路并发抢同一个**正确** invite,只成功 1 次且设备表只多 1 台 |
| `a_consumed_invite_cannot_be_replayed` | 时间视角的重放(和上一条会被不同的 bug 打破) |
| `throttle_is_per_peer_and_does_not_punish_bystanders` | A 被封,B 拿正确 invite 仍一次通过(否则输错几次就能锁全家) |
| `throttle_map_stays_bounded_under_a_distributed_attack` | 4096 个 IP 各失败一次,限流表 ≤ 1025 条 |
| `failure_messages_do_not_distinguish_near_misses` | 空串 / 同长度差一字符 / 正确前缀截断 / 等长全 A,报错必须同一句;invite 和 device token 两条路径各跑一遍 |
| `a_revoked_device_token_fails_like_an_unknown_one` | 撤销后的文案 == 从未签发的文案 |

配套改动:`THROTTLE_FREE_FAILURES` 提为 `pub(crate)`(测试里硬编码 3 会在改常量时假绿)、
新增 `#[cfg(test)] throttle_len()`(只暴露计数,表本身保持私有 —— 否则测试能直接构造状态,
断言就变成自说自话)。

> **没给 `AuthOutcome` 派生 `Debug`。** `expect_err` 要求 `T: Debug`,但
> `AuthOutcome::Paired` 里装着**明文 device token**,派生 Debug 等于给「某天某处顺手写个
> `{:?}` 把长期凭据打进日志」开门。为一个测试便利去放宽生产类型的可打印性不值得,
> 改成文件内自己的 `err_of()` 接管这一步。

> **一个结构性结论**:`throttle_wait` 和 `record_failure **都是私有的**,调用方无法在两者
> 之间插入别的操作;而生产侧 `state.auth.lock().authenticate(...)` 是整调用一把锁
> (`remote/mod.rs:187`)。所以闸门的原子性是**结构保证**的,不是靠约定。
> 测试证实了这个组合的最终状态,但要记得:它测不出「某个未来的调用方拆开两步用」——
> 那种写法压根编译不过,这才是真正的防线。

**变异测试 9 个,8 杀 1 存活**(只跑 `auth_stress` 过滤器,不带 auth.rs 自带的 15 个用例 ——
否则旧用例杀掉的变异会被记成我的战果)。存活的那个是 **等价变异,如实记录**:
把 device token 查找的 `constant_time_hash_eq` 换成 `==`。两者对所有可达输入
**功能完全一致**(都按长度先否,而 `hash_token` 恒产出 64 位 hex,`is_empty()` 那道
守卫从这条路径不可达),差别只在时序侧信道 —— 没有任何基于返回值的断言能区分,
而挂钟计时的测试只会 flake。**不为了凑杀死率去写它。**

**② `remote-relay/src/rate_limit.rs` 追加 `stress_tests`(11 测试)**

`main.rs` 的测试模块已覆盖顺序基本面,这里补对抗面。`try_acquire` 的 `now` 是入参,
所以窗口相关用例全部确定性推进时间,**不 sleep**。

| 用例 | 钉住的性质 |
|---|---|
| `concurrent_attempts_from_one_ip_cannot_exceed_the_window_budget` | 64 路并发单 IP,放行次数**恰好**等于额度 |
| `concurrent_attempts_from_many_ips_each_get_their_own_budget` | 16 IP × (额度+4) 并发,每个都拿满且不多放 |
| `the_table_stays_bounded_and_evicts_the_least_recently_seen_entry` | 表 ≤ 4096,且淘汰的是 `last_seen` 最旧那条 |
| `evicting_a_throttled_entry_gives_an_attacker_strictly_less_than_flooding_already_does` | 见下 |
| `a_throttled_peer_stays_throttled_within_the_window_without_table_pressure` | 窗口内重试 49 次全拒(「重试刷新 last_seen」不能变成「重试能续命」) |
| `the_window_is_fixed_not_sliding_and_that_is_documented_here` | 固定窗口跨边界能拿 2 倍额度,断言「确实是 2 倍不会更多」 |
| `a_non_loopback_peer_can_never_override_its_own_rate_limit_key` | 直连客户端伪造 3 种头部组合都无效 |
| `malformed_forwarded_headers_fall_back_to_the_peer` | 空 / 空白 / `not-an-ip` / `,` / `999.999.999.999` / `::gg` 全回落到对端 |
| `the_rightmost_forwarded_entry_wins` | 取最右一跳(取最左会采信客户端自填段) |
| `an_ipv6_loopback_proxy_is_also_trusted` | `::1` 也是可信反代(漏这支会让 IPv6 部署下所有客户端共享一条计数) |
| `an_ipv4_mapped_loopback_is_not_treated_as_a_trusted_proxy` | `::ffff:127.0.0.1` 的 `is_loopback()` 为 **false**,方向是安全那侧 |

> **写这一批时有一条测试先失败了,而错的是我的前提,不是代码。**
> 我原本断言「攻击者无法靠灌表挤掉自己那条来清零计数」,理由是被拒时
> `last_seen` 也会刷新。**实测失败** —— 灌表流量的时间戳比攻击者的更新,
> 所以攻击者恰恰是最旧的那条,确实会被挤掉、计数确实被重置。
>
> 算了一下成本才知道不该修:灌满表需要 4096 个独立 IP,而这些 IP 自身就有
> **4096 × 12 = 49152** 的额度,比「重置一条拿回 12」多 **4096 倍** ——
> 走淘汰路径重置自己是**严格更差的攻击**,不给攻击者任何新能力。
> 反过来如果改成「表满时拒绝新 IP」,灌表者就能把新来的正常客户端全挡在门外,
> 那是真正的 DoS。当前实现是两害中更轻的一侧。
> 于是把用例改成**如实复现这个行为**并在注释里写下成本分析,同时守住真正的边界:
> 重置一次也只拿回一个窗口的额度。将来谁改了淘汰策略,这条会失败并把人指到那段分析。

**变异测试 9 个全部杀掉,零存活**(额度 `>=`→`>`、不累加、不分 IP、去表上限、
淘汰挑最新、窗口永不翻转、非 loopback 也信头部、取最左一项、畸形头部回落到 `0.0.0.0`)。

**验证**:`cargo test --lib -- --skip notebook` **1187 通过 0 失败**(新增 8 条,原 1179);
`cargo test --lib remote::` 126 通过;relay `cargo test` **21 通过**;
两个 crate 的 `cargo clippy --all-targets` **零告警**;
`rustfmt --edition 2021 --check` 干净(只对我的文件跑 —— `cargo fmt` 是整 crate 的,
会重排并行会话的禁区文件)。

**③ `src-tauri/src/local_router/circuit_breaker.rs` 追加 `stress_tests`(6 测试)**

熔断器是整个 local_router 唯一的**共享可变状态**,所有上游请求都撞它。原有 `tests`
覆盖顺序状态机(阈值打开、半开单探针、中性释放),这里补并发面。
`timeout_seconds: 0` 让 Open 态一次 `allow_request` 就翻 HalfOpen,**全程不 sleep**。

| 用例 | 钉住的性质 |
|---|---|
| `only_one_of_many_concurrent_requests_gets_the_half_open_probe` | 64 路并发**恰好** 1 个放行,且它必须带 `used_half_open_permit`(否则完成时不归还令牌) |
| `a_half_open_permit_is_never_leaked_by_any_completion_path` | 成功 / 失败 / 中性三条归还路径逐个走 —— 漏一条就永久卡在半开且无人可试探 |
| `concurrent_failures_open_the_circuit_exactly_once` | 128 路并发失败,`failed_requests` 与 `total_requests` 一次不少(少记=有失败丢在竞态里) |
| `a_closed_circuit_never_throttles_concurrent_traffic` | 256 路并发全放行,且闭合态不发半开令牌 |
| `the_error_rate_path_opens_the_circuit_without_a_failure_streak` | 只走 `||` 的错误率那半(失败阈值设到 `u32::MAX` 够不着) |
| `reset_clears_the_probe_token_and_the_counters` | `reset` 必须连令牌一起清,否则重置后第一个请求被当成"已有探针在飞"拒掉 |

> **不测"注册表被撑爆"**:`target_id` 来自用户自己配的上游列表(`server.rs:201` 的
> `target.id()`),不是请求里的数据,条数由配置规模决定。那张 map 无上限是可接受的 ——
> 写进测试模块的文档注释里,免得后来人当成漏洞去"修"。

**变异测试 11 个,9 杀 2 存活。两个存活都不是缺测试,是冗余闸门。**
追加了一轮组合变异来定位承重点(只去一处 / 去两处 / 去三处),结论:
`half_open_in_flight` 共在 **5 处**清零,只有 2 处承重 ——

| 清理点 | 结论 |
|---|---|
| `transition_to_half_open` | **承重**:进入 HalfOpen 的唯一入口,而这个标志只在 HalfOpen 分支被读 |
| `record_success` 的 `if used_half_open_permit` | **承重**:未达 `success_threshold` 时不发生任何 transition,不清就永久卡住 |
| `transition_to_open` / `transition_to_closed` / `record_failure` 的同款守卫 | 冗余(单去、两两同去都全绿;三处同去才红) |

> **三处冗余刻意保留,已在代码里写明原因。** 令牌泄漏的后果是熔断器永久停在半开
> 且无人可试探(上游恢复了也回不到 Closed),是这个文件最严重的失效模式;
> 状态机被 6 个方法改写,让每次转移自己恢复不变量比依赖"当前调用图恰好不会走到那里"更稳。
> 这与「无冗余代码」不冲突:那条标准针对的是上帝结构 / 死代码 / 重复逻辑,
> 而不是并发状态机里的防御性不变量恢复 —— 删掉是拿健壮性换一个观感上的胜利。
> 按 [[mutation-survivors-mean-redundant-guards]] 的说法这类应「收敛成一道」,
> 这里的判断是**例外并说明理由**,而不是默默留着。

**④ `src-tauri/src/local_router/mod.rs` 追加 `metrics_stress_tests`(9 测试)**

补的是两块**全仓库零测试引用**的进程级共享可变状态(搜 `RuntimeMetrics` /
`begin_request` 只有实现自己那几行)。它们的失效方式都是「数字慢慢飘、面板一直显示
错的东西」,不崩不报,只能靠断言守。

| 用例 | 钉住的性质 |
|---|---|
| `concurrent_begin_and_finish_conserve_every_counter` | 16 线程 × 500 次轮转三条收尾路径,收工 `active == 0` 且三路之和 == `total` |
| `in_flight_requests_are_visible_while_they_are_still_running` | **栅栏对齐,在"还没收尾"时读一次 `active`**,必须等于在途线程数 —— 见下 |
| `a_client_abort_is_counted_as_neither_success_nor_failure` | 断开只回收在途,`successful` 与 `failed` 都保持 0(`mod.rs:596` 的约定) |
| `active_requests_never_underflows_when_finish_outnumbers_begin` | 多余收尾夹在 0,不回绕成 `u64::MAX`(那会显示 1844 亿在途) |
| `concurrent_error_writes_leave_a_consistent_last_error` | 12 线程 × 100 次并发写 `last_error`,`agent` 与 `message` 必须来自**同一次**写入(字段撕裂就红) |
| `a_restart_on_the_same_port_does_not_clear_the_new_generation` | 重启重叠窗口:旧一代收尾不能撤掉新一代的标记 —— **`mod.rs:632` 注释描述的真实 bug** |
| `generations_are_strictly_increasing_under_concurrency` | 16 × 200 并发取号两两不同(撞号就退回上面那个 bug) |
| `is_listening_on_only_answers_for_the_recorded_port` | 别的端口必须 false(宁可让 Agent 直连上游,也不指向没人接的端口) |
| `clearing_an_unknown_generation_is_a_no_op` | 停一个早被顶替的服务不影响当前标记 |

配套改动:`RuntimeMetrics` 新增 `#[cfg(test)] counters()` 返回四个计数器快照 ——
生产侧读这些值走 `LocalRouterState::status()`,要一个跑着的 server 才能构造,
压测只想验计数器本身的守恒性,不该为此起一整个服务。

> **`LISTENING` 是进程级 static,而锁只挡得住本模块。** 模块内用一把
> `LISTENING_TEST_LOCK` 串行化,但另一头 `app_settings` 那批用例会经
> `get_agent_launch_spec_from_settings` → `is_listening_on` 读同一个 static,
> 期望「没有服务在跑」。所以这批统一用 **43991+** 这段端口:既不是默认端口
> `DEFAULT_LOCAL_ROUTER_PORT`(15721),也不在那批用例写的 80 / 19090-19092 里,
> `is_listening_on` 对它们仍然如实返回 false。**新增用例请继续用这段端口。**

> **`in_flight_requests_are_visible_while_they_are_still_running` 是变异测试逼出来的。**
> 第一版只有那条"守恒"用例,它在全部收尾后断言 `active == 0` —— 而 **0 正是这个计数器
> 从未被累加过时的同一个值**。实测:把 `begin_request` 里的 `active_requests.fetch_add`
> 整行删掉,9 条里 8 条照样绿(在途恒为 0,收尾时 `saturating_sub` 又夹在 0)。
> 后果是健康面板的「在途请求」永远显示 0,排查卡住的请求时完全没有信号。
> 修法是拿栅栏把 8 个线程停在"已 begin、未 finish"的中间态上观测一次。
> 这是 [[fake-clock-base-leaks-into-ui]] 的同一个形状:**哨兵值和真实值撞在一起**,
> 断言就失去区分度。

**变异测试 11 个全部杀掉,零存活**(begin 不累 total / 不累 active、成功失败反转、
断开顺手计失败、断开不回收在途、`set_error` 丢 agent、取号不自增、`clear_listening`
无条件清空即原始 bug、代号判断反转、`is_listening_on` 忽略端口、饱和减改回绕)。

**验证**:`cargo test` **1953 通过 0 失败**;`cargo clippy --all-targets` 在
`local_router` 侧**零告警**(顺手修掉自己引入的一条 `is_multiple_of` 建议;
剩余 5 条全在 `notebook/export.rs`,属并行会话禁区,未动);
`rustfmt --edition 2021 --check` 干净;relay `cargo test` 21 通过。

**⑤ `src-tauri/src/local_router/server.rs` 追加 `tests::stress_tests`(6 测试)**

**这一批是真起 loopback 服务、真发 HTTP 请求**(沿用既有 harness 的 `start_mock_upstream` /
`unused_port` / `temp_database_path`),断言的是账目守恒。嵌在既有 `mod tests` **里面**
而不是并列,为的是直接吃现成的 helper,不必放宽它们的可见性 —— 那会改到既有测试代码。

被守的是两层套着的 RAII 记账守卫:`RequestCompletion`(`new()` 里 `begin_request`,
`complete()` / `Drop` 收尾)外面又套 `ProxyBodyState`(流式 body 的 `Drop` 补
`release_neutral`)。**每条退出路径必须恰好收尾一次**:漏一次在途数永久飘高,多一次失败率虚增。

| 用例 | 钉住的性质 |
|---|---|
| `concurrent_real_requests_keep_the_metrics_ledger_balanced` | 64 路并发真请求:全 200、成功计数 == 64、失败 0、无错误横幅;落库 64 行且 `request_id` 不重复 |
| `in_flight_requests_are_visible_and_do_not_block_the_health_probe` | 栅栏把 8 个请求停在上游处理器里,此刻在途数 == 8 **且 `/health` 必须秒回** —— 代理路径上有全局锁的话健康探针会跟着卡住 |
| `concurrent_failovers_do_not_lose_or_double_count_attempts` | 首选永久 503、次选健康,32 路并发全部 200;**转移成功不算请求失败**(失败计数 0),落库的 `target_id` 是最终服务的那个 |
| `concurrent_upstream_failures_are_all_accounted_for` | 单目标恒 500,32 路并发失败计数**正好** 32,状态码原样透出,错误横幅带上游原因 |
| `clients_walking_away_mid_stream_do_not_pollute_the_failure_rate` | 流式中途丢掉 Response:在途回收、失败 0、成功 0、**无错误横幅**,落库行带 `CLIENT_ABORT_SUMMARIES[0]` |
| `a_request_cancelled_before_the_upstream_answers_still_balances_the_ledger` | 上游挂 30 秒、客户端 300ms 超时走掉:走**外层** `RequestCompletion::drop`,落库行带 `CLIENT_ABORT_SUMMARIES[1]` |

> **计数器稳定 ≠ 请求行已落库。** `finalize_request` 先 `finish_client_abort()` 再
> `usage_store.insert(...).await`,中间隔着一次 await。所以在 `Drop` 那条 spawn 出去的
> 路径上,`active_requests` 归零时 insert 还在飞 —— 实测 6 个断开请求只读到 3 行。
> 因此有两个轮询等待器:`wait_until_settled`(等计数器)和 `wait_for_rows`(等落库)。
> 断言落库内容的用例必须用后者,别拿前者的结论当落库完成。**两个都不用固定 sleep** ——
> 收尾时刻不确定,写死要么 flake 要么拖慢。

**变异测试 15 个,13 杀 2 存活。两个存活各有独立结论,都不是"缺测试"。**

先说**两个被我自己冤枉过的**:M3/M4(从 `CLIENT_ABORT_SUMMARIES` 名单里各去一条)
第一轮报 COMPILE_FAIL,原因是那个常量声明成 `[&str; 2]`,去掉一项类型就不对 ——
是我的变异写错了,不是等价变异。改成同时把长度改到 1 之后**两个都被杀**。

| 存活 | 结论 |
|---|---|
| M6 `success` 不看状态码 | **等价变异,已用探针证实。** 要区分需要"`error_summary == None` 且状态码不在 200..400",而 `error_summary` 只在状态码 < 400 时为 `None`(server.rs:569 那道 `>= 400` 分流),于是只剩 1xx 一条路。写了个 199 上游的探针:**hyper 根本不把它当终态**,到路由这里已经是传输错误(500 + 摘要)。不可达 |
| M11 `ProxyBodyState::Drop` 的 `finalized` 守卫 | **冗余但刻意保留,已在代码里写明。** `finalize_success` / `finalize_failure` 都无条件 `stream_completion.take()`,所以紧跟着那句 `let Some(..) = take() else { return }` 已经拦住重复收尾。但这个标志同时被 `streaming_response` 的 unfold 循环读(`upstream_finished && !finalized`),那处承重;让"已收尾"在每条出口自己成立比依赖"另一处恰好把 Option 取空"更稳 |

> **M9/M10 一开始存活,而问题在我的断言不在代码。** 两个变异分别把
> `RequestCompletion::drop` 和 `ProxyBodyState::drop` 打哑,却都全绿。原因:流式响应下
> `RequestCompletion` **装在** `ProxyBodyState.stream_completion` 里 —— 打哑外层,内层随之
> 自然析构并接手收尾,只是写下另一条文案;而我原来的断言是
> `assert!(is_client_abort_summary(summary))`,**接受名单里任意一条**,于是照样绿。
> 差别在于那条路径**不调 `release_neutral`**,半开令牌就漏了。
> 改法:把两条路径各自钉到**具体那一条文案**(`CLIENT_ABORT_SUMMARIES[0]` / `[1]`),
> 并补上第 6 条用例专门覆盖外层守卫(上游回话前取消)。改完 **M9/M10 各被对应的那条用例杀掉**。
> 这是 [[mutation-testing-exposes-vacuous-assertions]] 的第三类(断言子串/集合而非具体值)
> 在后端的一次重演:**先改断言,再考虑加测试。**

> **⚠️ 变异 runner 的一个方法论 bug,已修,后来人别踩。** 判"编译失败"原来写
> `"error[E" in out` —— 这个串会命中**并行会话**正在改的 notebook 文件的编译错误,
> 于是我的变异被冤枉成等价变异(第一轮 8 个 COMPILE_FAIL,手工复现 M14 却是 killed)。
> 修法两条,都在 `/tmp/mutate_server2.py` 里:①编译失败要求错误**落在被变异的那个文件**
> (按 `--> <target>` 匹配),否则判环境错误并重试;②每轮前先 `--no-run` 探针,
> 树本身编不过就等。**在共用工作区里做变异测试,必须能区分"我的变异坏了"和"别人的树坏了"。**

**验证**:`cargo test --lib local_router` 112 通过;我的两块单独跑 6 + 9 通过;
`cargo test` 全量 **2099 通过 0 失败**(计数在涨,并行会话在加测试);
`cargo clippy --all-targets` 在 `local_router` 侧**零告警**;`rustfmt --check` 干净;
relay `cargo test` 21 通过;**我的块连续重复 10 次全绿**(时序敏感,必须验重复稳定性)。

> **全量跑偶发 1 个 notebook 失败,已证实与我无关。** 同一个固定二进制做 A/B:
> **排掉我的块跑 30 轮,失败 1 轮**(`notebook::sync::git`),含我的块 18 轮失败 1 轮 ——
> 同量级,不是我引入的。机制也对得上:`run_git_network` 走真实 `git` 子进程,
> 失败时 `Err("")` 是**空**的(stdout/stderr 都空),那是子进程被杀,而我的用例
> 一个子进程都不起。另一次失败在 `notebook/import/enex_tests.rs`,mtime 比我的运行只早
> 5 分钟,且测试总数在我循环期间从 2100 涨到 2102 —— 并行会话正在往里加测试。
> 判据仍是 [[shared-worktree-with-parallel-session]]:**先比 mtime,再当成自己的 bug。**

**这一块还剩什么**:`server.rs` 的并发面已覆盖记账 / 断开 / 转移 / 失败四类;
真实 GUI 交互在 vitest 范围外,仍需人工清单。

## 6. 复现命令

```bash
# 本轮新增的三个测试文件(应 169 通过:34 + 41 + 94)
pnpm exec vitest run src/test/branch-bar.test.tsx src/test/mcp-panel.test.tsx \
  src/test/file-extension-predicates.test.ts

# PromptEditor(应 175 通过)+ 单文件覆盖率
pnpm exec vitest run src/test/prompt-editor.test.tsx
pnpm exec vitest run src/test/prompt-editor.test.tsx --coverage \
  --coverage.reporter=text \
  --coverage.include='src/components/new-task/PromptEditor.tsx' \
  --coverage.thresholds.lines=0 --coverage.thresholds.functions=0 \
  --coverage.thresholds.branches=0 --coverage.thresholds.statements=0

# App.tsx 的 remote-task-request 分片(应 22 通过)
pnpm exec vitest run src/test/app-remote-task-request.test.tsx

pnpm exec tsc --noEmit
pnpm exec eslint src --max-warnings 0
pnpm exec prettier --check "src/**/*.{ts,tsx}"

# 全局覆盖率基线(必须排掉禁区那个文件,否则它失败会导致报告不生成)
pnpm exec vitest run --exclude "src/test/notebook-panel.test.tsx" --coverage

cd src-tauri && cargo test && cargo clippy --all-targets
# 注意:**别跑 `cargo fmt --check`**,它是整 crate 的,会连并行会话的禁区文件一起报。
# 只校验自己动过的文件:rustfmt --edition 2021 --check src/local_router/mod.rs ...

# 本轮五块压测(应 8 / 6 / 9 / 6 / 11 通过)
cargo test --lib remote::auth_stress_tests
cargo test --lib local_router::circuit_breaker::stress_tests
cargo test --lib local_router::metrics_stress_tests
cargo test --lib local_router::server::tests::stress_tests
cd ../remote-relay && cargo test rate_limit::stress_tests
```

`server.rs` 那块**时序敏感,必须验重复稳定性**,单次绿不算:

```bash
for i in $(seq 1 10); do cargo test --lib local_router::server::tests::stress_tests \
  2>&1 | grep '^test result'; done
```

`local_router::metrics_stress_tests` 的变异 runner 是 `/tmp/mutate_metrics.py`
(清单内联,11 个模式,备份 `/tmp/mod_rs_backup_metrics.rs`;filter 收窄到
`local_router::metrics_stress_tests`,免得让别的既有测试替我"杀掉"变异)。

`server.rs` 那块用 `/tmp/mutate_server.py`(15 个模式,**跨两个文件**变异 ——
server.rs + mod.rs 各自独立备份)。**修过一次方法论 bug 的版本是
`/tmp/mutate_server2.py`,以后拿它当模板**:它区分"我的变异编不过"和"并行会话的树编不过"
(要求错误行 `--> ` 落在被变异文件里,否则重试),并在每轮前用 `--no-run` 探针等树变干净。

变异测试:`python3 /tmp/mutate.py <config.json>`,配置格式见 `/tmp/mut_mcp.json`。
它会**先断言基线全绿**、跑完**按字节校验复原** —— 这两步别绕过。
被打断的脚本会留下变异文件,下一轮当成基线,整轮结论作废。

PromptEditor 轮换用了两个更小的 runner:`/tmp/mut.py`(模式走命令行参数)与
`/tmp/mutf.py`(模式从文件读,给含字面 `\n` 的模式用)。三条硬约束,少一条结论就不成立:
- **替换前断言模式在全文命中恰好一次。** 短模式会撞上同文件另一处,报出假「存活」。
- **不要用 perl。** 模板字符串里的反引号 / `${}` 让 `-0pi` 反复报 NO-OP;
  Python 字面 `str.replace` 才稳。
- **不要广撒 `unicode_escape`。** 它会把 `\&` 也解掉,把源码弄成语法错误 ——
  两个变异因此报 INDETERMINATE(**是构建失败伪装成变异结果,不是存活**)。
  只窄解 `\n` 和 `\uXXXX`。不可见字符(nbsp / ZWSP)必须以 `\uXXXX` 传参,
  字面字符会被 shell argv 吃掉。

`remote-task-request` 轮用 `/tmp/mut_remote.py`(变异清单**内联在脚本里**,
24 个多行模式;每轮复原后校验 sha256,汇总行缺失判 ERROR 而不判存活)。
这个形态最省事:模式带缩进和多行时,写进 Python 三引号比塞 JSON 转义清楚得多。

FileExplorer 轮用 `/tmp/mutfe.py`(变异清单走 JSON 文件,`old`/`new` 里直接写 `\n`,
两个 spec 一起跑,复原后校验 sha256):

```bash
# 126 通过 / 与既有 ui 文件合跑 132
pnpm exec vitest run src/test/file-explorer-fs-actions.test.tsx
pnpm exec vitest run src/test/file-explorer-fs-actions.test.tsx \
  src/test/file-explorer-ui.test.tsx --coverage --coverage.reporter=text \
  --coverage.include='src/components/FileExplorer.tsx' \
  --coverage.include='src/components/file-explorer/**'
# 变异(先确认 src/components/FileExplorer.tsx 与 /tmp/FileExplorer.orig.tsx 同 sha)
python3 /tmp/mutfe.py /tmp/mfe5.json
```

## 7. 踩过的坑(会再咬人的)

- **⚠️ 工作区和另一个会话共用,禁区外的失败先比 mtime 再当成自己的 bug。** 本轮实测两次:
  14:41 `cargo test --lib` 1858 全绿 → 14:57:53 对方改了 `notebook/sync/remote.rs`
  → 14:58 我再跑 1 个失败,失败点就在那个文件里 → 单独重跑它又通过。
  第二次同样的剧本发生在 `notebook/sync/local.rs`(15:01:42 改,15:02 我的跑挂 4 条)。
  判据:`stat -f "%Sm" -t "%F %T" <文件>`,mtime 落在「上次绿跑」和「本次跑」之间就先重跑。
- **⚠️ 会话开头那份 git status 快照是陈旧的,别拿 HEAD 当基准。** 快照只列了 2 个 Rust 文件
  被改,实际 `git status` 有 37 个 —— 长任务跨了很多次压缩,快照来自最初那次对话。
  于是 `git show HEAD:src-tauri/src/dsh_webui.rs | wc -l` 是 **7141**,而我开工前工作区是 **5429**。
  拿 HEAD 当基准会把别人(和更早几轮)的工作算进自己的账上。要报改动量就报
  「工作区前后」,并说明 HEAD 差异里混了别的东西。
  **这也说明 §1 早前那句「唯一拆过 `app_settings.rs`」是错的** —— `dsh_webui.rs`
  在我开工前就已经被拆过一轮(7141→5429,抽出 `build_readiness` / `event_stream` /
  `terminal_render`),只是没记进本文。

- **假时钟必须在 render 之前装。** `setInterval` 在 effect 里建立,render 之后才
  `useFakeTimers()` 拿到的是真时钟上的 timer,`advanceTimersByTime` 永远推不动它 ——
  于是「卸载后没有多打 IPC」这类断言**无条件通过**。本轮三个轮询测试全中过这个,
  已用 `renderWithFakeTimers()` helper 兜住,并在断言「没增加」之前先断言
  「这个时钟确实能推动轮询」。
- **大小写不敏感的测试要用带大写的数据。** 搜 `"LOGIN"` 去匹配 `feature/login` 是空断言:
  `q` 已经小写,少一次 `name.toLowerCase()` 照样命中。分支名必须带大写(如 `Feature/JIRA-42`)。
  ⚠️ **这条我在 FileExplorer 上又踩了一次** —— 搜 `"readme"` 匹配 `README.md`,
  少一次 `query.toLowerCase()` 全绿。方向要搞对:**查询词带大写、数据小写**才有区分度。
- **派生状态钉不住它派生自的状态。** 面包屑、行高亮、`currentDirectoryPath` 全是从
  `findNode(nodes, selectedPath)` 推出来的,节点一消失就自动回落到项目根 —— 于是
  「压根不清 `selectedPath`」这个变异**从这些出口看完全一样**(FileExplorer 上有 4 个变异
  因此存活:删除清选中、换根清选中、切项目清选中各一)。要看见差别,得找一个**直接读原始
  state** 的出口:复制路径的 `if (!selectedPath) return` 就是。写「某个 state 被清了」这类
  断言之前,先问一句「我看的这个东西,是不是它自己也会在同样条件下变成一样的值」。
- **活着的轮询会替你的代码通过测试。** 2500ms 的自动刷新落在 `waitFor` 的 3000ms 窗口里,
  于是「删完不调 refresh」「粘完不调 refresh」这些变异全绿(FileExplorer 实测)。
  凡是断言「**立刻**做了某事」,都要上假时钟并且**只冲 microtask、一毫秒都不推进**。
  同理:断言「**没**装轮询」必须真的把时钟推过去(`active === false` 那条变异就是靠这个才杀掉的)。
  更一般地 —— 一个变异存活时先问「有没有第二条路会把结果补对」:`sortTreeNodes` 的递归
  就是被 refresh 每层自带的 `sortFileEntries` 兜住的,只有在**重读失败**的窗口里才独自生效。
- **`waitFor` 超时是 3000ms**(`src/test/setup.ts:10` 的 `asyncUtilTimeout`),不是 RTL 默认的 1000。
- **同名按钮要用 `within(dialog)` 限定**;文案会变的(Save → Saving...)按前缀匹配,
  否则元素在中途「消失」。
- **覆盖率报告在任一测试失败时不生成**(vitest 提前退出)。text reporter 会截断长路径
  (`src/...ojectState.ts`),要重建路径就直接解析 `coverage/lcov.info`。
- **`notebook-panel.test.tsx` 的 `跨文件重命名 > 改完重扫` 失败不是我们的。**
  7374 行 / 379 测试的单文件累积退化把重扫推过 3000ms(子集 ~0.6s/测试,全量 4.9s/测试,
  单跑 3/3 通过)。**含禁区的全量跑已确认它就是唯一那条失败**(310 文件 / 4557 测试 /
  1 失败,耗时 1072s,断言实况是 `tagNames()` 停在 `["#work, 1 uses in 1 notes"]`)。
  归属证据:`grep -c 'noteSheetChrome' NotebookPanel.tsx` → 0。
  修它要动禁区文件。**教训:别把测试堆成一个大文件** —— App.tsx 的测试从一开始就按
  关注点分文件(初始化 / 项目切换 / 全局事件 / 窗口生命周期各一个)。
- **CPU 争抢会污染计时结论。** 曾用 `(while :; do :; done) &` 造负载后
  `kill $(jobs -p)` 没杀掉(不同 eval 上下文),8 个忙循环活了 35 分钟,
  两次「干净」的计时全在 load 46 下跑出来,结论正好反过来。另外别的 Claude 会话
  也可能同时在跑 `pnpm test`(查 PPID 链确认过一次)。
- **仓库在 `同步空间/` 云同步目录下**,目录读取会偶发挂住并返回**空输出** ——
  空输出不是结果。本次交接被这个打断过两次。
- **`use super::*` 拿不到兄弟模块的 `pub(crate)` 项**,要在父模块显式转一手;
  只被 `#[cfg(test)]` 用到的转发要加 `#[cfg(test)]`,否则 lib 构建报 unused import。
- **拆 Rust 大文件时依赖清单不要手挑** —— 我手挑漏了 6 个符号(`fs`、
  `agent_scripts_dir`、4 个 `append_*_env`),21 + 27 个编译错误。靠全文扫描 + 编译器输出枚举。

## 8. 文件位置

- 备份:`/tmp/mine-backup/` —— `styles/`(14 个)、`app_settings.rs.orig`、
  `McpPanel.tsx.orig`、`BranchBar.tsx.orig`、`fileExtensions.ts.orig`、`notebook/`。
- 脚本:`/tmp/mutate.py`(通用变异 runner)、`/tmp/mut_*.json`、`/tmp/style_audit.py`、
  `/tmp/parse_cov.py`、`/tmp/check_windows_syms.py`、`/tmp/mut.py` + `/tmp/mutf.py`
  (PromptEditor 轮用的小 runner)、`/tmp/PromptEditor.orig.tsx`(备份)、
  `/tmp/mutfe.py` + `/tmp/mfe*.json`(FileExplorer 轮,两个 spec 一起跑)、
  `/tmp/FileExplorer.orig.tsx` + `/tmp/ContextMenu.orig.tsx`(备份)。
  `/tmp` 会被系统清掉,当交接材料看别当依赖。
- 长期记忆:`~/.aeroric/agent-homes/sota_claude/projects/-Users-macbook-Downloads------LYX-Aeroric/memory/`,
  索引 `MEMORY.md`。与本任务直接相关的:「假时钟基准会漏到 UI 上」
  「变异存活多半是空断言」「一批变异同时存活＝多道闸门互相兜底」
  「CPU 争抢会污染计时结论」「变异测试基线必须先全绿」
  「变异的 perl 模式必须锚到唯一处」。
