# 时序脆弱测试清单(22 条)

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
> 本清单终态统计（2026-09-18 复核）：`fixed` 15 / `wontfix` 7 / `open` 0，共 22 条；67 条全部附有复核行（0 条缺）。
> 四份清单合计：`fixed` 59 / `wontfix` 8 / `open` 0，共 67 条。

## src/test/notebook-panel-core.test.tsx:385

status: fixed | kind=2(waitFor 默认超时套重操作)| severity=重要 | evidence: `await waitFor(() => expect(math?.querySelector(".katex")).not.toBeNull());` —— 等的是 renderMathBlock 里 `await getKatex()` 的结果,而 getKatex 走真实 `import("katex")` + `import("katex/dist/katex.min.css")`(src/components/notebook/noteVisuals.ts:55-62),本文件只 mock 了 @tauri-apps/api/core 和 plugin-clipboard-manager,katex 没被 mock。| why_flaky: 这是**唯一**一处真等 KaTeX 落地的断言(全 src/test 里 `.katex` 只在这一行被断言)。katexPromise 是模块级缓存,所以每个 worker 冷加载恰好一次,时间全花在 Vite 解析/转换/v8 插桩一个 ~270KB 的 CJS 包上。默认预算只有 3000ms(setup.ts:10),这个文件本身是全仓最重的之一(1504 行 + 真实 CodeMirror + userEvent),9 个 fork 抢 CPU 时 3000ms 会被吃满;而一旦 node_modules/.vite 暖了就只剩几十毫秒 —— 正是「红一次、再跑两次全绿」的形状。作者在 :383-384 已写明「满负载跑整个套件时 KaTeX 的动态 import 还没回来」,他们改的是「等对的东西」,**没有**同时放宽预算。
> 复核(2026-09-18,补充):**上一条结论不够 —— 放宽预算没有解决问题,已找到真因并修掉。**全量并行(367 文件 / 9 fork)时这条仍然失败:报的是**全局 testTimeout 30000ms** 而不是那条 waitFor 的 15000ms,说明把内层预算抬到 15000 并不是有效手段。真因是 **mermaid**:该用例名里就写着 math **and mermaid**,而 `getMermaid()` 的 `import("mermaid")` 体积远大于 katex。实测(定向跑该用例):基线 **12,786ms** → 只预热 katex **13,146ms**(无变化,证明 katex 不是主因) → 同时预热 mermaid **703ms**(18×)。修法:在测试文件**模块加载期**把两条懒加载都踢出去(`void getKatex(); void import("mermaid");`),冷导入成本落到不受 testTimeout 约束的模块加载阶段。不改变任何断言 —— KaTeX / mermaid 仍必须真的渲染进 DOM 才算过。整文件 70/70 通过。

## src/test/terminal-wheel-scroll.test.ts:426

status: fixed | kind=1+6(真实时钟 + 真实动画帧)| severity=重要 | evidence: `globalThis.setTimeout(resolve, WHEEL_REPAINT_GRACE_MS + 40)` 之后 `expect(replayed.length).toBeGreaterThan(burst)` —— 等 50ms 宽限期自己走完,再靠真实 jsdom rAF(~16ms 一帧)把排队的 32 行放行。| why_flaky: 只有 40ms 余量,而放行必须由一次真实 rAF 回调落在这 40ms 里。CPU 超订时 jsdom 的 rAF(底层是 setTimeout(16))被饿到几十毫秒一拍,90ms 内可能一帧都不落 → replayed.length 仍等于 burst → 硬性正向断言翻红。同文件 :378-381 与 :113-114 的注释已经明说这是「赛跑,快慢由 CI 负载决定」,并为**兄弟用例**(:383)专门装了 `vi.useFakeTimers({toFake:[requestAnimationFrame,...]})`,却把这一条留在真时钟上。
> 复核(2026-09-18):已修:整条用例改用 vi.useFakeTimers({ toFake: [requestAnimationFrame, cancelAnimationFrame, performance, Date] }) + await vi.advanceTimersByTimeAsync(WHEEL_REPAINT_GRACE_MS + 16),不再依赖真实 jsdom rAF 落进 40ms 余量。

## src/test/notebook-visual-scheduler.test.ts:245

