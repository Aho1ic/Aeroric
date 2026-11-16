# Agent Note: PATH 上排在前面的 CLI 安装坏了时，跳过它用后面能用的那份；版本检查失败时报出 stderr

Status: implemented

## Problem

2026-11-14 做接力讨论对照时，审阅版里的 Claude 成员每场都启动失败，报错是"无法识别 Claude Code 版本："，冒号后面是空的。

查下来有三层原因：

- 这台 Mac 上有两份 `claude`：
  - `~/.local/bin/claude` 是一份残缺的 npm 安装，没有原生二进制，运行时只往 stderr 打出 "claude native binary not installed"，退出码 1；
  - `/opt/homebrew/bin/claude` 是好的，版本 2.1.322。
- 宿主启动内置 Claude 时用的是裸名 `claude`，由子进程环境里的 PATH 查找。这台 Mac 的登录 shell PATH 里 Homebrew 排在前面；但宿主读不到登录 shell 的环境时，会退回 `build_fallback_path`，这份备用 PATH 里 `~/.local/bin` 排在 `/opt/homebrew/bin` 前面。这很可能就是选中坏那份的原因，不过没有直接证实。
- 版本检查（`spawn_checked`）只看 stdout。坏安装把原因写在 stderr 里，所以界面上只看到一个空的版本号，看不出原因。

## Decision

都在 core `sessions::live::codex::server` 的 `spawn_checked` 里。用它启动、并且检查版本的是内置 Claude 和 OMP；DSH 的版本检查恒为通过，不受影响。

- **`version_passes`**：跑 `<program> --version`，交给各自的版本检查。检查不通过时，在报错后面加上程序写到 stderr 的第一行非空内容，例如"无法识别 Claude Code 版本：（claude 报错：Error: claude native binary not installed.）"。
- **`checked_program`**：
  - 先检查 `spec.program`，通过就用它。
  - 不通过、并且 program 是裸名（不含 `/`）时，在子进程环境的 `PATH`（没有就用宿主自己的 `PATH`）里，按顺序列出同名的可执行文件（`path_copies`）。跳过第一份，因为它就是刚才失败的那份；同一个真实文件（`canonicalize` 后相同）只试一次。用第一份通过版本检查的完整路径去启动。
  - 都不通过时，返回第一次的报错，它已经带上了 stderr。
  - program 是路径，即用户在设置里填了 `claude_path` 这类，不再另找：用户指定的就是用户要的。
- 版本检查原本每次启动都要跑；只有第一份失败时才会多试几份，正常情况没有额外开销。

## Alternatives considered

- **改 `build_fallback_path` 的顺序**，把 Homebrew 排到 `~/.local/bin` 前面：一行改动。没有采用：哪份在前本来就没有对错，Claude 的原生安装器装在 `~/.local/bin`，别的机器上坏的也可能是 Homebrew 那份；而且读得到登录 shell 环境时，顺序是用户自己的 PATH 决定的。按"能不能用"挑，比按位置挑更稳。
- **启动前先用 `which -a` 挑一份并缓存**：只挑一次。没有采用：用户修好或装坏 CLI 后要重启宿主才生效；现有的版本检查本来每次启动都跑，失败时顺着往下找就够了。
- **只在报错里带上 stderr，不自动换一份**：行为改动最小。没有采用：用户仍然要自己去删坏的那份，Claude 会话才能启动；而 PATH 上明明有一份能用的。

## Consequences

- 收益：
  - PATH 上排在前面的那份坏了时，内置 Claude 和 OMP 会话会自动改用后面能用的那份；
  - 都坏的时候，报错里能看到 CLI 自己说的原因。
- 代价：
  - 用到的可能不是用户以为的那份；版本检查通过的那份才会被使用，两份版本不同时，可能用到较旧的一份（只要不低于最低版本）；
  - 只看 `--version` 是否通过；一份能答出版本、却在真正运行时坏掉的安装，挑不出来；
  - Codex 的 `process_launcher` 走的是另一条路（版本只查一次），没有接这套逻辑；
  - 这台 Mac 上那份残缺的 `~/.local` 安装没有动，要用户自己删掉或重装。

## Testing

- core `codex::server` 新测试 `the_first_copy_that_passes_down_the_path_is_used`（只在 Unix 上跑）：
  - 两个临时目录里各放一个同名脚本，第一个只往 stderr 报错，第二个答出版本；裸名启动时选中第二个；
  - 只有坏的那份时，报错以原来的检查报错开头，并带上 stderr 那一行；
  - program 是绝对路径时，不另找。
- 没有在审阅版里重测真实的 Claude 启动：这要求宿主恰好读不到登录 shell 的环境，列进了待目检清单。
