# 共享状态污染清单(16 条)

> **状态字段**：每条发现的首行以 `status: <值> | ` 开头；已到终态的条目在下方另有
> 一行 `> 复核(<日期>):<证据或理由>`，把结论和依据放在一起。
>
> - `open` —— 仍需人工复核（**不等于「有问题」**，请勿当成缺陷计数）。本清单已无 `open`。
> - `fixed` —— 已核实修复。判定依据是**读代码**（现场是否已改成真断言、是否已补 `try/finally`、
>   mock 是否已还原、是否已改用确定性等待……），**不是**「用例名还在不在」——后者会大面积误判：
>   一轮 49 条 `open` 里有 39 条其实早已修好，只是用例还在、内容被改写成了真断言。
> - `wontfix` —— 经判断不改，条目内写明理由（允许把原判断修正得更精确，但必须给依据）。
>
> 取值口径见 `docs/history/PROJECT_REVIEW_2026-09-18.md` §3.1 与 §11。
> 本清单终态统计（2026-09-18 复核）：`fixed` 15 / `wontfix` 1 / `open` 0，共 16 条；67 条全部附有复核行（0 条缺）。
> 四份清单合计：`fixed` 59 / `wontfix` 8 / `open` 0，共 67 条。

## src/test/project-rail-drag.test.tsx:463

status: fixed | 【重要 · 真实顺序依赖】what: 在 it 内 localStorage.setItem("aeroric:language", "zh")。cleaned: 无 —— 整个文件【没有任何 beforeEach / afterEach】,全文搜不到 localStorage.clear()。impact: src/i18n.tsx:50 的 effect 每次 I18nProvider 挂载都把当前语言写回 localStorage,所以 463 行之后语言一直是 zh,直到 527 行才被改回 en。夹在中间的 491 号用例("opens the Agent terminal initial page...")自己不设语言:全量跑拿到 zh,单跑时 getInitialLanguage() 回落到 navigator.language(jsdom 恒为 "en-US" → en)。它当前只按 { name: "Beta" }(项目名,不本地化)查询所以没红。真正的隐患在 297 号用例("opens the agent settings section"):它按英文字面量 { name: "Agent settings" } 查询且同样不设语言,现在只是因为排在 463 之前、继承了 208 行留下的 en 才通过 —— 把 zh 用例上移一条、或在它前面新增任何 zh 用例,297 立刻红。修法:文件加 beforeEach(() => localStorage.clear())。
> 复核(2026-09-18):已修:文件顶部(:40-41)新增 `beforeEach(() => { localStorage.clear(); … })`,语言不再跨用例继承,297 号用例不再依赖「自己前面恰好是 en」这个偶然顺序。

## src/test/shell-terminal-panel.test.tsx:339

status: fixed | 【重要 · 掩盖回归】what: vi.spyOn(console, "error").mockImplementation(() => {}) 把 console.error 静音;730 行同一形状。cleaned: 仅 344 / 736 行的 errorSpy.mockRestore(),【不在 try/finally 里】,且整个文件【没有 afterEach】(只有 138 行 beforeEach 做 useFakeTimers + 重置 hoisted 状态),配置也没开 restoreMocks。impact: 340-343(或 731-735)之间任一断言抛出,console.error 就对该文件【剩下的所有用例】永久静音。这个文件测终端面板的挂载/销毁/dispose,React 的 "not wrapped in act"、重复 key、effect 抛错全走 console.error —— 静音后这些回归在 CI 里彻底看不见。一处失败放大成整文件失明。修法:改成 try/finally,或补 afterEach(() => vi.restoreAllMocks())。
> 复核(2026-09-18):已修:两处 spy(:348 / :743)都包进 try/finally;另外文件级 afterEach(:149)本就有 vi.restoreAllMocks()。原条目写的「全文没有 afterEach」是陈旧描述,已同步改正 :343 处的注释。

## src/test/stall-recorder.test.ts:52