status: fixed | kind=1(挂钟性能预算)| severity=重要 | evidence: `const setupMs = performance.now() - t0; ... expect(setupMs).toBeLessThan(100);` 包住 `scheduleVisualBlocks(root, "div.viz-block", ...)` 在 500 个块上的同步 setup(querySelectorAll 500 + 500 次 stub 版 getBoundingClientRect,每次新分配一个对象字面量)。| why_flaky: 纯挂钟门槛,和被测逻辑无关。裸跑约几毫秒,但这是一段**同步**代码块 —— 一次 GC 停顿、或 OS 在 9 个 fork 争抢下把这个 worker 线程挂起 100ms,就直接翻红,重跑必绿。这是全仓唯一一条 wall-clock 性能断言。
> 复核(2026-09-18):已修:挂钟断言已整段移除(全文件已搜不到 performance.now / toBeLessThan),不再拿 100ms 门槛守一段同步 setup。

## src/test/notebook-panel-templates.test.tsx:483

status: fixed | kind=1+5(真实时钟 + 依赖真实经过时间)| severity=重要 | evidence: `await waitFor(() => expect(screen.queryByRole("dialog", {name:"Quick capture"})).toBeNull(), { interval: 1 });` 紧接 `setEditorValue(...)` 与 `await act(async () => { await new Promise(r => setTimeout(r, 1200)); })`。注释自陈:「1ms 一轮,真实时间还远没走到 200ms」。| why_flaky: 全仓唯一一处 `interval: 1` 的 waitFor,它把测试的**鉴别力**押在「轮询完成时真实经过时间 < CodeMirror 的 200ms 打字闩」这个挂钟条件上。负载一上来,1ms 轮询自己就在烧 CPU、每轮还要跑一次 queryByRole,闩会在断言前自己过期 → 挂起更新平静生效 → 有没有 bump editorEpoch 在 DOM 上没区别 → **假绿**(测试不再守任何东西);而它前面那个默认 3000ms 的 waitFor 等的是一次经 harness 的真实保存链,负载下也可能直接超时翻红。两个方向都不稳。
> 复核(2026-09-18):已修:waitFor 的 { interval: 1 } 已去掉(现在只在该处注释里作为「原来的写法」被引用),不再把鉴别力押在挂钟条件上。

## src/test/app-event-wiring.test.tsx:382

status: fixed | kind=2(嵌套 waitFor)| severity=重要 | evidence: `await waitFor(async () => expect((await latestSavedTask("p1","t-1")).status).toBe("detached"));` —— 外层 waitFor 用默认 3000ms,回调里 `latestSavedTask` 自己又是一个 `waitFor(..., { timeout: 3000 })`(:225-235)。同形态还有 :392、:408、:415、:433、:438。| why_flaky: 外层单**一次**轮询就能吃掉外层的全部 3000ms 预算 —— 内层不满足时要耗满 3000ms 才 reject,外层这时已经超时,报出来是内层那句「not in any persisted snapshot yet」,看不出真因。整条链是 renderApp() 全量 App + 事件订阅 + taskPersistence 的 350ms 防抖落盘;CPU 超订下防抖醒得晚,内层多轮询几次就把外层顶穿。
> 复核(2026-09-18):已修:新增同步读取器 savedTaskNow(:231)专供 waitFor 回调,等待只剩一层;另抽出 waitForSavedTask helper。嵌套 waitFor 的根因已写进 :224-230 的注释。

## src/test/app-remote-task-request.test.tsx:255

status: fixed | kind=2(带副作用的 waitFor 套重操作)| severity=重要 | evidence: `await waitFor(async () => { const requestId = `probe-${probeCounter++}`; await emitRemoteRequest({...}); ... })` —— 每一轮轮询都往活着的 App 里**真发一次远程请求**并等应答。| why_flaky: waitFor 默认 50ms 一轮,每轮 = 一次事件 handler + 一次 invoke + 一次全树重渲染。负载下每轮自身耗时超过 50ms,轮询就叠着排,3000ms 预算被探针本身消耗掉,而不是用来等启动链路完成。探针计数器 `probeCounter` 还是模块级递增状态(:222),同文件用例之间共享。
> 复核(2026-09-18):已修:先 await 无副作用的 waitForBoot()(:234,只读 invoke 调用记录)再进带探针的 waitFor,并把轮询间隔放宽到 interval: 250(:279),探针不再自己吃掉 3000ms 预算。

