# Agent Note: 参考代码库（Q-R6）

Status: implemented

## Problem

用户在本机收集了很多别人的仓库（`LYX/` 下就有几十个），写代码时常想看"别人怎么实现同类功能"。Agent 看不到这些文件夹：Claude 要 `--add-dir` 才读得到，Codex 虽然能读，却不知道这些文件夹在哪、里面有什么。路线复核把它列为 Q-R6（[路线复核](../../proposed/architecture/2026-11-02-roadmap-recheck.md)）：把用户收集的仓库建只读索引，给 Agent 检索工具，复用 R4 的来源管理，先只支持本机文件夹。原型是 xerj；它自己的案例研究只有 8 个任务，并说明对模型已熟悉的热门库可能没帮助。

用户 2026-11-13 让继续推进，第一批按推荐：

- 设置里一个全局"参考代码库"列表，加本机文件夹，可起名、写一句用途；所有项目共用；
- 文件列表、全文检索、读文件，加上代码索引的函数名和类名；不做向量；改动按指纹发现，后台增量重建，不替用户 `git pull`；
- 上下文 MCP 加 `search_references` 和 `read_reference`，路径限制在登记的文件夹内；默认开，只加工具说明，不往上下文里塞内容；
- 第一批不做前后对照评测，放到第二批；目检攒到待目检清单里一起做。

## Decision

**core：`knowledge::references`**

- 列表存在 `<data>/references/list.json`，每项有 id、名称、规范化后的绝对路径、用途、加入时间。同一个文件夹只能加一次。id 由名称和路径哈希组成。
- 每个文件夹一个 `<data>/references/<id>/index.db`（SQLite）：
  - `files`：读过的文件及其大小和修改时间。重建时只读变了的文件，删掉已经不在的。
  - `chunks`：每 40 行一块，进 FTS5 索引。正文用 Wiki 同一套中文分词（`notebook::rag::cjk`），块里带上路径，路径里的词也能搜到。
  - `symbols`：顶层定义（类、函数、结构体……），来自代码索引的解析器（新增 `code_index::file_symbols`）。只有 Rust、Swift、Python、TS/JS 有解析器，其他语言只能全文搜。
  - `meta`：指纹、索引时间、是否截断。
- 收哪些文件：
  - 是 git 仓库就用 `git ls-files -co --exclude-standard`，`.gitignore` 照样生效；不是就遍历，跳过 `node_modules`、`target`、`build` 等目录和隐藏目录。
  - 只收文本类扩展名（源码、Markdown、配置）和 `Makefile`、`Dockerfile` 这类文件名。
  - 单个文件超过 512 KB 不收，前 8 KB 里有 NUL 字节的当二进制跳过。
  - 最多 2 万个文件，超出就截断并标出来。
- 指纹：列出的文件路径、大小、修改时间合起来哈希，不读内容。设置页列表和每次检索都会比一次，变了就是"有改动"。
- 检索（`References::search`）：
  - 先按定义名精确匹配（不分大小写，结果标 `symbol:<kind>`）；
  - 再按 FTS5 的 bm25 排全文块（词之间是"或"）；
  - 每块回到文件里找第一行含查询词的行，给出行号和这一行。
  - 跨文件夹合并、去重，一次最多 12 条。可以用名称或 id 只搜一个库。
- 读文件（`References::read`）：
  - 路径必须是相对路径，不能含 `..`。
  - 规范化之后还得在文件夹里，所以符号链接也跳不出去。
  - 二进制和超过 2 MB 的文件不读。
  - 一次最多 400 行，带行号；没读完会说还剩多少行、从哪一行接着读。
- 移除只删列表项和索引，文件夹本身不动。
- 同一个文件夹同时只有一个索引任务（`begin_index`）。

**宿主：`session_live_services/references.rs`**

- 给设置页的 RPC：
  - `references.list`：每项带文件数、定义数、索引时间、是否截断、正在索引、有改动、出错原因；
  - `references.add {path, name?, purpose?}`：加入后马上在后台线程建索引；
  - `references.remove {id}`；
  - `references.reindex {id}`。
- 给上下文 MCP 的 RPC：
  - `context.referencesAvailable`：令牌有效、列表非空时为真。
  - `context.referenceSearch {token, query, repo?}`：发现某个库有改动就在后台重建，这次先用旧索引回答，并注明"正在更新索引，结果可能不全"。
  - `context.referenceRead {token, repo, path, start?, end?}`。
  - 这几个只看令牌有效，不按项目限制：参考库是用户的，不是哪个项目的。