status: fixed | 【重要 · 冻结时钟】what: afterEach 只有 vi.unstubAllGlobals() + vi.useRealTimers(),【缺 vi.restoreAllMocks()】;而 58/80/96/117/137/174/194/230/255/277/296 行反复 vi.spyOn(performance, "now").mockImplementation(() => now),每个 spy 闭包捕获各自的 let now。cleaned: 无,只靠「每个用例自己再 spy 一次」覆盖。impact: 213 号用例("survives the real tauri internals descriptor")自己不 spy performance.now,继承的是 194 号用例结束时冻结在某个值的时钟;它只断言 invokeProbeActive 这个布尔量所以现在没红。这个文件测的正是【耗时】:任何新增的、不自己 spy 时钟的用例都会拿到冻结的 performance.now,所有时长算成 0,slowInvokes 恒为空,"没有慢命令" 类断言变成永真。修法:afterEach 补 restoreAllMocks。
> 复核(2026-09-18):已修:afterEach(:51-59)补上 vi.restoreAllMocks(),performance.now 的冻结时钟不再跨用例泄漏,213 号用例拿回真实时钟。

## src/test/file-explorer-fs-actions.test.tsx:1415

status: fixed | 【次要 · 仅理论风险,方向明确】what: Object.defineProperty(document, "execCommand", { configurable: true, value: vi.fn().mockReturnValue(false) });2342 行重复同一动作。cleaned: 【无】。267-281 行的 beforeEach 会在 276 行重装 navigator.clipboard,但从不还原或删除 execCommand;282-284 行的 afterEach 只有 vi.useRealTimers()。impact: 1412 号用例之后,document.execCommand 对该文件剩下约 60 个用例都是「恒返回 false 的 vi.fn」。当前后续用例走 clipboard 成功路径、够不到兜底分支,所以没红 —— 仅理论风险。但任何新增的、想验证「execCommand 兜底成功」的用例,全量跑拿到前面留下的 false 桩子、单跑拿到 jsdom 的未实现版本,两种跑法结论不同。修法:改用 vi.spyOn 或在 finally 里 Reflect.deleteProperty。
> 复核(2026-09-18):已修:afterEach(:282-288)在 useRealTimers() 之后补了 Reflect.deleteProperty(document, "execCommand"),把「值为 undefined 的自有属性」彻底摘掉,而不是留一个恒 false 的桩子。

## src/test/notebook-attachment-drop-hook.test.tsx:113

status: fixed | 【次要 · 仅理论风险】what: 在 it 内 Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true })。cleaned: 【无】。63-85 行的 beforeEach 只重置 dropListener / invoke / unlisten,不碰 devicePixelRatio。impact: jsdom 默认 devicePixelRatio = 1;这条用例之后同文件所有用例都在 DPR=2 下跑。后续用例("系统拖入在没有打开笔记时静默忽略"、"落在编辑器外就整个忽略"、"只认图片扩展名" 等)传的落点是 { x: 1, y: 1 },被测代码换算成 (0.5, 0.5) 而非 (1, 1);它们只断言 invoke 有没有被调,所以现在看不出来。这个文件专测坐标换算 —— 后续任何一条断言具体坐标的新用例都会单跑/全量跑不一致。
> 复核(2026-09-18):已修:文件级 afterEach(:89-91)把 devicePixelRatio 还原回 jsdom 默认的 1,坐标换算不再跨用例继承 DPR=2。

## src/test/terminal-highlight.test.ts:266

status: fixed | 【次要 · 失败时改变后续被测路径】what: Object.defineProperty(navigator, "scheduling", { configurable: true, value: { isInputPending: () => inputPending } })。cleaned: 283 行 Reflect.deleteProperty(navigator, "scheduling"),【在断言之后、不在 finally 里】;284 行的 vi.useRealTimers() 同样在 finally 之外。impact: 279-281 的断言一旦失败,navigator.scheduling 带着 isInputPending 泄漏给同文件后面的 describe("cursor line highlight overlay")。被测的 createSmartWriter 会因此走「浏览器有待处理输入」的让路分支而不是默认分支 —— 一条失败连带改变后续用例走的代码路径,失败信息会指向错误的地方。
> 复核(2026-09-18):已修:该用例改成 try/finally,Reflect.deleteProperty(navigator, "scheduling") 与 vi.useRealTimers() 都在 finally 里(:283-288)。另补了文件级 afterEach —— 本文件十处 useFakeTimers 大多把 useRealTimers 写在用例末尾,断言一抛就还不回真实时钟,整类泄漏一次性闭合。