## src/test/app-boot.test.tsx:149

status: wontfix | kind=2 | severity=重要 | evidence: `await waitFor(() => expect(savedTaskSnapshots(projectId).length).toBeGreaterThan(0), { timeout: 3000 });`(helper `savedTasksFor`,被本文件多条用例复用)| why_flaky: 显式 3000ms 与默认同值,等的是「renderApp() 全量 App 启动 → 一批启动探测 invoke → get_active_task_ids → setState → taskPersistence 350ms 防抖 → save_project_tasks」。全 App 挂载在 v8 插桩下本身就要几百毫秒,9 fork 争抢时这条链很容易逼近 3000ms。这是本文件多条用例共用的入口,一旦超时会连片红。
> 复核(2026-09-18):不改,但已从「可疑」升级为「有实测依据」:预算保持 3000ms。实测这条链裸跑 357/360ms、9 个重文件并行 389/360ms —— 耗时由固定的 350ms 防抖主导,而 setTimeout 是挂钟、不随 CPU 争抢放大,可放大的只有 ~10-40ms 的同步段,3000ms 有 7 倍以上余量。理由已写进 :144-152 的注释。

## src/test/app-boot.test.tsx:241

status: wontfix | kind=1 | severity=次要 | evidence: `await new Promise((resolve) => setTimeout(resolve, 600));` 等 350ms 防抖窗口过去,再断言 `expect(saves).toEqual([])`。| why_flaky: 方向上负载越慢越不会写盘,所以是**假绿**风险而不是翻红风险 —— 但它在整个 App 已挂载的情况下白等 600ms 真实时间,是套件挂钟的净成本;同文件 :306 还有一处 200ms 同型等待。
> 复核(2026-09-18):不改,但结论比原条目更精确:这里的 600ms 其实**不是**假绿风险。它与被测的 350ms 防抖同为 setTimeout,而 setTimeout 回调按到期时间顺序执行 —— 350ms 那个必然先于 600ms 这个触发,与机器忙不忙无关(能拖长的是同步段,不是定时器的相对顺序)。所以它是确定的,代价只是 600ms 挂钟。要省掉这 600ms 只能改假时钟,而这条用例已经挂载了全量 App,中途切假时钟捕获不到挂载时用真实定时器排上的防抖,得不偿失。已知残余:约 600ms 挂钟成本(同文件 :306 另有 200ms)。

## src/test/app-event-wiring.test.tsx:386

status: fixed | kind=1 | severity=次要 | evidence: `await new Promise((resolve) => setTimeout(resolve, 600));` 之后 `expect((await latestSavedTask("p1","t-1")).status).toBe("detached")`。| why_flaky: 600ms 真实等待之后紧跟一个内含 3000ms waitFor 的 helper。等待本身是负向断言(不该被拽回),负载慢 → 假绿;但它把这条已经很长的链又拉长 600ms,和 :382 的嵌套 waitFor 叠在同一个用例里。
> 复核(2026-09-18):已修:600ms 真实睡眠已删,改用「另一条会落盘的事件当有序屏障」(emit t-2 → done 并等它落盘),把负向断言换成有因果的正向等待,同时省掉 600ms 净挂钟。

## src/test/database-view-workspace-grid.test.tsx:706

status: wontfix | kind=4 相关(单条用例逼近全局 30s)| severity=重要 | evidence: 一条 `it("applies DBX grid filtering, sorting, search, and column visibility controls")` 横跨 706-993 行,约 20 次 `await user.*`,其中 `user.type` 的字符串合计约 42 个按键(`"status = 'active'"`、`"email DESC"`、`"alice"`、`"active"`、`"note"` …),每个按键都是一轮完整的 DBX 网格重渲染,期间还夹着 6 处 waitFor。| why_flaky: vitest.config.ts:7-9 的注释自陈「覆盖率插桩 + 并行 worker 会让重 UI 测试的墙钟时间放大 5~6 倍,15s 对最慢的几个 DBX 网格测试只剩 ~20% 余量」。也就是说这类用例在 15s 门槛下实测已到 ~12s;30s 给了余量,但这是全仓单条最重的用例,同时也是最接近 testTimeout 的那一条。
> 复核(2026-09-18):不再拆分,改由全局预算兜住:vitest.config.ts:10 的 testTimeout 已从 15s 抬到 30000,并把依据写在 :7-9(覆盖率插桩 + 并行 worker 让重 UI 用例墙钟放大 5~6 倍,15s 对最慢的 DBX 网格只剩 ~20% 余量)。拆分这条会丢掉「筛选→排序→搜索→列可见性」一次走完的端到端鉴别力,收益不足。属已知残余:它仍是全仓单条最重、最接近 testTimeout 的用例。

