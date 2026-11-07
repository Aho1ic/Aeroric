# Agent Note: 回溯评审 P1 A 批——DSH 模型参数、多行提交、钥匙串写入、链接改名、字面路径

Status: implemented

## Problem

2026-11-06 回溯评审确认了六个 core 侧问题：

- **B3-M1**：DSH 在配置下启动时，`--patch <file>` 追加在 `-- <prompt>` 之后。它变成了位置参数，模型覆盖不生效。
- **B4-30**：`git::service::commit` 用 `-m` 传说明，`validate_git_arg` 又拒绝换行，所以多行说明必然失败。界面在发出提交时就清空了草稿。
- **B3-G2**：Claude 登录续期后写回钥匙串时，凭据 JSON 的十六进制出现在 `security add-generic-password` 的 argv 里，`ps` 可见。
- **B3-G1**：宿主进程没有 USER 环境变量，钥匙串 account 回退成 `claude-code-user`，和 Claude Code 自己的不一致。
- **B4-18**：改名和移到废纸篓前，`resolve_within` 把末段的链接 canonicalize 掉了，结果操作的是真实目标；链接指向项目根时，能把整个项目移进废纸篓。
- **B4-25**：`restore`/`add`/`reset`/`checkout` 不按字面解释路径。放弃 `a*` 会连 `ab` 一起恢复。diff 却用了 `--literal-pathspecs`，预览和写入的范围不一致。

## Decision

- **M1**：新增 `option_before_prompt`，把选项插到第一个 `--` 之前；没有提示词时追加在末尾。
- **B4-30**：新增 `transport::run_git_with_input`，写端放在线程里。提交改用 `git commit -F -`，说明走标准输入。`validate_commit_message` 只拒绝空白和 NUL。
  - 不加 `--cleanup`：`-F` 不经编辑器，默认是 whitespace 模式，`#` 开头的行会保留。
  - 界面：`GitSidebarPanel` 记下本次提交的草稿（`committing`）。`store.working` 从 `GitPageText.committing` 变回 nil 时，如果没有 `actionError`，并且草稿和当时一样，才清空。
- **G2**：`security -i` 从标准输入读命令，凭据不进 argv，钥匙串项的创建者仍是 `/usr/bin/security`，Claude CLI 读取时不会弹授权框。
  - 实测 `security -i` 单行上限约 4KB：3001 字节通过，4201 字节失败。超长时退回旧的 argv 路径，因为 refresh token 已经轮换，写不进去就等于退出登录。
  - service 和 account 用双引号括起来；含引号、反斜杠或换行时拒绝。
- **G1**：没有 USER 时，用 `getpwuid_r(getuid())` 取用户名。`HostSupervisor` 启动宿主时也传入 `USER=NSUserName()`。
- **B4-18**：新增 `path_policy::resolve_entry_within`：父目录 canonicalize 后必须落在根内，末段不跟随链接，必须已存在，并且不能是项目根。`rename_path` 和 `delete_path_with_trash` 改用它。
  - trash crate 只 canonicalize 父目录、保留末段名字，Finder 删除的是链接本身。
  - 读写仍然走 `resolve_within`，跟随根内的链接。
- **B4-25**：transport 的三个 git 入口对带路径的写命令（`add`、`restore`、`reset`、`checkout`、`rm`）设置 `GIT_LITERAL_PATHSPECS=1`。查过这些调用方，没有依赖通配或 `:(…)` 魔法的。最初是对所有命令全局打开，结果 `git stash push -u` 会把未跟踪文件留在工作区（已实测复现，`git_stash_push_stashes_changes_and_validates_params` 也因此失败），所以收窄成这几个子命令。

## Alternatives considered

- **G2 改用 keyring crate 或 Security.framework**：钥匙串项的创建者会变成 Aeroric，Claude CLI 用 `security` 读取时会弹授权框。
- **G2 超长时直接报错**：refresh token 已经轮换，旧的作废，报错就等于让用户退出登录。退回 argv 只是短暂泄露在本机进程表里，代价更小。
- **B4-30 继续用 `-m` 并放开换行**：argv 里的换行本身安全，但 `validate_git_arg` 是全局守卫，为一处例外放宽它，会削弱其他调用。
- **B4-25 逐处加 `--literal-pathspecs`**：涉及十几处，以后新增的写操作容易漏掉；按子命令在 transport 里统一设置，一处就能覆盖。
- **B4-25 对所有 git 命令打开**：`stash -u` 内部依赖 pathspec 魔法，会失效。

## Consequences

- 原生改名和移到废纸篓现在只动链接。旧测试里“改名指向外部的链接会被拒绝”改为“只重命名链接”。
- 以后新增带路径的写命令，如果不在这五个子命令里，要把它加进 `literal_pathspecs` 的清单。
- 超长的 Claude 凭据仍会短暂出现在 argv 里，这是已知取舍。

## Verification

- 新增测试：
  - `the_dsh_patch_lands_before_the_prompt_terminator`
  - `a_multi_line_commit_message_is_kept_verbatim`
  - `a_wildcard_file_name_is_discarded_literally`
  - `the_keychain_write_goes_through_stdin_and_never_needs_escaping`
  - `the_account_falls_back_to_the_process_user_without_user`
  - `rename_and_trash_act_on_a_link_never_on_its_target`
- `security -i` 的行为在临时钥匙串文件里实测过：退出码会传出来，长度上限如上。没有碰登录钥匙串。
- 多行提交保留草稿的行为，靠 Codex Computer Use 目检验证。