## src/test/resizeObserverStub.ts:13

status: fixed | 【次要 · setup.ts 装进去的唯一跨用例可变单例】what: 模块级 const entries = new Set<StubEntry>(),triggerResize() 遍历它同步派发;installResizeObserverStub() 由 setup.ts:143 对全部 360 个文件执行(jsdom 确实不实现 ResizeObserver,已确认 interfaces.js 无该接口,所以守卫必然放行、桩子必然装上)。cleaned: 【无】任何 beforeEach / afterEach 清空,只靠被测组件卸载时 disconnect() 自摘。impact: 目前 src/components 下所有 new ResizeObserver 的 effect 都在 cleanup 里 disconnect(FileExplorer.tsx:354、GitChanges.tsx:109、ProjectPage.tsx:1476、TerminalView.tsx:410、terminalRuntime.ts:156、useNoteLayoutTier.ts:63、TaskList.tsx:104、UsageDashboard.tsx:564、AnimatedSelection.tsx:146),所以现在不漏。两层真实风险:(a) 任何漏 disconnect 的新组件会让后续用例的 triggerResize() 打到已卸载元素上,而 notebook-layout-tier-hook.test.tsx:69 那条「卸载后断开观察」正是靠 measureCount 不涨来判定 —— 前面漏下的 entry 会让它误判;(b) 替身对同一 target 重复 observe() 【不去重】(每次 entries.add({target, callback}) 都是新对象),真实 ResizeObserver 按 target 去重,语义有偏差。修法:导出一个 resetResizeObserverStub() 供需要的文件在 beforeEach 调用。
> 复核(2026-09-18):已修:按建议导出 resetResizeObserverStub(),由 setup.ts 的文件级 afterEach 在每个用例之后调用。漏 disconnect 的组件不再把 entry 带到下一个用例,notebook-layout-tier-hook 那条「卸载后断开观察」不会再被前面的残留 entry 误判。

## src/test/terminal-runtime.test.ts:94

status: fixed | 【次要 · 仅理论风险,但同形状有 4 个文件】what: vi.spyOn(window, "requestAnimationFrame").mockImplementation(...) 写在 beforeEach 里(92 行还有 vi.stubGlobal("ResizeObserver", ...))。cleaned: 100-102 行的 afterEach 只有 vi.useRealTimers(),【无 restoreAllMocks / unstubAllGlobals】。impact: 每个用例的 beforeEach 都在上一个 spy 之上再套一层,window.requestAnimationFrame 整个文件回不到真实实现,层数随用例数线性增长,单个 mockRestore 只剥一层。当前每条用例都自己重设 animationFrames = [] 所以行为一致 —— 仅理论风险。同一形状还有:terminal-theme-replay.test.tsx(120 行 stubGlobal + 215 行 spyOn window.cancelAnimationFrame)、running-view-resume.test.tsx:69、running-view-session-heal.test.tsx:91 —— 这三个文件【连一个 afterEach 都没有】(已 grep 确认 afterEach / restoreAllMocks / unstubAllGlobals / useRealTimers 全部零命中)。
> 复核(2026-09-18):已修:afterEach 改为 rafSpy.mockRestore() + vi.unstubAllGlobals()(rafSpy 用变量持有引用)。刻意不用 vi.restoreAllMocks() —— 它会连带抹掉模块级 vi.fn() 的实现。同形状的三个文件(terminal-theme-replay / running-view-resume / running-view-session-heal)也各自补了 afterEach(() => vi.unstubAllGlobals())。

## src/test/notebook-panel-links.test.tsx:258