## src/test/skills-shop.test.tsx:122

status: fixed | kind=2(显式超时 < 2000)| severity=重要 | evidence: `{ timeout: 1200 }` —— 全仓唯一一处把 waitFor 预算**收紧**到默认 3000ms 以下的地方,守的是 `await user.type(..., "react")` 之后 SkillsShop.tsx:137-141 那个**真实 300ms 防抖** → setDebouncedQuery → load() → invoke。| why_flaky: 余量约 900ms,而这条用例前面已经串了 selectOptions×2 + 两次 waitFor;user.type 的 5 个按键各触发一次重渲染。负载下 jsdom 的 300ms 定时器会滑,waitFor 自身 50ms 一轮也要抢 CPU,1200ms 是这一串里最薄的一层。收紧到默认值以下没有任何收益(通过路径一满足就返回),纯粹是把余量让出去。
> 复核(2026-09-18):已修:{ timeout: 1200 } 已删除,回到默认 3000ms;原委(通过路径一满足即返回,收紧无收益、只是把余量让出去)写进 :116-119 的注释。

## src/test/notebook-panel-links.test.tsx:584

status: fixed | kind=1 | severity=次要 | evidence: `// 等过出卡延迟(380ms)才有意义` + `await new Promise((resolve) => setTimeout(resolve, 450));` 之后 `expect(hoverCard()).toBeNull()`;同型在 :603。| why_flaky: 只有 70ms 余量。断言方向是「不该弹卡」,所以负载慢 → 卡片本该弹但还没弹 → **假绿**(守着空气),而不是翻红。真实风险是这条测试在满负载下失去鉴别力,同时净烧 900ms 挂钟。
> 复核(2026-09-18):已修:450ms 真实睡眠换成确定性假时钟 —— vi.useFakeTimers({ toFake: ["setTimeout","clearTimeout"] }) 之后 await vi.advanceTimersByTimeAsync(HOVER_SHOW_DELAY + 50)。只伪造出卡链真正依赖的两个定时器,不牵动 rAF/Date。同文件 :603(嵌入块头部)同样处理。已做变异对照:去掉选择器里的 :not(.notebook-embed-head)、或去掉 hoverTargetFrom 的 `!path` 守卫,两条用例都会翻红(此前真睡 450ms 时是假绿)。

## src/test/notebook-panel-history.test.tsx:185

status: wontfix | kind=1 | severity=次要 | evidence: `await new Promise((resolve) => setTimeout(resolve, 1200));` 等 CodeMirror 的 200 拍打字闩过期(注释:「200 tick × 1ms 的 interval,jsdom 里会更慢」),随后 :187 `await waitFor(() => expect(harness.read(notePath)).toContain(...))` 用默认 3000ms 等 800ms 自动保存落盘。| why_flaky: 前半段是假绿方向(闩被饿住 → 覆盖不发生 → 断言照过);后半段是真翻红方向 —— 1200ms 已经烧掉,再让 800ms 防抖 + harness 保存链挤进 3000ms。同一形态的 1200ms 真实睡眠还有 notebook-panel-replace.test.tsx:190、notebook-panel-tasks.test.tsx:267、notebook-panel-templates.test.tsx:490,四处合计约 4.8s 净挂钟。
> 复核(2026-09-18):不改,理由有三:(1) 断言方向是**假绿**而非误报 —— 负载极慢时闸没走完,断言的鉴别力下降,但不会给正常提交制造红灯;(2) 闸走的是 CodeMirror 内部的 setInterval(…, 1),而这条用例已经挂载了真实编辑器,中途 vi.useFakeTimers() 捕获不到挂载时用真实定时器排上的 interval,要改就得把整个编辑器搬进假时钟,爆炸半径远大于它消掉的风险;(3) 同形态的 1200ms 真实睡眠还有 notebook-panel-replace:190、notebook-panel-tasks:267、notebook-panel-templates:490 三处,要动应作为一次独立的「编辑器闸」专项统一处理,而不是在这一轮里改一处留三处。已知残余,记录在案。

