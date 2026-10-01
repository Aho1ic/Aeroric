# tauri → native 迁移评审记录:XcodeGen 工程和本地无账号构建(Task P4-01)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**:`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口（委托链见 `docs/native/tooling.md` §2/§3）。本会话 `skill://grilling` **加载成功**（与 P3-06 当日 404 相同、P3-07 当日成功不同——设备在册、文件可读），委托链在本轮全程成立。
- **实际调用入口**:① `skill://grilling` 读取＋按其轮次格式发出 frontier（§1 三条 Q92–Q94，用户不在场，按 P3-02/P3-04/P3-05/P3-06/P3-07 先例按推荐项记账、异议可重开）；② 五轴自查（§1，每条事实当场可复验）；③ 当日全矩阵（§2 表，退出码逐条单读）。
- **审查 commit/diff hash**（G31 规矩，**当场记**）:`git rev-parse HEAD`＝`2ae0492c4030c1cfb6971cc7f8d2910fabfc3013`（2ae0492，2026-10-01）；`git status --porcelain`＝**1 条目**（`?? .zcodeignore`——用户未跟踪文件、非本任务、不入 diff）；`git diff | git hash-object --stdin`＝`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`（**空 blob 对象哈希＝diff 为空**）。代码树即最终树：`8a1a7bf`（工程规格＋app 骨架＋generate.mjs）→`6387e7b`（构建链五件）→`2ae0492`（断言修正）**全部先 commit 再评审**；评审当日注定落盘的文档批（本文件＋总表行＋task-status＋freeze/digest）按纪律随评审后的 `docs(plan)` 终提交一并入库。
- **问答**:§1 frontier 三条（Q92–Q94）；无产品决策悬置。
- **证据**:§1 五轴的当日读数、命令与文件级引用；§2 验收对照表；补记二百九十六的逐片实测段（见 `MIGRATION_PLAN.md` Task P4-01 小节与续跑区）。
- **问题级别**:**两处结构性真发现，全部当场根修**——① **测试断言面误报注入失败**:`AppLaunchTests` 对 `staticTexts["bootstrap.environment"]` 断言 `.label`，实测恒得空串（`XCTAssertEqual failed: ("") is not equal to ("ui-test")`）；临时诊断把 `app.debugDescription` 全树打出，AX 实证＝`StaticText … identifier: 'bootstrap.environment', value: ui-test`，且 `bootstrap.data-dir` 行同屏——**注入链无恙，错在断言面**:macOS SwiftUI 静态文本把可见文本放在 `value`，`label` 为空。根修＝`(value as? String) ?? label` 双面读取（`2ae0492`），复跑 5.2s passed。教训:断言面先取证再写，空串误报要往「读错属性」想，而不是先怀疑注入链。② **XcodeGen 2.46.0 两个静默语义坑**（本地探针实证）:自定义 config 名必须在**顶层 `configs:` 键**声明——`options.configs` 的各种形状被静默忽略（未知键不报错）；`-p` 是**父目录**语义——传 `<dir>/X.xcodeproj` 会拼成 `X.xcodeproj/<SpecName>.xcodeproj` 且 copy ENOENT 失败。两坑修法（顶层键＋`-p macos`＋产物存在性断言）固化进 `scripts/native/generate.mjs` 注释与代码。**一处文档面观察如实记**:`.trellis/spec/aeroric-macos/frontend/index.md` 引用的 `native-ui.md` 在磁盘上不存在（死链接）——非代码问题，不在 P4-01 修，owner＝spec 维护轮（`trellis-update-spec`），不阻塞。**一行对账注记**:开工轮曾按手交接预测校验 `grep -c '^> \*\*【执行状态】补记'`（预测 143→144），实测 203→205——预测基线过时（该 pattern 计的是全文件【执行状态】块总数，含各 Task 小节块，非仅续跑区）；以 `git diff --stat` 净变化量（12+/2−，与设计逐行对上）为准证据，无文件损伤。
- **修复后复验**:本文写毕前当日矩阵全绿（§2 表），全部为当日实测，无引用旧读数；评审文件落盘后 skeleton 单轮复位（见 §2）。

## 1. 收尾拷问轮(2026-10-01，五轴)

**轮次输入**:Slice 1 十文件＋Slice 2 构建链五件全部落地（补记二百九十六）；三笔提交 `8a1a7bf`/`6387e7b`/`2ae0492`。

**五轴自查结论(事实，非问题——逐条当场可复验)**:

