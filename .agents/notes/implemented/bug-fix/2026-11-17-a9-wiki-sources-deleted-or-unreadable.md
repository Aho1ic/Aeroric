# Agent Note: A9 Wiki 模块说明：来源被删、读不到要分开说

Status: implemented

## Problem

[批次 A](./2026-11-15-adoption-a-truthful-state.md) 的 A9。2026-11-17 复核：

- **被删的来源不算过时。** `knowledge/explain.rs` 的 `Explanation::changed` 只返回"现在还在、内容不同"的文件。模块说明读过的文件被删了，页面上的"职责（推断）""常见坑（推断）"不会标过时，空闲时也不会重写。
- **读不到和被删分不开。** `code_index::scan` 遇到下面三种文件都直接跳过，扫描结果里看不出区别：
  - 列出来了但 `metadata` 失败；
  - 超过大小上限；
  - 读取失败（比如权限被收回）。
- **整个模块读不到时页面被撤掉。** 模块里的文件全都读不到时，这个模块不在扫描结果里，编译就把它的页面当成"来源已删除"撤掉。只有整个项目文件夹读不到（`gaps.all`）时才会保留模块页。
- 宿主空闲时挑"该重写"的模块（`wiki.rs` 的 `wanted`），用的也是同一个 `changed`。
- 第二份评审的 NR-06（`knowledge/notes.rs` 把读不到的笔记当成不存在）已经在 R1-1 修过：`notes::scan_checked` 有 `unreadable`，编译保留对应的决策页。这次不用再改。

## Decision

**扫描记下"在但读不到"和"太大"；模块说明的来源分成改过、被删、读不到三种；读不到不改写、不撤页。**

### 扫描

`CodeIndex` 加两个集合：

- `unreadable`：列出来了，文件在，但这次读不了（`metadata` 或 `read` 的错误不是"不存在"）；
- `too_large`：超过大小上限、没进索引的文件。

"不存在"（`NotFound`）的文件不进任何集合，就是不在了。

### 三种状态

- `Explanation::sources_now(current)` 取代 `changed`。`current` 把路径对到现在的 blob，读不到的对到 `None`；不在里面的就是不在了。说明读过的每个文件，返回下面三种状态之一（没变的不返回）：
  - `Changed`：在，内容不同；
  - `Removed`：不在了（删了，或移出了这个模块）；
  - `Unreadable`：在，但这次读不到。
- 编译时 `current_for` 这样组 `current`：
  - 模块里的文件用现在的 blob；
  - 说明读过、这次在 `unreadable` 里的记成 `None`；
  - 在 `too_large` 里的记成空 blob，算改过（文件变大了）。
- `needs_rewrite`：有 `Changed` 或 `Removed` 才重写；只有 `Unreadable` 时不重写。

### 页面上怎么说

说明块顶部按原因分行：

- `> 可能过时：生成后 `a` 改过；`b` 被删了或移出了这个模块。`
- `> 待核对：`c` 这次读不到，没有据此改写。`

两行可以同时出现。只有一个文件改过时，文字和原来一样。这是写在页面内容里的引用块，App 原样渲染，Swift 不用改。

### 空闲重写

模块页概览块的来源里，额外放进"说明读过、这次读不到"的文件，不带版本。宿主的 `wanted` 用概览来源组 `current`：

- 带版本的照常比较；
- 不带版本的算读不到；
- 不在里面的算被删。

只有 `needs_rewrite` 为真才排进重写。

### 不撤页

`compile_checked` 多保留一种模块页：这个模块不在这次的扫描结果里，而它列出的某个文件在 `unreadable` 里。保留的页面原样不动。文件真的删掉后，下一次编译照常撤页。

### 和批次笔记的出入

- 批次笔记说"Swift：Wiki 页的状态条显示这三种原因"。原因是写在页面内容里的引用块，Wiki 页本来就渲染出来，所以没有改 Swift。
- 批次笔记说"扫描结果带一个完整性标记"。没有加整体的标记，改成逐文件记 `unreadable` 和 `too_large`：逐文件能说出是哪个文件读不到，整体标记只能说"这次不完整"。
- NR-06 已在 R1-1 修过，见上。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [批次 A](./2026-11-15-adoption-a-truthful-state.md) | 上级方案 | 原地补 A9 的落地情况和出入；A9 状态和待目检项回写路线文档 §8 |
| [Wiki 模块说明](../feature/2026-11-10-wiki-module-explanations.md) | 被修正的事实 | 原地补"过时"的三种原因 |

## Alternatives considered

**扫描结果只带一个"完整/不完整"标记，不完整时不判任何来源被删。** 最强的理由是简单。没选：一个文件读不到，整个项目的"被删"都不判了，真被删的来源也不说。逐文件记下来，只有那个文件算"待核对"。

**读不到的文件当成改过，照常重写。** 最强的理由是保守，宁可多写一次。没选：读不到时模型也拿不到这个文件，重写出来的说明反而丢了它，还白用一次每日额度。

**在 `wanted` 里重新扫一遍代码，不靠页面里的来源。** 最强的理由是不往概览块里放不带版本的来源。没选：`wanted` 在状态查询和空闲轮询里都会调用，每次扫全项目太重；编译刚扫过，结果存在页面上就够了。

## Consequences

- **收益**：
  - 来源被删的说明会标"可能过时"，空闲时会重写；
  - 读不到的来源只标"待核对"，不重写、不撤页。
- **代价**：
  - 概览块的来源里可能有不带版本的文件，表示这次读不到。
  - 文件移到另一个模块也算"被删了或移出了这个模块"，会触发重写。这是对的：模块的内容确实变了。
  - "扫描中途来源消失"（列出来时在、读时没了）按"不在了"处理，和删除一样。
- **验证**：
  - `explain_tests`：
    - `sources_are_changed_removed_or_unreadable_and_said_apart`：三种状态、文字（两行同时出现、"等 N 个文件"、没有就为空），只有读不到时不重写。
    - `an_unreadable_source_is_to_be_checked_and_a_deleted_one_is_stale`：把说明读过的文件 `chmod 000`，扫描记进 `unreadable`，页面写"待核对"，概览来源里它没有版本，`needs_rewrite` 为假；删掉后不在 `unreadable` 里，页面写"被删了或移出了这个模块"，`needs_rewrite` 为真。
    - `a_module_whose_files_cannot_be_read_keeps_its_page`：模块唯一的文件读不到时，模块不在扫描结果里，页面原样保留；删掉后页面撤掉。
    - 原有的"改过"测试改用 `sources_now`，文字不变。
    - 以 root 运行时 `chmod 000` 拦不住读取，两个文件权限测试直接返回。
  - 2026-11-17 在 Mac 上：
    - 只格式化本次改动的文件，`cargo clippy -p aeroric-core -p aeroric-host --all-targets -D warnings` 通过；
    - `knowledge` 测试 31 个通过；
    - `cargo test --workspace --no-fail-fast` 3755 通过、0 失败；
    - `pnpm verify-notes` 通过。
  - 宿主 `wanted` 的改动没有单独的测试，它和编译共用 `sources_now`、`needs_rewrite`，上面的测试用同样的方式（概览来源）组 `current`。界面没有目检，记在路线文档 §8 待目检清单。
- **2026-11-18 补强**：包清单读不到、目录列不出、文件超大时也保留模块页（B1-9），见 [全量审查 Batch 1](./2026-11-18-review-batch-1-core-gaps.md)。