- `References` 和 Wiki 一样挂在 `StructuredExecutor` 上（`executor.references()`）。

**上下文 MCP（`aeroric-cli/src/mcp_context.rs`）**

- `tools/list` 在 `context.referencesAvailable` 为真时加上 `search_references`、`read_reference`。`initialize` 的说明里也写上"搜、读用户收集的参考代码库"：Codex 只看这句说明决定要不要加载工具（见 [Wiki 评测](../testing/2026-11-10-wiki-agent-eval.md)）。
- 询问宿主同样受 3 秒上限约束（见 [通用 ACP 驱动](2026-11-08-acp-dialects.md)）。

**界面：设置 → 参考代码库**

- 新的设置分栏，宿主回答了 `references.list` 才显示。
- 每个文件夹一行：
  - 名称、路径、用途；
  - 状态：文件数和定义数、正在建索引、有改动、文件太多只收了一部分、索引失败的原因；
  - "重建索引""移除"两个胶囊按钮。移除前确认，并说明只删索引。
- "添加文件夹…"打开系统选择框，再填名称（默认文件夹名）和用途，点"添加"。
- 有文件夹在建索引时，每 2 秒刷新一次，建完就停。

## Alternatives considered

- **向量检索（embedding）**：能搜"意思相近"的代码。没有采用：要额外的模型和存储，路线图 §9 第一版不做向量服务；函数名、类名加全文检索对"别人怎么写某个类"这类问题够用，第二批评测后再看。
- **按项目登记参考库**：不同项目各配各的。没有采用：用户收集的仓库多数跨项目都有用，按项目登记要重复添加；需要时可以用 `repo` 参数只搜一个库。
- **直接让 Agent 读文件夹（`--add-dir` 或写进说明）**：不用建索引。没有采用：Claude 每个文件夹都要启动参数，而且没有检索，Agent 不知道该看哪个文件。
- **代码索引的整库扫描（`code_index::scan`）**：现成的模块和定义。没有采用：它每次重读全部文件，每个模块只留 20 个定义，上限 8000 个文件；参考库要的是按文件增量更新、全部定义。只复用了它的单文件解析器。

## Consequences

- 收益：
  - Agent 能在动手前查用户自己收集的实现，结果带仓库、路径和行号，可以接着读全文；
  - 改动按文件增量更新，大仓库第二次索引很快；
  - 只读，路径出不了文件夹。
- 代价：
  - 每个文件夹一份 SQLite 索引，大仓库会占一些磁盘；
  - 设置页和每次检索都要算指纹（列文件加 stat），文件很多的库会慢一些；
  - 只有四种语言能按定义搜，C/C++、Go 等只有全文；
  - 指纹只看列出的文件，git 忽略的改动不触发重建。
- 净收益验证：xerj 自己说热门库可能没帮助。R6-2 用 mmyolo 的 6 个真实问题做了前后对照：两组都是 24/24，R 组总用时少 26.5%，按事先的规则算有收益，所以保持默认开启。噪声很大，见 [R6-2 评测](../testing/2026-11-13-reference-library-eval.md)。
- 待目检：设置页的添加、列表、重建、移除；会话页上参考工具调用显示成什么样。Agent 在真实会话里调用这两个工具，已由 R6-2 评测经宿主 RPC 验证。已记进路线图的待目检清单。

## Testing

- `knowledge::references` 测试：
  - 登记、去重、拒绝不存在的文件夹；
  - 第一次索引只收文本文件，跳过 `node_modules` 和二进制，第二次什么都不重读；
  - 定义优先、中文全文、按库检索；
  - 改一个文件、删一个文件后只重读改的，并发现有改动；
  - 读文件带行号，拒绝 `..`、绝对路径和符号链接逃逸；
  - 移除不动文件夹。
- 宿主集成测试 `the_reference_library_is_searched_and_read_by_an_agent`：后台建好索引，令牌检索到定义、读到指定行，跳出文件夹的路径和无效令牌被拒绝，移除后文件夹还在。
- `aeroric-cli` 测试：工具到宿主方法的映射，说明里提到参考代码库。
- Swift `ReferenceLibraryTests`：
  - 解析一行和状态文字；
  - 宿主不支持时隐藏分栏；
  - 添加后轮询到索引完成，移除；
  - 添加被拒时显示原因。
