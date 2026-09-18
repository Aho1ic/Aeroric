# 前端空转测试清单(17 条)

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
> 本清单终态统计（2026-09-18 复核）：`fixed` 17 / `wontfix` 0 / `open` 0，共 17 条；67 条全部附有复核行（0 条缺）。
> 四份清单合计：`fixed` 59 / `wontfix` 8 / `open` 0，共 67 条。

## src/test/dsh-slash-palette.test.tsx:166

status: fixed | test_name=「hasArg 由远端的 input.hint 决定」 | kind=2 | why=唯一断言是 expect(options().length).toBeGreaterThan(0)。这个条件在 fireEvent.click 之前就已成立(远端 mock 返回 2 条,上一行的 waitFor 已断言 toHaveLength(2)),点击既不卸载面板也不改条数,断言与 hasArg 毫无因果。把 src/components/DshSlashPalette.tsx:55 的 `hasArg: Boolean(row.input?.hint)` 改成 `hasArg: false`(或 `true`、或整行删掉)它都不会红 —— 更根本的是 grep 全仓库确认 hasArg 只在 dshSlashCommands.ts 里被写、在 DshSlashPalette.tsx:55 被赋值,生产代码里没有任何读取点(commit() 只看 cmd.popup),所以这个字段当前没有任何可观测行为,测试名承诺的契约不存在。 | suggest=改成断言点击远端行时真正可观测的东西:expect(editorInsert).toHaveBeenCalledWith("withhint") 与 expect(onDismiss).toHaveBeenCalled();若确实要守 hasArg,先让它在 UI 上可观测(如带 hint 的行显示参数提示)再断言,否则连同 :55 的死代码一起删掉。 | severity=重要
> 复核(2026-09-18):已修:空转的 hasArg 用例已删除,改为同文件 :160「点击远端命令行插入名字并关面板」——断言 editorInsert 收到 "withhint"(带 input.hint 的那条)且 onDismiss 被调用,正是原建议要求的可观测契约;hasArg 字段与该用例名在仓库中均已不存在。

## src/test/editor-utils-extensions.test.ts:384

status: fixed | test_name=「blame 标记不吃鼠标事件(点它不该动光标)」 | kind=2 | why=两条断言分别是 expect(() => marker.dispatchEvent(event)).not.toThrow() 和 expect(view.state.doc.toString()).toBe("hello")。mousedown 本来就不会改文档文本,所以第二条恒真;测试名承诺的「不该动光标」是 selection,而 selection 一次都没被断言。把 src/components/file-viewer/editorUtils.ts:526-528 的 `ignoreEvent(): boolean { return true; }` 整个删掉(或改成 return false),CodeMirror 会开始把这次 mousedown 当成编辑器内点击去移光标,文档文本仍是 "hello"、也不抛异常,测试照样全绿。 | suggest=先记下 const before = view.state.selection.main.head,dispatch 之后断言 expect(view.state.selection.main.head).toBe(before);把 not.toThrow() 换成这条真正的不变量。 | severity=重要
> 复核(2026-09-18):已修,且修法优于原建议。原建议的 `view.state.selection.main.head` 断言在 jsdom 下恒真(没有布局,posAtCoords 永远算不出位置),所以 :377 改成钉 ignoreEvent 真正的观察点:向 .cm-inline-blame 派发 mousedown 后,编辑器注册的 domEventHandlers({mousedown}) 必须一次都收不到;并加一条正对照——同一处理器对正文 .cm-line 的 mousedown 必须收到 1 次,否则上一条就是空断言。

## src/test/database-sidebar-tree-state.test.ts:41

