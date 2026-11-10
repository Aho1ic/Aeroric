# Agent Note: Wiki 模块页骨架（R4-1）：结构解析、直接读到的依赖、来源版本

Status: implemented

## Problem

[Wiki 骨架](2026-10-31-wiki-skeleton.md)只有决策页、操作手册和索引，项目的代码本身不在 Wiki 里：Agent 想知道"某个模块管什么、入口在哪、依赖谁"，只能自己翻目录。路线文档 §4.2 把模块页列为第一种页型；[路线复核](../../proposed/architecture/2026-11-02-roadmap-recheck.md) §5 给代码索引定了规则：先用 LSP 或结构解析，图的边要分清直接读到（EXTRACTED）和推断（INFERRED），用 5–10 个真实问题做前后对照，没有净收益就不留。

用户 2026-11-10 定了 R4 的拆法，全部按推荐：

- R4-1：结构索引和模块页骨架，不调模型；
- R4-2：模型写的职责和常见坑，标为推断，加过时检测；
- R4-3：真实问题前后对照。

本篇只覆盖 R4-1。

## Decision

**结构索引**（`knowledge/code_index.rs`）

- **列文件**：git 项目用 `git ls-files -co --exclude-standard`，遵守 `.gitignore`；不是 git 的项目遍历目录，跳过 `target`、`node_modules`、`build`、`.venv` 等。两种都跳过 `vendor`、`third_party`、`external` 下的第三方代码。单个文件超过 512 KB 不读，最多 8000 个文件。远程（SSH）项目不做。
- **模块划分**：包下面再分一级。
  - Rust：`Cargo.toml` 里有 `[package]` 的是包，`src/` 下的顶层 `mod` 是模块，`lib.rs`、`main.rs` 属于包本身；
  - Swift：`Package.swift` 所在目录是包，`Sources/<目标>/<一级目录>` 是模块（直接在目标下的文件属于目标本身）；
  - Python：最上层带 `__init__.py` 的目录是包，子包是模块；
  - TS/JS：`package.json` 所在目录是包，去掉 `src/` 后的第一级目录是模块；
  - 其他语言（Go、Java、C 等）和不在任何包里的文件：顶层目录当包、第二层当模块，只列文件、不解析符号。
- **解析**：tree-sitter（Rust、Swift、Python、TypeScript/TSX 四个语法），只读每个文件顶层：
  - 公开符号：Rust 只算 `pub`（不含 `pub(crate)`）；Swift 去掉 `private`/`fileprivate` 和 `extension`；Python 去掉下划线开头的；TS 只算 `export` 出来的。类型排在函数前。
  - import：Rust 的 `use crate::x` 和 `use 其他crate::x`；Swift 的 `import 目标`；Python 的绝对和相对 import；TS 的相对路径和本项目包名。
- **依赖边**：一条 import 能对到本项目另一个模块，才记一条边，并记下读到它的文件、行号和 import 原文。只有这一种边（EXTRACTED）。
- **页数上限 60**：
  - 超过时，先把模块平均最小、且模块数不超过 30 的包整个合成一页；
  - 还超过，就每次把全项目最小的一个模块并进它所在包的"其他"页，页数自然留给代码多的地方；
  - 最后才按大小截断。
  - 被合并掉的模块记下去向。指向它的 import 会跟到它并进的那一页，不会错落到包本身。
- **来源版本**：每个文件记下 git blob hash，是按磁盘上的内容自己算的（和 `git hash-object` 一致），所以未提交的改动也有自己的版本。整页另记读取时的提交 `source_commit`。

**模块页**（`compile.rs`，页型 `module`，slug 形如 `module-core-crates-aeroric-core-sessions`）

- 一个模块一页，分组（`category`）是包名。小节依次是：
  - 概览：路径、语言、文件数、大约行数、所属包；
  - 入口：`lib.rs`、`mod.rs`、`__init__.py`、`index.ts` 等；
  - 关键类型与函数：最多 20 个，带 `文件:行号`；
  - 依赖（直接读到）：链到被依赖模块的页面，写出读到的文件、行号和 import；
  - 被谁依赖（直接读到）；
  - 文件：最多列 40 个。
- 每段的 `sources` 是对这一段有贡献的所有文件及其版本，`source` 是第一个（入口文件优先），用作脚注。
- 正文里不写会随每次编辑变化的东西：不写文件版本，不写提交号，行数只写大约值。只有来源版本或提交号变了时，存储层直接更新页面，不生成修订，免得改一次代码就多一条历史。
- 读不到项目目录时，已有的模块页原样保留，和读不到笔记时保留决策页一样。
- 索引页按包列出"模块 · 包名"分组，排在决策分组前面。

**编译时机**

- 巡检的指纹加上代码文件列表，算法只看路径、大小和修改时间，不读内容。编译时才完整扫描代码。
- `COMPILER_VERSION` 升到 4（第一轮目检后合并策略改过一次，又升了一级），所以已有的 Wiki 都会重编一次。