status: fixed | 【次要 · 属性形状变化】what: 248 行 const original = Element.prototype.scrollIntoView → 覆盖成记录用的函数 → 258 行在 finally 里 Element.prototype.scrollIntoView = original。cleaned: 有,且正确放在 finally 里(写法本身是对的)。impact: 但 jsdom 不实现 scrollIntoView(已确认 Element-impl.js 无此方法),所以 original 是 undefined,「还原」实际是在 Element.prototype 上【留下一个值为 undefined 的自有属性】,而原先根本没有这个属性 —— "scrollIntoView" in Element.prototype 由 false 翻成 true。仓库生产代码用的是 typeof node.scrollIntoView === "function" 判定(DshTriggerMenu.tsx:64、NoteCommandPalette.tsx:88、NoteTriggerMenu.tsx:151),对 undefined 与缺失一致,所以当前无影响;但 DshTrajectoryLedger.tsx:192 是【无保护】调用 rowNodes.current.get(seq)?.scrollIntoView(...),该组件若进入这个文件就会因「属性存在但不是函数」而抛。修法:finally 里改成 delete Element.prototype.scrollIntoView。
> 复核(2026-09-18):已修:finally 里按 original 是否存在分两种还原 —— 本来有就还回去,本来没有就 Reflect.deleteProperty,不再留下「值为 undefined 的自有属性」。

## src/test/agent-usage-store.test.tsx:141

status: fixed | 【次要 · 失败时掩盖】what: const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})。cleaned: 154 行 consoleError.mockRestore(),【不在 try/finally 里】;42-45 行的 afterEach 有 useRealTimers + resetAgentUsageCacheForTests,但【无 restoreAllMocks】。impact: 142-153 之间断言失败则 console.error 对该文件后续用例持续静音,后面的 React 警告与 store 错误路径日志一起消失。规模比 shell-terminal-panel 小,但形状相同。
> 复核(2026-09-18):已修:spy 包进 try/finally;文件级 afterEach(:42-48)另补 vi.restoreAllMocks() 作双保险。原注释「全文没有 afterEach」已改正。

## src/test/notebook-sync-hook.test.tsx:376

status: fixed | 【次要 · 失败时掩盖】what: const warn = vi.spyOn(console, "warn").mockImplementation(() => {})。cleaned: 389 行 warn.mockRestore(),【不在 finally 里】;98-100 行的 afterEach 只有 vi.useRealTimers()。impact: 377-388 之间失败则 console.warn 对文件剩余用例静音。同文件其余用例测的是同步失败路径,警告是主要观察面 —— 静音后 "跑一轮失败时报出来"(392 行)之类用例的诊断价值下降。
> 复核(2026-09-18):已修:spy 包进 try/finally;文件级 afterEach(:98-103)另补 vi.restoreAllMocks() 作双保险。原注释「全文没有 afterEach」已改正。

## src/test/file-explorer-fs-actions.test.tsx:263

status: fixed | 【次要 · 仅理论风险】what: watchErrors() 里 window.addEventListener("error", onError),把 stop() 作为返回值交给调用方。cleaned: 调用点(如 623 行取 watch、639 行 watch.stop())把 stop() 放在断言【之后】,不在 finally 里。impact: 断言失败时 error 监听器泄漏,继续往一个已经没人读的 errors 数组里 push —— 对其他用例的断言结果无影响,属惰性泄漏。列出是因为同文件已有 267/282 的 beforeEach/afterEach 骨架,把 stop 挪进 afterEach 是顺手的事。
> 复核(2026-09-18):已修:watchErrors() 把 stop 登记进模块级 activeErrorWatchers,由文件级 afterEach 统一摘除,不再依赖调用点把 stop() 写在断言之后。

## src/test/project-rail-drag.test.tsx:299

status: fixed | 【次要 · 仅理论风险】what: window.addEventListener("aeroric:open-app-settings", listener)。cleaned: 331 行 removeEventListener,【不在 try/finally】,中间夹着 326-330 的断言。impact: 断言失败时监听器泄漏到同文件后续用例;泄漏的是空 vi.fn(),不改变别人的断言结果,惰性。同形状:all-agent-configs-panel.test.tsx:108/132 与 148/164。对照组说明这是漏写而非约定 —— 同仓库 mcp-panel.test.tsx:154-168、cleanup-report-panel.test.tsx:105-119、notebook-panel-history.test.tsx:270-280、app-event-wiring.test.tsx:685-704、notebook-sheet-chrome.test.tsx:248-256 都正确用了 try/finally。
> 复核(2026-09-18):已修:监听器摘除挪进 try/finally。同形状的 all-agent-configs-panel.test.tsx 两处改用 trackAppSettingsChanged() 登记 + 新增文件级 afterEach 统一摘除。