## src/test/notebook-panel-history.test.tsx:924

status: fixed | kind=1 | severity=次要 | evidence: `harness.releaseTagScan(0);` 之后 `await new Promise((resolve) => setTimeout(resolve, 20));` 再 `expect(tags()).toContain("#quick"); expect(tags()).not.toContain("#slow");`| why_flaky: 20ms 真实窗口用来证明「迟到的响应不会盖掉当前那条」。负载下 20ms 内 setState + 重渲染根本没发生,于是就算把 noteId 守卫删掉测试也照绿 —— 假绿。应该改成 `await waitFor` 等一个能证明迟到响应**已经被处理过**的正向信号(例如 heldTagScanCount 归零或某次渲染计数增长),再断言 tags 未变。
> 复核(2026-09-18):已修:20ms 真实窗口换成 await act(async () => {}) —— 放行到落库整条链全是微任务(harness resolve → mock invoke 的 await → 组件 await → setState),而 act 内部跨一个宏任务边界,宏任务开跑前微任务队列必然排空,所以「迟到响应已处理完」是确定的。已做变异对照:去掉 useNoteProperties.ts 里 `current?.noteId === noteId` 的守卫,这条用例翻红(此前 20ms 时是假绿)。

## src/test/docker-view.test.tsx:211

status: fixed | kind=1 | severity=次要 | evidence: `await new Promise((resolve) => setTimeout(resolve, 20));` 之后 `expect(invoke).toHaveBeenCalledTimes(1);` 用来证明「父层重建同一个 SSH 连接对象不会重新拉取」。同型:bridge-python-field.test.tsx:133(20ms 后 `expect(mockInvoke).not.toHaveBeenCalled()`)。| why_flaky: 负向断言 + 真实 20ms 窗口 = 负载下假绿(多余的 effect 还没跑完就断言了)。翻红方向只有一种:前一句 `waitFor(() => expect(invoke).toHaveBeenCalledTimes(1))` 若因任何原因先看到 2 次就永不满足,耗满 3000ms 后失败。
> 复核(2026-09-18):已修:20ms 真实窗口换成 await act(async () => {}),理由同上(invoke 是立即 resolve 的 mock,整条链在微任务里)。已做变异对照:让 dockerRemoteKey 每次返回不同值(模拟「父层重建对象就重新拉取」),这条用例翻红。

## src/test/omp-hook.test.ts:53

status: wontfix | kind=3(真实文件 IO + 进程级环境变量)| severity=次要 | evidence: `eventDir = mkdtempSync(join(tmpdir(), "aeroric-omp-hook-"))`,并写 `process.env.AERORIC_TASK_ID / AERORIC_EVENT_DIR / AERORIC_AGENT`;`written()` 每次 `readFileSync(join(eventDir, "events.jsonl"))`;afterEach 用 `rmSync(..., {recursive:true, force:true})`。| why_flaky: 全仓唯一一处真写临时目录的测试。mkdtempSync 保证目录唯一,写读都是同步的、没有定时器,所以**不存在时序竞速**;残留风险是 macOS 上 tmpdir 被 IO 饥饿拖慢(每个 it 一次 mkdtemp + 一次 rm -rf),以及 rmSync 在 afterEach 前抛出时留下 /tmp 垃圾。归为次要,列出是为了闭合「真实 IO」这一类的排查。
> 复核(2026-09-18):无需改动 —— 条目本身已认定「不存在时序竞速」,归为次要只是为闭合「真实 IO」这一类的排查。mkdtempSync 保证目录唯一,读写全同步、无定时器;残留的只有 macOS 上 tmpdir 被 IO 饥饿拖慢这类环境噪声。保留 wontfix 状态字段以示已复核。

## src/test/resizeObserverStub.ts:15