status: fixed | test_name=「builds compact connection badges with deterministic or configured colors」 | kind=5 | why=expect(connectionBadgeColor(item)).toBe(connectionBadgeColor(item)) 是自比较,同一个纯函数同一个入参,恒真。同 it 里其余断言只覆盖 connectionBadgeText 的四种缩写和「dbx.color 显式配置」这一支,哈希兜底那一支(src/components/database/databaseSidebarTreeState.ts:55-56 的 CONNECTION_BADGE_COLORS[stableNameHash(connection.name) % len])完全没有绝对断言。把 stableNameHash 改成 `return 0`、把 `% CONNECTION_BADGE_COLORS.length` 改成 `% 1`、甚至把整个兜底换成固定返回一个色值,这条都不会红 —— 所有连接的徽章挤成同一个颜色(徽章配色的唯一作用就是让多连接可区分)也测不出来。 | suggest=断言绝对值与区分性:expect(connectionBadgeColor(connection("one","Production"))).toBe(<CONNECTION_BADGE_COLORS 里的具体色值>),再加一条 expect(connectionBadgeColor(connection("one","Production"))).not.toBe(connectionBadgeColor(connection("two","Staging"))) 守住「不同名字落到不同色」。 | severity=重要
> 复核(2026-09-18):已修:自比较已换成绝对色值断言(Production → #0f766e、Staging → #ca8a04)+ 一条「两者必须不同」的区分性断言,哈希兜底支不再是盲区。

## src/test/agent-options.test.ts:208

status: fixed | test_name=「uses one icon size across the compose toolbar triggers」 | kind=5 | why=唯一断言 expect(CONTROL_ICON_SIZE).toBe(14),就是常量等于它自己的字面量(src/components/new-task/AgentPermSelector.tsx:97 `export const CONTROL_ICON_SIZE = 14`)。测试名承诺的是「整排 trigger 用同一个尺寸」这条跨组件不变量,但一个组件都没渲染。把 AgentPermSelector.tsx:653 的 `size={CONTROL_ICON_SIZE}` 改成 `size={18}`、或把 LaunchModeSelector.tsx:171/229 改成写死数字,这条都不会红 —— 而它正是唯一声称守这件事的测试。 | suggest=要么渲染工具条、断言各 trigger 里 svg 的 width/height 一致(如 screen.getAllByRole("button") 内 svg 的 getAttribute("width") 全等);要么删掉这条,把「不许写死数字」交给 lint。单看常量值本身没有防御价值。 | severity=次要
> 复核(2026-09-18):已删除。CONTROL_ICON_SIZE 的字面量断言与「uses one icon size」用例名在仓库中均已不存在;该文件现以 composeFixedControlStyle / composeAgentTriggerStyle 的 toEqual 真行为断言收尾(:187、:197),不再靠「常量等于自己」占位。

## src/test/dsh-live-sessions.test.tsx:117

status: fixed | test_name=「applies jobs and queue frames」 | kind=2 | why=waitFor 里两条断言只看 s.s1?.jobs?.length 和 s.s1?.queue?.length 是否为 1,数组内容一次都没校验。src/hooks/useDshLiveSessions.ts:80-98 的 applyJobs/applyQueue 只要把落库改成截断或改写内容 —— 例如 `jobs: frame.jobs.slice(0, 1).map(() => ({}) as DshJobView)`、或把 items 里的 text 抹成空串 —— 长度仍是 1,测试照样绿,而 DshLiveBars 上会显示一排空白的 job/queue 行。 | suggest=直接断言落库内容:expect(s.s1?.jobs).toEqual([{ id: "j1", kind: "bash", status: "running", label: "lint" }]) 与 expect(s.s1?.queue).toEqual([{ itemId: "q1", text: "then do X" }]);顺带用两条以上元素守住不被截断。 | severity=次要
> 复核(2026-09-18):已修:断言从「长度为 1」升级为逐元素 toEqual 完整对象(jobs 两条、queue 两条),落库被截断或内容被抹空这类变异现在会变红。

## src/test/ide-tool-registry.test.ts:21

status: fixed | test_name=「registers IDE tools as metadata only」 | kind=5 | why=第 21-22 行 expect(IDE_TOOL_REGISTRY.every(tool => typeof tool.titleKey === "string")).toBe(true) 与同形的 commandId 版,断言的是 TypeScript 已经在 src/plugins/ideToolRegistry.ts:33/35 声明为 `titleKey: string` / `commandId: string` 的类型事实,运行时不可能为假。把任一条目的 titleKey 改成错的 key(如 "gitAdvanced.title" → "nope.title")、或把 commandId 改成和 getCommandPaletteIdeTools 期望的不一致,这两条都不会红 —— 同 it 里那条 id 顺序断言管的是 id,不覆盖 titleKey/commandId 的取值。 | suggest=换成取值断言:expect(IDE_TOOL_REGISTRY.map(t => [t.id, t.titleKey, t.commandId])).toEqual([...具体清单...]),或断言每个 titleKey 在 i18n 目录里存在(与 src/test/i18n-keys.test.ts 同思路);typeof 检查直接删掉。 | severity=次要
> 复核(2026-09-18):已修:typeof 检查已删除,换成 [id, titleKey, commandId] 三元组的精确清单断言,并逐个断言 titleKey 在 zh 语言包里存在(与 i18n-keys.test.ts 同思路)。titleKey/commandId 写错现在会变红。

## src/test/scoped-stores.test.ts:29

status: fixed | test_name=「applies functional debug session updates without sharing instances」 | kind=4 | why=it 里调了 setWatchDraft("count") 再断言 watchDraft === "count",而 debugPanelStore.ts:40 的 setWatchDraft 就是 set({watchDraft}) 这一句裸转发,属于「塞 X 读回 X」;真正被 it 名字点名的 setSessions 只用恒等函数调了一次,全程没有任何 sessions 断言。把 debugPanelStore.ts:29-30 改成 `setSessions: (value) => set({ sessions: value as DebugSessionSnapshot[] })`(丢掉 typeof===function 分支,直接把函数本身当数组存进 state)这条测试照样绿 —— 而那正是它声称在测的那一行。 | suggest=改成传一个真正变形的 updater(如 setSessions([snapA]) 后 setSessions(prev => [...prev, snapB])),断言 first.getState().sessions 的最终数组内容与长度;watchDraft 那句要么删掉,要么换成断言 reset() 后回到 ""。 | severity=重要
> 复核(2026-09-18):已修:setSessions 改成真正变形的 updater(first 传 [snapA] 再传 prev => [...prev, snapB]),断言最终数组 debugId 为 ["a","b"] 且第二个 store 仍为空;watchDraft 那句改成断言 reset() 后回到 ""。「把函数本身当数组存进 state」这种变异现在会变红。

## src/test/notebook-wysiwyg.test.tsx:158

status: fixed | test_name=「暗色主题下不抛」 | kind=2 | why=唯一断言是 not.toThrow(),而且它连暗色都没进去:mountEditor()(同文件 16-32 行)固定传 themeVariant="light",NoteSourceEditor.tsx:553 用的是 themeFor(themeVariant) 而不是读 documentElement 的 class。加在 documentElement 上的 "dark" 类对被测组件毫无影响。把 NoteSourceEditor.tsx:252 的 `if (variant === "dark") return githubDark;` 整行删掉,这条依然绿。 | suggest=让 mountEditor 接受 themeVariant,以 "dark" 挂载,断言可观测差异(例如 content 上的 .cm-theme-dark / 代码 widget 前景色与 light 挂载不同),否则直接删掉这条。 | severity=重要
> 复核(2026-09-18):已修:改为 :155「暗色主题真的换成暗色配色,而不是照着亮色渲染」——以 themeVariant="dark" 挂载后断言 CodeMirror 公开 facet EditorView.darkTheme === true,cleanup() 后以 "light" 再挂载断言 === false 作对照。原 not.toThrow() 已删除。

## src/test/notebook-wysiwyg.test.tsx:167

status: fixed | test_name=「未闭合围栏不抛」 | kind=2 | why=唯一断言是 not.toThrow()。同项目的 notebook-render.test.ts:195 对同一场景钉的是真契约「未闭合的围栏不吞掉后续内容」,这里却只要求不抛。装饰层若把未闭合围栏整段替换成 block widget 而丢掉 "unclosed" 这段可见文本、或让 doc 长度与源串不再相等(即本文件 145-155 行那条最重要的不变式在这个输入上失效),这条测试全程不会红。 | suggest=补两句:expect(view.state.doc.toString()).toBe("```js\nunclosed\n") 保住「装饰不改文档」不变式,以及 expect(content.textContent).toContain("unclosed") 保住内容没被 widget 吞掉。 | severity=重要
> 复核(2026-09-18):已修:改为 :175「未闭合围栏不吞掉后面的内容,也不改文档」——断言 doc.toString() === 源串(保住「装饰不改文档」不变式)且 content.textContent 仍含 "unclosed"。

## src/test/ssh-split-layout-css.test.ts:5

status: fixed | test_name=「uses an exact one-pixel divider and shrinkable equal columns」 | kind=5 | why=整个文件只有这一句,断言常量等于自己的字面量。viewMode.ts:13 的 SSH_SPLIT_GRID_TEMPLATE 只是 AUXILIARY_SPLIT_GRID_TEMPLATE 的别名,全仓库除了这个测试没有任何生产代码引用它(ProjectPage.tsx:1788 用的是 AUXILIARY_ 那个名字)。也就是说:把 ProjectPage.tsx:1788 的 gridTemplateColumns 改成 "1fr 1fr" 之类的错值、或把 viewMode.ts:12 的真常量改坏,这条都不会红;它唯一能抓的是「有人手动去改那个没人用的别名」。文件名叫 SSH split layout CSS,实际一行 CSS 都没验。 | suggest=要么删掉这个文件与 SSH_SPLIT_GRID_TEMPLATE 这个死别名,要么改成渲染 ProjectPage 的 split 布局后断言那个容器的 style.gridTemplateColumns —— 把常量和它的使用点绑在一起才有区分度。 | severity=重要
> 复核(2026-09-18):已删除,且按建议连同死别名一起清掉:src/test/ssh-split-layout-css.test.ts 文件已不存在;生产侧 SSH_SPLIT_GRID_TEMPLATE 在全仓库(排除 node_modules/dist/coverage)已无任何引用。

## src/test/notebook-wysiwyg.test.tsx:163

status: fixed | test_name=「空文档不抛」 | kind=2 | why=唯一断言是 not.toThrow()。空文档上真正会出错的形态是「装饰层在空 doc 上凭空造出 widget」或「doc 被写成非空」,这两种都不抛。把装饰构建改成在空文档上也 push 一个 range(比如去掉空文档的 early return),CodeMirror 只要区间合法就不报错,这条仍然绿。 | suggest=补 expect(view.state.doc.length).toBe(0) 与 expect(content.querySelector(".cm-md-code-widget")).toBeNull(),把「空文档上不产生任何装饰」这个可观测结果钉住。 | severity=次要
> 复核(2026-09-18):已修:改为 :166「空文档上不产生任何 widget,文档也仍是空的」——断言 doc.length === 0,并断言 .cm-md-code-widget / .cm-md-table-widget / .cm-md-frontmatter-widget 三者皆为 null。

## src/test/platform-fonts.test.ts:34

status: fixed | test_name=「provides platform-compatible UI and terminal fallback chains」 | kind=5 | why=四句断言全是「常量包含自己字面量里的一个子串」(DEFAULT_UI_FONT_BY_PLATFORM.windows 含 "Segoe UI" 等),不经过任何被测函数。it 名字里的 "fallback chains" 一点没验:把 types.ts 里这几个常量改成只剩一个字体名、把 sans-serif/monospace 兜底整段删掉,只要还留着 "Segoe UI" 就照样绿;composeFontStack 的行为也完全没被这条覆盖。 | suggest=改成经由 composeFontStack 断言链条形状:每个平台的默认链都必须以通用族(sans-serif / monospace)结尾、且长度 > 1;或者直接删掉这条,后面 58-83 行那三条 composeFontStack 用例已经覆盖了真行为。 | severity=次要
> 复核(2026-09-18):已修:改成经由 composeFontStack 断言链条形状 —— 每条链拆开后长度必须 > 1 且以通用族(sans-serif/serif/monospace/ui-monospace)收尾,「只剩单字体、CJK 兜底被删」这种变异现在会变红。

## src/test/notebook-outline.test.ts:180

status: fixed | test_name=「undefined 输入不抛」 | kind=2 | why=唯一断言是 not.toThrow(),而 analyzeNote(noteOutline.ts:130)在 undefined 上一旦不抛,返回什么都无人过问。给它加一句 `if (!source) return { outline: [], words: 999, readingMinutes: 42 };` 这类错值,状态栏会显示出凭空的字数,而这条测试全程绿 —— 同文件 172 行已经写明「空文档是 0 字 0 分钟」是契约,undefined 分支却没被同等对待。 | suggest=改成 expect(analyzeNote(undefined as unknown as string)).toEqual({ outline: [], words: 0, readingMinutes: 0 }),与上面「空文档」那条同一口径。 | severity=次要
> 复核(2026-09-18):已修:改为「undefined 输入按空文档处理」,断言 analyzeNote(undefined) 逐字段 toEqual { outline: [], words: 0, readingMinutes: 0 },与「空文档是 0 字 0 分钟」同一口径。

## src/test/notebook-status-bar.test.tsx:56

status: fixed | test_name=「保存状态挂在 role=status 上」 | kind=3 | why=唯一断言是 getByRole("status") 存在,而这条断言无法独立失败:同文件 49-54 行的四条参数化用例每条都先 getByRole("status") 再读 textContent,getByRole 找不到就直接抛。要让这条红,那四条必须先红;它没有携带任何新增区分度(也没验 aria-live/aria-atomic 之类真正决定「能否被播报」的属性)。 | suggest=要么删掉,要么换成断言它真正想守的东西:expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite")(或组件实际用的那个 live 属性)。 | severity=次要
> 复核(2026-09-18):已删除:空转的「保存状态挂在 role=status 上」已移除;role=status 的覆盖仍由 :49-54 四条参数化用例(每条先 getByRole("status") 再读文案)承担,并新增 :62「四种状态各有各的颜色」断言四种状态的 style.color 去重后为 4。无覆盖回退。

## src/test/notebook-split-scroll.test.ts:172

status: fixed | test_name=「不可滚动的元素比例为 0,不抛」 | kind=2 | why=读方向有真断言(getRatio() === 0),写方向只有 not.toThrow()。splitScrollSync.ts:136-138 的 setRatio 是 `el.scrollTop = ratio * Math.max(0, max)`;把 Math.max(0, max) 去掉,max 为负时算出 -200 这类负值,赋 scrollTop 不抛任何异常,这条照样绿 —— 而 it 名字和 173 行注释点名要防的正是这里算出 Infinity/NaN/负值。 | suggest=在 setRatio 之后补 expect(pane.el.scrollTop).toBe(0)(顺带排除 NaN:expect(Number.isNaN(pane.el.scrollTop)).toBe(false))。 | severity=次要
> 复核(2026-09-18):已修:按建议在 setRatio(0.5) 之后补上 expect(pane.el.scrollTop).toBe(0) 与 expect(Number.isNaN(pane.el.scrollTop)).toBe(false);写方向不再只有 not.toThrow(),把 Math.max(0, max) 去掉导致算出负值/NaN 的变异现在会变红。

## src/test/terminal-font-size.test.ts:9

status: fixed | test_name=「defaults terminal font size to one point smaller than before」 | kind=5 | why=唯一断言是常量等于自己的字面量(DEFAULT_TERMINAL_FONT_SIZE === 11),不接任何行为。真正读这个默认值的路径是 platform-fonts.test.ts:22-32 那条(localStorage 缺键时回落),而它引用的是常量本身、不是 11。所以:改坏取默认值的那条分支(appThemeState 里读不到键时返回别的值)这条不会红;把常量从 11 改成 12 时它会红,但那是一次显式动作而非 bug。it 名字里的 "one point smaller than before" 更是无从校验。 | suggest=删掉;默认值已经被 platform-fonts.test.ts「读不到键时回落到 DEFAULT_TERMINAL_FONT_SIZE」那条以行为方式覆盖。真要钉字面值,应像 shell-terminal-panel.test.tsx:161-166 那样写明变异测试理由并放在有行为用例陪衬的文件里。 | severity=次要
> 复核(2026-09-18):已删除:该文件现仅 10 行,只保留 deriveShellTerminalFontSize 的真行为用例(12→11、10→10 触底);DEFAULT_TERMINAL_FONT_SIZE 的字面量断言与「defaults terminal font size to one point smaller than before」用例名在仓库中均已不存在。

## src/test/terminal-font-size.test.ts:18

status: fixed | test_name=「allows up to ten shell terminal tabs」 | kind=5 | why=唯一断言是 SHELL_TERMINAL_MAX_SESSIONS === 10,且与 shell-terminal-panel.test.tsx:161-166 完全重复 —— 那一条已经写明「其余用例都读常量,所以这里钉字面值」的变异测试理由,并且同文件 188-213、817-827 有真行为用例(到上限后加号禁用、addShell 不再新建)兜着。本文件里这条没有任何行为陪衬:把 ShellTerminalPanel.tsx:255 的 `if (shells.length >= SHELL_TERMINAL_MAX_SESSIONS) return;` 整行删掉,用户能开出无限多个终端,这条也不会红。 | suggest=删掉这条(保留 shell-terminal-panel.test.tsx 里那条有理由、有行为陪衬的);本文件只留 deriveShellTerminalFontSize 那条真行为用例,并把 describe 名字收回到 font sizing。 | severity=次要
> 复核(2026-09-18):已删除:SHELL_TERMINAL_MAX_SESSIONS 的字面量断言与「allows up to ten shell terminal tabs」用例名在仓库中均已不存在,describe 已收回到 font sizing;上限行为仍由 shell-terminal-panel.test.tsx 里有行为陪衬的那条承担。