1. **唯一 owner**:工程规格单源＝`macos/project.yml`（xcodegen 唯一输入；五个无账号签名基线在 `settings.base` 单源，两份 xcconfig 只放 config 增量——Debug 侧 `SWIFT_ACTIVE_COMPILATION_CONDITIONS = DEBUG` 是因为自管 xcconfig 替换 Xcode 默认，`#if DEBUG` 依赖它）；生成物 `AeroricNative.xcodeproj` 被 `.gitignore` 盖，`generate.mjs` 的产物存在性断言是「绝不手编 pbxproj」的机械保证；构建编排单链＝`build.mjs` 六步（generate→cargo `--locked`→xcodebuild `CODE_SIGNING_ALLOWED=NO`→复制 helper→sign-local→verify-bundle，每步 exit 码传播），`sign-local.sh`/`verify-bundle.sh` 只被它调用；app 配置单源＝`AppServices.resolvingInjectedPreviewConfiguration`（全仓唯一读 `AERORIC_*` 的地方，`AeroricApp.init` 经 `State(initialValue:)` 恰好一次构造）。
2. **反例**:XcodeGen 两坑探针（§4.2②）与钉版校验（xcodegen ≠ 2.46.0 即拒绝，`generate.mjs` 头部断言）；`AppLaunchTests` 注入值**精确比对**而非存在性——曾误报，根修后每绿一次都证明 TEST_RUNNER_ 转发真的到达 app；`verify-bundle.sh` 对三 helper（`aeroric-cli`/`__preview.dylib`/`AeroricNative.debug.dylib`）与主 bundle 逐件 `--verify --strict`（实测全部 validated）；`generate.mjs` spec 缺失/产物缺失即 fail。
3. **边界**:app **零存储访问**——`AppServices` 只记录注入值，data dir 不发现/不创建/不读取，无任何默认用户路径（结构性，不是纪律约定）；隔离双层＝脚本层（ui-test/dev 各自 `rmSync`+`mkdirSync` 全新 scratch＋`AERORIC_ENV=ui-test/dev` 注入）＋系统层（bundle id `com.aeroric.native.dev`/`.dev.uitests` 与旧 `com.aeroric.desktop` 不共享 TCC，从未请求过权限）；签名＝无账号 ad-hoc（DEVELOPMENT_TEAM 空串/CODE_SIGN_STYLE Manual/CODE_SIGN_IDENTITY `-`/ENABLE_HARDENED_RUNTIME NO/ENABLE_APP_SANDBOX NO），Developer ID 路径归发布 Task；`cargo build --locked -p aeroric-cli` 只为把产物复制进 bundle helpers（构建链，非源变更）。
4. **回归**:当日矩阵全绿（§2 表）；Rust/宿主/前端**零面**（无 Rust/TS 源变更，宿主 `--lib` 与全量 `pnpm test` 未跑，口径同补记二百九十三/二百九十四）；`xcodebuild -list`＝双 target（AeroricNative＋AeroricNativeUITests）＋双 config（Debug/ReleaseLocal）＋AeroricNative scheme（另有 AeroricKit 包级 schemes AeroricClient/AeroricProtocol/AeroricUI/AeroricUIUnitTests＝包解析默认产物，非双真相，观察入 Q92）。
5. **回滚**:三笔提交各自独立 revert；`AeroricNative.xcodeproj` 是派生物，revert 后 `pnpm native:generate` 即恢复；无数据/格式迁移、无生产路径写入、TCC 零污染（新 bundle id 从未请求权限）；`.zcodeignore` 用户文件不入库。

**frontier 三条(产品决策，按提问轮格式；本轮用户不在场，处理照 P3-02/P3-04/P3-05/P3-06/P3-07 先例＝按推荐项记账、异议可重开，不当作用户答复)**:

- ❓ **Q92** - **XcodeGen 钉版策略**:`generate.mjs` 硬断言 xcodegen 2.46.0（与 `tooling.md` §5 双写）；两个静默语义坑证明小版本行为差异真实存在。➡️ 推荐:**维持钉版**，升级必须走 `tooling.md` §5 逐版验证＋`generate.mjs` 断言同步改＋两坑注释复核；包级 schemes 观察（轴 4）不构成双真相，不处理。**标注:用户未答，按推荐项记账。**
- ❓ **Q93** - **预览 bundle id 定型**:`com.aeroric.native.dev`（uitests 加后缀）与旧 `com.aeroric.desktop` 刻意隔离——预览期是红利（TCC/存储互不沾），但最终产品 id 迟早要定。➡️ 推荐:P4-01～P4-03 维持 `.dev`；**P4-04 真壳落地时**连同 Agent Family 设置一并定型最终 id 并一次性迁移（届时预览 id 使命结束）。**标注:用户未答，按推荐项记账。**
- ❓ **Q94** - **ReleaseLocal 的用途**:配置已建成（`-O`/wholemodule/`ONLY_ACTIVE_ARCH NO`）但脚本只用 Debug。➡️ 推荐:**保留**作为本地 release 冒烟配置（`xcodebuild -configuration ReleaseLocal` 随时可验优化档编译），发布链（含 Developer ID 签名与公证）归发布 Task，本轮不接脚本不接 CI。**标注:用户未答，按推荐项记账。**