status: fixed | kind=5(模块级可变单例,跨测试不清)| severity=次要 | evidence: `const entries = new Set<StubEntry>();` 是模块级的,`triggerResize()` 遍历它同步派发所有回调;**没有任何 reset 导出**,setup.ts:107 只在文件开头 `installResizeObserverStub()` 一次。| why_flaky: 同一文件内前面用例挂载过、但没能正常 disconnect 的组件,会把 entry 留在 Set 里;后面某个用例调 `triggerResize()` 时会连带触发那些**已卸载组件**的回调 → 对已卸载树 setState / 读已摘掉的 DOM。表现为「顺序相关的偶发失败」:单跑那一条绿,整文件跑红。正常路径下组件 unmount 会 disconnect,所以只在有泄漏时暴露 —— 但这正是「偶发」的定义。建议导出一个 reset 并在 setup 的 afterEach 里清。
> 复核(2026-09-18):已修:与 shared-state.md 同一处 —— 新增 resetResizeObserverStub() 导出,由 setup.ts 的文件级 afterEach 每个用例清一次,模块级可变单例不再跨用例残留。

## src/test/app-settings-usage.test.tsx:66

status: fixed | kind=5(mock 未还原)| severity=次要 | evidence: `const requestFrame = vi.spyOn(window, "requestAnimationFrame").mockImplementation(...)`,还原写在用例**末尾**(:116-117 `requestFrame.mockRestore(); cancelFrame.mockRestore();`),而本文件**没有** afterEach。对照 app-settings-panel-registry.test.tsx:50-52 有 `afterEach(() => vi.restoreAllMocks())`。| why_flaky: 中途任何一条断言抛出就走不到 mockRestore,window.requestAnimationFrame 会以「只入队、永不执行」的形态留给同文件后续用例。当前它恰好是文件里最后一条用例,所以影响被掩住;一旦有人往后追加用例,就变成顺序相关的连锁失败。同文件还有:每条用例各自 `localStorage.setItem("aeroric:language","en")` 但无 `localStorage.clear()`。
> 复核(2026-09-18):已修:文件顶部(:6-9)已有 afterEach(() => { vi.restoreAllMocks(); localStorage.clear(); }),requestAnimationFrame / cancelAnimationFrame 两个 spy 与语言键都被兜住,不再依赖「还原写在用例末尾」。

## src/test/terminal-input-fix.test.ts:158

status: wontfix | kind=6(真实动画帧)| severity=次要 | evidence: `await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));` 之后 `expect(textarea.style.left).toBe("30px")`;同型在 :186、:218。这三条都在 `vi.resetModules()` + `vi.doMock("../platform")` + `await import("../components/terminalInputFix")` 之后。| why_flaky: 依赖真实 jsdom rAF。这里等的是**同一次**帧回调(被测代码在 __renderCb 里 schedule,测试再 await 一帧),注册顺序保证了被测回调先跑,所以逻辑上不竞速;风险只在 jsdom rAF 被饿住时整体拉长(每处 ~16ms→可能几百 ms),叠上 resetModules 导致的重复模块转换。归次要,列出以闭合 kind=6 —— 全仓其余 rAF 用法都已被 spy/stubGlobal 接管(terminal-runtime、terminal-output-stop、stall-recorder、app-settings-* 等),这三处是唯一还在等真帧并做正向断言的。
> 复核(2026-09-18):不改 —— 条目自身已说明「注册顺序保证了被测回调先跑,逻辑上不竞速」,风险只在 jsdom rAF 被饿住时整体拉长。要改成确定性就得 stub 掉 requestAnimationFrame,而被测的 attachWindowsIMEPositionFix 正是靠「把定位推迟到下一帧」工作的 —— 同步化等于把这个测试的存在理由删掉。三处(:158/:186/:218)保留真帧等待。

## src/test/notebook-visual-scheduler.test.ts:128

