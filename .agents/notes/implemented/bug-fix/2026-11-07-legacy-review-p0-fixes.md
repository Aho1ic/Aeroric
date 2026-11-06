# Agent Note: 回溯评审的 4 个 P0——改名不覆盖、文件树能打开、Git 大输出不卡、远程 Codex 不带本机 MCP

Status: implemented

## Problem

2026-11-06 用 Codex 对 10-30 之前的功能做了回溯评审和 Computer Use 目检，发现 4 个优先级最高的问题：

- **B4-17**：`files::read_write::rename_path` 直接调用 `fs::rename`，目标存在时会被静默覆盖，被覆盖的文件不进废纸篓。旧测试还把“原生改名可以覆盖”写成了预期行为。
- **B4-36**：资源面板的文件列表上，双击、回车、右键都没有反应（三次目检都复现）。列表是单选的 `List(selection: Binding<String?>)`，选择值类型是 `String`，可行标签用的是 `Optional(id)`，`contextMenu(forSelectionType:)` 声明的也是 `String?`，两边类型对不上。
- **B4-28**：`git::transport::collect_output` 先等进程退出，再去读管道。git 的输出一旦超过管道缓冲（约 64KB）就会阻塞在写端，永远等不到退出，最后报超时。实际的写操作可能已经成功，却显示为失败。
- **B2-12**：`executor.rs` 用 `Route::Codex(_) | Route::Claude(_)` 判断要不要注入上下文服务器，远程的 `ServerKey::Remote` 也被匹配进去。远端机器上没有本机的 `aeroric-cli`，而这个服务器又是 required 的，首轮可能直接起不来。

## Decision

- **改名**：用 `symlink_metadata` 检查目标是否已被占用，悬空链接也算占用。目标和源是同一个条目（dev 与 inode 都相同，也就是大小写不敏感卷上只改了大小写）时放行。其他情况一律拒绝，返回 `RENAME_TARGET_EXISTS`，界面原样显示。检查和 rename 之间仍有 TOCTOU 窗口，这和本模块其他写入一致。
- **文件树**：右键菜单的类型改成 `String.self`，文件行和目录行的标签都改成 `.tag(row.id)`。
- **Git**：子进程一启动就开两个线程分别读 stdout 和 stderr，主线程照旧轮询退出状态并处理超时。超时时不 join 读线程，因为孙进程可能还握着管道。
- **远程 MCP**：新增 `Route::runs_locally()`，只有本机 Codex（Agent/Config）和本机 Claude 返回 true。

## Alternatives considered

- **改名自动加后缀，或弹窗确认覆盖**：自动加后缀会改掉用户输入的名字；弹窗确认要新增界面，而且覆盖依然不可撤销。直接拒绝最简单，也最安全。
- **用 `renamex_np(RENAME_EXCL)` 做原子独占改名**：只有 macOS 有，要写 FFI；只改大小写的场景还得单独处理。目前先做检查即可。
- **文件树改成每行挂一个 `.contextMenu` 再加双击手势**：会和 List 原生的选择与键盘行为冲突。先修类型，如果目检仍不通过再换方案。
- **Git 换用 `wait-timeout` crate 或 `Command::output()`**：前者要多引一个依赖；后者没有超时。
- **把上下文服务器转发到远端**：要处理反向端口转发和 token，属于另一项功能。

## Consequences

- 原生改名和旧版改名的冲突策略统一成“拒绝”。旧测试里“native overwrite is a different policy”这条预期已经改掉。
- 远程 Codex 会话暂时用不了上下文 MCP，和远程 Claude 一致。
- Git 输出完整读进内存，大小没有上限。diff 的大小守卫仍由各调用方负责。

## Verification

- 新增测试 `rename_never_overwrites_but_allows_a_case_only_change`。改写 `legacy_rename` 的冲突测试，覆盖已占用和悬空链接两种情况。
- 新增 `output_larger_than_a_pipe_buffer_is_read_before_the_timeout`：读取 200KB 以上的 `git show`。
- 新增 `only_local_routes_get_the_local_context_server`。
- 文件树的修复靠 Codex Computer Use 目检验证。