**裁定(§4.2 四条判据对照)**:实际调用证据存在（本文件＋`skill://grilling` 本会话加载成功＋G31 三件套当场记）；必选测试通过（§2 当日矩阵:generate/工程形状/六步构建链/UI 测试/dev 探针/AC 单独复跑/skeleton/doctor 全 rc 0；签名 strict 校验三 helper＋主 bundle 逐件 validated）；无未解决的崩溃/数据丢失/安全越界/功能阻断问题（断言面误报根修；两语义坑探针＋断言封死；app 零存储访问是结构性边界）；中低风险明确由谁接受（Q92–Q94 按推荐项记账、具名可重开；死链接观察具名归 spec 维护轮）。**⇒ P4-01 标 accepted。**

## 2. 验收对照(2026-10-01 当日实测，退出码逐条单读)

| 命令 | 结果 |
|---|---|
| `pnpm native:generate` | rc 0——`Created project at macos/AeroricNative.xcodeproj`；钉版 2.46.0＋spec/产物存在性断言在链（两坑修法固化，§4.2②） |
| `xcodebuild -list -project macos/AeroricNative.xcodeproj`（工程形状） | rc 0——双 target（AeroricNative＋AeroricNativeUITests）＋双 config（Debug/ReleaseLocal）＋AeroricNative scheme 在列 |
| `pnpm native:build`(AC 六步链) | rc 0——xcodebuild **BUILD SUCCEEDED**＋三 helper（`aeroric-cli`/`__preview.dylib`/`AeroricNative.debug.dylib`）内→外 ad-hoc 签名＋`codesign --verify --strict` 逐件 validated＋主 bundle `valid on disk`/`satisfies its Designated Requirement` |
| `pnpm native:ui-test`(AC) | **TEST SUCCEEDED**——AppLaunchTests 1 用例 **5.2s passed**（冷启→`bootstrap.environment` 15s/`protocol`/`build` 各 5s 断言＋TEST_RUNNER_ 注入值精确比对＋`terminate()`；曾误报 label 空串，根修见 §4.2①；scratch 数据目录每次 rm+mkdir 全新） |
| `pnpm native:dev` | rc 0——直接拉 `Contents/MacOS/AeroricNative`（pid 67327）＋`AERORIC_DATA_DIR=.artifacts/native/dev-data`＋`AERORIC_ENV=dev` 注入＋1s `process.kill(pid,0)` 存活探针过；验证后 `kill` 杀净 |
| `xcodebuild … CODE_SIGNING_ALLOWED=NO build`(AC 单独复跑) | **BUILD SUCCEEDED**（增量 3.6s） |
| `pnpm native:check skeleton` | **24 case/188 用例 rc 0**（评审文件落盘后单轮复位，非代码回归） |
| `pnpm native:doctor` | **18/18 rc 0** |
| `inventory --check` → `--freeze` ＋ `digest --write`/`--check` | `--check` consistent（开工轮镜像验证在册）；freeze 在评审文件就位后 rc 0；task-status accepted 三处同步一致；digest `--check` rc 0 |
| `cargo clippy`/`cargo fmt --check` | 未跑（Rust 零面；`cargo build --locked` 只为 bundle helper 产物，无源变更） |
| 宿主 `--lib`／全量 `pnpm test` | 未跑（零宿主/前端面；口径同补记二百九十三/二百九十四） |
| `git diff --check` | 终批前复跑（随 `docs(plan)` 终提交的文档批一并核） |
| 真实家对账 | 结构性保证:app 零存储访问＋scratch 全新＋bundle id 隔离两层（轴 3），无用例直写 `~/.aeroric` |
| Swift／XCTest UI | `native:ui-test` 当日绿（见上）；`pnpm native:ui-test` 是 `macos/AeroricUITests/` 的唯一规定入口（spec 硬规则） |
