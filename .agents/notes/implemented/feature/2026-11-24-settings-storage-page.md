# Agent Note: 设置 › 存储

Status: implemented

## Problem

设置方案（`proposed/feature/2026-11-24-settings-redesign.md`）的“数据与隐私”里有一页“存储”：
- 数据目录的占用条，分会话、记忆与索引、参考代码库索引、工作副本、日志；
- 每项写大小，并有“在访达中显示”。

设置界面第一版没做，因为宿主没有算目录大小的方法。

在 MacBook 的数据目录上量过：
- `agent-homes` 约 1 GB（各 Agent 的配置目录，会话记录在里面）；
- `remote-packages` 约 330 MB（发到远程主机的程序包）；
- `Fonts` 约 110 MB；
- 其余都在几十 MB 以下。

工作副本不在数据目录里，在各项目的 `.aeroric/worktrees`。数据目录里的 `copies` 是独立副本（R2-3b）。

## Decision

**core：`maintenance::storage_usage::measure(data_dir, project_roots)`，只读。**
- 数据目录第一层的每一项按名字归到一类（`group_of`）：
  - 会话与 Agent：`agent-homes`、`projects`、`turns`、`checkpoints` 等；
  - 记忆与索引：`memory`、`knowledge`、`recall`；
  - 参考代码库索引：`references`；
  - 工作副本：`copies`，加上每个已登记项目的 `.aeroric/worktrees`（同一个目录只算一次）；
  - 日志：`logs`；
  - 远程主机的程序包：`remote-packages`；
  - 没列到的都算“其他”（字体、各种小配置文件）。
- 大小按磁盘上实际占的块算（`blocks × 512`），不是文件长度。
- 走目录时不跟符号链接：链接只算它自己，不把它指向的项目或主目录算进来。
- 读不了的文件和目录不算大小，只记个数（`unreadable`），页面上写出来。
- 返回七类，顺序固定，空的也有；每类的条目按大小从大到小排。

**宿主：`storage.usage {}`**，在阻塞线程上量，项目目录取自已登记的项目。

**Mac：设置“数据与隐私”下新增“存储”页**（宿主连上才出现）。
- **“占用”一组：**
  - 写数据目录共多大，和读不了的项数；
  - “在访达中显示”“重新计算”两个按钮；
  - 下面一条按类分色的占用条，加图例（类名和大小）。
- **每一类一组：** 列出最大的 8 项，每项写名字、大小和“在访达中显示”；更多的写“还有 N 项较小的没列出”。
  - 数据目录里的项写相对名字，其他路径写全，主目录写成 `~`。
- **什么时候量：** 打开页面时量一次，之后只在点“重新计算”时量。
- **搜索：** 登记了“数据目录”，搜“存储”“磁盘”“占用”能找到。

**顺带：** “允许手机改文件”打开前要确认。方案定的是“打开要确认”，上一版漏了。关掉不用确认。

## Alternatives considered

**每次打开设置就在后台量好。** 没选：`agent-homes` 上万个文件，走一遍要一会儿，打开设置不应该触发磁盘扫描。只在进了这一页时量。

**用 `du` 命令。** 没选：要起外部进程，输出还得解析。Rust 自己走目录可以控制不跟链接、能数清读不了的项，也能在测试里造目录核对。

**给每一类加“清理”按钮。** 没选：方案只要“管理”或“浏览”。删数据牵涉会话记录和记忆的一致性，现有的清理入口（旧版 Agent 清理、工作副本自动清理）各有规则。这一页只看、不删。

## Consequences

- **收益：**
  - 能看出数据主要占在哪里，比如 Agent 配置目录、远程程序包；
  - 能直接在访达里找到对应目录。
- **局限：**
  - 量的时候没有进度，大目录要等几秒；
  - 远程项目（SSH）的工作副本在远程主机上，量不到；
  - 只看、不删；
  - 界面没有目检。
- **验证：**
  - Rust：
    - `the_data_folder_is_measured_by_kind`（分类、顺序、项目工作副本只算一次、从大到小排）；
    - `a_symlink_counts_as_itself_not_what_it_points_at`；
    - `a_missing_data_folder_says_it_could_not_read_it`；
    - 宿主 `storage_usage_reads_the_data_folder_by_kind`；
  - Swift：
    - `StorageClientTests`（缺 id 的类、缺路径的项丢掉，超过 32 位的数）；
    - `testStorageRowsNameTheirFolders`；
    - 搜索测试里加了“磁盘”。