## src/test/databaseViewTestUtils.ts:100

status: fixed | 【次要 · 仅理论风险 + 维护陷阱】what: resetDatabaseViewMocks() 只 removeItem 了 "aeroric:database:pinned-nosql-tree-nodes" 与 "aeroric:database:extra-dbx-connection-groups",不是整体 localStorage.clear();96 行还 Object.defineProperty(navigator, "clipboard", ...) 每次重装(这部分是对的)。cleaned: 部分。impact: 这两个键正好是 databaseViewModel.ts:320-322 导出的全部 database 持久化键,所以对 database 视图【当前是完整的】。但这批文件会渲染带 I18nProvider 的树,i18n.tsx:50 的 effect 会写入 aeroric:language 而这里不清,语言在同文件内一路继承;因为断言都是英文字面量、jsdom 的 navigator.language 恒为 en-US → 默认就是 en,所以现在不出问题。真正的陷阱是新增 database 持久化键时容易忘同步这份 remove 名单 —— 应直接改成 localStorage.clear()。
> 复核(2026-09-18):已修:按建议整体 window.localStorage.clear(),不再维护一份容易忘同步的 removeItem 名单(该名单漏掉了 i18n 写的 aeroric:language)。

## src/test/dsh-trajectory-overlay.test.tsx:24

status: fixed | 【次要 · 隔离模型的行为反证】what: 模块顶层(不在 beforeEach 里)Element.prototype.scrollIntoView ??= () => {}。cleaned: 无,文件内永久生效。impact: 这是【有意的 polyfill】,不是缺陷 —— 因为每个测试文件独占环境,它不会漏到别的文件。恰恰因此,同样渲染 DshTrajectoryOverlay 的 dsh-deliverables.test.tsx:161、dsh-image-attachments.test.tsx:376、dsh-trajectory-timeline.test.tsx:201 三个文件【必须各自再装一遍】,而它们现在都没装 —— 目前不炸只是因为没触发 DshTrajectoryLedger.tsx:191-192 那条「选中行滚动」的无保护调用路径。这三个文件是一个待爆的坑,同时也是「补丁不跨文件」的直接行为证据。
> 复核(2026-09-18):已修:按发现的原意给三个同组件文件各自补了一份同名 scrollIntoView 补丁(dsh-deliverables / dsh-image-attachments / dsh-trajectory-timeline),不再依赖「恰好没走到 DshTrajectoryLedger.tsx:192 那条无保护调用」。四处的注释已交叉引用,便于一起维护。刻意没搬进 setup.ts:那会让全仓 375 个文件里 `typeof scrollIntoView === "function"` 的分支走向改变,爆炸半径远大于收益。

## src/test/omp-hook.test.ts:54

status: wontfix | 【无影响 · 已正确清理,列出以说明 process.env 类别已排查】what: beforeEach 改写 process.env.AERORIC_TASK_ID / AERORIC_EVENT_DIR / AERORIC_AGENT;139 行还有 delete process.env.AERORIC_TASK_ID。cleaned: 有 —— 59-69 行的 afterEach 用文件顶部快照的 saved 逐个还原,值为 undefined 时走 delete 而不是赋空串,写法完整正确。impact: 无。这是全仓库 src/test/ 下【唯一】的 process.env 改动点(已 grep process.env 赋值 / delete / vi.stubEnv 全库确认),加上每个文件独占 worker,env 类污染在本仓库不存在。
> 复核(2026-09-18):无需改动 —— 条目本身已判定「无影响·已正确清理」,列出只为说明 process.env 这一类已排查完。afterEach 用文件顶部快照逐个还原、值为 undefined 时走 delete,写法完整;且是全仓库唯一的 process.env 改动点。保留状态字段为 wontfix 以示「已复核、确认不需要动作」。