**界面**

- 页面树先列"模块 · 包名"分组，再列"决策 · 分类"分组，模块页的图标是 `shippingbox`。
- 页头加"读自提交 xxxxxxx"标签。
- 来源胶囊写"main.rs 等 N 个文件"，悬停显示每个文件和版本，点开就在项目里打开这个文件。
- 正文里的 `xxx.md` 链接现在点了会跳到对应的 Wiki 页面。以前索引页的链接点了没反应，这次一并修好。

## Alternatives considered

- **LSP（rust-analyzer、sourcekit-lsp）**：引用关系最准，还能拿到类型推断后的调用。没选：要求机器上装了各语言服务器，启动慢，Wiki 的定时巡检用不起；R4-1 只需要 import 层面的边。
- **逐行正则匹配**：最轻，不加依赖。没选：会把注释、字符串里的 `use`、`import` 当真，也会漏掉多行写法；边标成"直接读到"，就不能读错。
- **按模块数从多到少合并**：最直观。用真实数据试过：Aeroric 的 `aeroric-core` 有 40 多个模块，第一个就被合成了一页，最有价值的页面全没了，所以改成先合并模块小的包。
- **一次把某个包最小的 k 个模块合进"其他"**：也试过，结果 Swift 包被整个清空了。
- **每次在模块最多的包里并一个**：第一轮目检时用的是这个，结果各包被拉到差不多的页数。`aeroric-core` 有 22 万行，只剩 18 页，和 6.6 万行的 `aeroric-host` 一样多。改成每次并全项目最小的模块后，`aeroric-core` 24 页、`AeroricKit` 22 页、`aeroric-host` 6 页。
- **把文件版本和提交号写进正文**：改一行代码就会让整页的文字变化，修订历史会被巡检刷满。所以版本只放在结构化的来源里。

## Consequences

- **收益**：
  - Agent 用 `read_wiki`、`search_wiki` 能直接查到模块的入口、公开类型和依赖关系，依赖关系每一条都有出处。
  - 每段记下了贡献文件的版本，R4-2 的过时检测可以直接拿来比。
- **代价**：
  - 新增 tree-sitter 和四个语法 crate，以及 `sha1`（锁文件里已有），编译变慢，宿主体积变大。
  - 依赖边只认 import：Rust 里不经 `use` 直接写全路径的引用、同一个 Swift 目标里跨目录的引用（Swift 同目标不需要 import）都没有边。
  - 不在任何包里的 Swift App 源码（例如 `macos/AeroricApp`）按目录分，粒度比较粗。
  - 大仓库编译一次要读全部代码：Aeroric 本身 debug 构建约 11 秒，指纹约 50 毫秒。只有指纹变了才编译，但边改代码边巡检时，每 2 分钟可能编一次。
- **验证**：
  - core 单测（`knowledge::code_index_tests`）：四种语言加其他语言的模块划分，跳过构建目录和第三方目录；只读公开符号；依赖边只来自对得上的 import，并记下文件、行号和原文；blob hash 与 git 一致；超过 60 页时的合并顺序；模块页的依赖链接、被依赖、来源版本、索引分组；只有版本变化时不产生修订；读不到目录时保留模块页。
  - Swift 单测（`WikiTests`）：模块分组在决策分组前；代码来源能打开文件，悬停显示版本；`xxx.md` 链接解析成 slug；页面带提交号。
  - 被合并的模块：一个 crate 有 62 个小模块时，被并掉的模块，其 import 指向"其他"页。
  - 真实数据：在 Aeroric 主检出目录上扫描，得到 60 个模块、没有截断。`sessions` 读到 5 条依赖：
    - `crate::ide::{diagnostics_face, tests_face}`，在 `auto_check.rs:25`；
    - `crate::paths`，指向"aeroric-core · 其他"。
  - 第一轮目检（2026-11-10，Codex Computer Use，审阅版）。Aeroric 编译出 143 页，60 个模块页分 11 个包，都排在决策分组前面。sessions 页的页头"读自提交 beec1c6"与 HEAD 一致，六个小节齐全。这些都通过了：
    - 来源胶囊"mod.rs 等 84 个文件"，点开就打开文件，AX 帮助里有文件和版本；
    - 依赖链接、索引里的模块和决策链接都能跳到对应页面；
    - 再编译一次"没有变化"，修订仍是 v1。

    同时发现了上面那条"拉平页数"的问题，以及 import 指向被合并模块时边落错页，都已修复。悬停提示的视觉效果没法用 Computer Use 验证。
  - 复测（2026-11-10）：11 个模块分组共 60 页，aeroric-core、AeroricKit、aeroric-host 分别 24、22、6 页，三者各有一张"其他"页；sessions 的 5 条依赖里 `hooks.rs:21` 的 `crate::paths::AppPaths` 指向"aeroric-core · 其他"，点击能跳过去。