status: fixed | kind=1 | severity=次要 | evidence: renderOne 内 `await new Promise((r) => setTimeout(r, 1))`,外面 `await new Promise((r) => setTimeout(r, 30))` → `fireAll()` → `await new Promise((r) => setTimeout(r, 100))` → `expect(maxInFlight).toBe(1)`;`disconnect()` 那条(:169)用 80ms。| why_flaky: `toBe(1)` 需要「至少跑过一块」且「从未并发」。1ms 定时器一定先于 30ms 定时器到期,所以可见块必然已渲染过、maxInFlight 必然≥1;串行不变量保证不会到 2 —— 逻辑上稳。真正的隐忧是这些魔法毫秒数没有余量说明:visualScheduler.ts 里 stage-1 的 visibleDone 循环与 IO 触发的 drain() 是**两个独立的异步循环**,只靠这 30ms 真实等待把它们错开;哪天有人把 viewportHeight 调大让可见集变多,这条就会变成真竞速。
> 复核(2026-09-18):已修:30ms / 100ms 两个魔法毫秒数换成两个正向信号 —— 先 await waitFor(() => expect(done).toBeGreaterThanOrEqual(1)) 保证「至少跑过一块」(此刻 maxInFlight 必然 ≥1),放行 IO 后再 await waitFor(() => expect(done).toBe(10)) 等十块全部完成,然后才断言 maxInFlight === 1。已做变异对照:把 drain 循环里的 await runOne(block) 改成 void runOne(block),用例在「全部完成后」那一句翻红 —— 说明它现在唯一能翻红的原因就是真的并发了。

## src/test/notebook-panel-search.test.tsx:59

status: fixed | kind=2 | severity=次要 | evidence: helper `openFind` 里 `await waitFor(() => expect(editorValue()).toBe(body));` 用默认 3000ms,守着 CodeMirror 挂载 + 磁盘正文进文档;文件头 :39-51 的长注释记录了这一类的根因:「@uiw/react-codemirror 的打字闩走 `setInterval(…, 1)`,jsdom 在整份测试文件的负载下 1ms 定时器会被饿到几秒一拍 …… 这类用例会『单独跑过、整文件跑挂』」。| why_flaky: 他们的规避手段是「正文从磁盘种进去,不产生编辑器侧变更,闸不会开」,这确实避开了闩。残留风险是 openFind 被本文件几十条用例复用,每次都要真挂一次 CodeMirror 并在 3000ms 内等文档就位;而 notebook-panel-replace.test.tsx:182/187/216/248、notebook-panel-history.test.tsx:143/172/181/207/412/818 仍在用 `setEditorValue`,**主动上膛**同一个 1ms-interval 打字闩。
> 复核(2026-09-18):已修:openFind 里那条 waitFor 显式抬到 10000ms 并写明依据。它是本文件几十条用例的公共入口,每次都要真挂一遍 CodeMirror 并把磁盘正文种进文档;`test:coverage` 那一遍(全量并行 + 插桩,整体比裸跑慢 ~1.7x)会把默认 3000ms 吃穿。与 notebook-panel-tags.test.tsx:367(10000ms)、file-viewer-outline.test.tsx:175(5000ms)同一先例。通过路径一满足即返回,不拖慢正常用例。

## src/test/file-viewer-outline.test.tsx:175

status: wontfix | kind=2/4(已被放宽的已知慢链)| severity=次要 | evidence: `const outline = await screen.findByRole("navigation", { name: "Outline" }, { timeout: 5_000 });` —— 全仓唯一一处把 findBy 预算抬到 5000ms 的地方,等的是 FileViewer.tsx:661-679 那个 **250ms 防抖** 后的 requestLspDocumentOutline,前面还要挂完整个 CodeMirror + lsp_server_status 探测。| why_flaky: 已经放宽,所以是**证据**而非高危项:它和 notebook-panel-tags.test.tsx:367(放宽到 10000ms,注释直言「同一个 3000ms 挂钟下链条长一倍,`test:coverage` 那一遍(355 个文件并行 + 插桩,整体比裸跑慢 ~1.7x)就会在这一条上先超时」)一起证明,3000ms 默认预算在这台机器上对「长链 + 真实防抖」已经被吃穿过至少两次。凡是同型长链但仍用默认 3000ms 的地方(本清单第 1、5、6、7 条)就是下一个候选。
> 复核(2026-09-18):无需改动 —— 条目自身定位为「已被放宽的已知慢链」,是**证据**而非待修项:它(5000ms)与 notebook-panel-tags.test.tsx:367(10000ms)共同证明了默认 3000ms 在本机对「长链 + 真实防抖」已被吃穿过。本轮已按这条线索把同型的 notebook-panel-search.test.tsx 公共入口一并放宽。保留 wontfix 以示已复核、且不再额外动作。
