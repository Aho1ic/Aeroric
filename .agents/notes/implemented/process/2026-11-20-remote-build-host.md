# Agent Note: 全量测试和打包放到另一台 Mac 上跑

Status: implemented

## Problem

用户反映：每次打包、跑全量测试时，开发用的 MacBook 都会非常卡。

2026-11-20 的几次全量验证（`cargo test --workspace`、`swift test`、`pnpm native:build`）都在这台 MacBook 上跑：

- H2 第一步那次跑了 79 分钟，15 分钟负载平均值约 55；
- 两次都有测试在高负载下等 5 秒超时，单独重跑才通过。

用户有一台闲置的 Mac mini（M4、10 核、24 GB 内存），磁盘剩 120 GB 左右，希望把这类工作挪过去。

动手前核对的事实：

- **网络路径。** 虚拟机发往 Mac mini 的 SSH 连接被断开，读不到对方的版本行。MacBook 直连 Mac mini 正常。MacBook 上开着 Clash Verge 和 Tailscale，虚拟机的流量经 MacBook 转发。所以由 MacBook 连 Mac mini；登录密钥由用户自己装在 MacBook 上（`~/.ssh/id_ed25519_mini`），不经手密码。
- **Rust 版本。** MacBook 上实际用的是 Homebrew 的 Rust 1.93.0（PATH 里 `/opt/homebrew/bin` 在前），Mac mini 是 rustup 的 1.96.1。两边版本不同，`rustfmt` 和编译警告都可能对不上。
- **XcodeGen。** `native:generate` 钉死 2.46.0。MacBook 上是 Homebrew 的 2.46.0，Homebrew 现在只给 2.48.0。
- **签名。** MacBook 上的打包是临时签名（ad-hoc），没有设 `AERORIC_SIGN_IDENTITY`。Mac mini 上没有签名证书，但临时签名不需要证书。
- **体积。** 本机仓库里，git 忽略的目录很大：
  - `core/target` 138 GB；
  - `.claude/worktrees` 99 GB；
  - `.artifacts` 27 GB；
  - `../dbx` 的 `target` 54 GB。

  第一版脚本按固定名单排除，漏了 `.claude`，传了 20 GB 才发现，已停下并删除。

## Decision

**`scripts/native/remote.sh`**，在 MacBook 上运行，有五个子命令：

- `sync`：同步仓库和 `../dbx`；
- `run`：同步后在远端执行一条命令；
- `verify`：同步后跑全量验证；
- `build`：远端打包，成功才把 .app 拷回本机原路径；
- `status`：报告远端磁盘余量和产物大小。

远端主机、密钥、目录都由环境变量给出，仓库里不写地址。

**同步规则。**

- 按 `git ls-files --others --ignored --exclude-standard --directory` 排除，git 忽略的都不传，不维护名单。
- `.git` 照常传。
- `../dbx` 只传打好补丁的工作区，不传它的 `.git`。
- `--delete` 不会删被排除的路径，远端自己的产物会留下。
- 首次同步 15 秒，之后每次约 1 秒，远端副本约 700 MB。

**远端环境。**

- rustup 的 `cargo` 排在 PATH 前面，`RUSTUP_TOOLCHAIN` 钉成本机 `cargo --version` 的版本。远端已装 1.93.0 和它的 `rustfmt`。
- XcodeGen 2.46.0 从 MacBook 的 Homebrew Cellar 原样拷到 `<远端目录>/tools/`，排在 PATH 最前。为装它临时装的 Homebrew 2.48.0 已卸掉。
- 磁盘：
  - `CARGO_INCREMENTAL=0`，调试信息只留行号；
  - `core/target` 超过 60 GB（可配）先 `cargo clean`；
  - 每次结束报告磁盘余量。
- 同一时间只跑一次（远端 `.lock` 目录）。日志留在远端 `logs/`。
- 测试会在临时目录留下 `aeroric-*` 目录（几次全量下来约 100 MB，会越积越多），每次运行结束时删掉这次运行期间新建的，不碰别的应用的临时文件（2026-11-20 补）。

**提交仍在 MacBook 上做**，远端只有副本。审阅用的 App 仍按原规则：打包成功才重启。

## Alternatives considered

**虚拟机直接连 Mac mini。** 最强的理由是少一跳。没选：连接被 MacBook 上的转发拦下。要通，就得改用户的代理规则；而由 MacBook 连，本来就是代码所在的地方。

**在 Mac mini 上 `git clone`，用 `git pull` 同步。** 最强的理由是只传提交过的东西，干净。没选：验证总是发生在提交之前，未提交的改动正是要验的内容。另外 `../dbx` 带着本地补丁，不是任何提交。

**按固定名单排除。** 第一版就是这样，漏了 `.claude/worktrees`。按 git 的忽略规则排除，不会漏，也不用维护。

**用 Mac mini 上 Homebrew 的最新 XcodeGen，放宽版本钉。** 没选：版本钉是因为 2.46.0 的 `-p` 参数语义经过实测（`generate.mjs` 的注释），换版本要重新核对，不在这件事的范围里。

## Consequences

- **收益**：
  - MacBook 上不再跑编译和测试，只同步文件、做提交；
  - 首次在 Mac mini 上从头跑全量用了 13 分钟，其中 `cargo test --workspace` 10 分半；Rust 3926 个测试全部通过，结果与 MacBook 一致；
  - 远端打包加拷回共 135 秒，拷回的 .app 签名校验通过。
- **局限**：
  - Mac mini 的磁盘只剩约 95 GB，全量测试加打包后 `core/target` 占 24 GB；
  - 远端的 Node 是 22（MacBook 是 25），满足手册要求的 22.18+；
  - 真实窗口的 UI 自动化仍然暂停，与在哪台机器跑无关；
  - MacBook 上 `.claude/worktrees`（99 GB）和 `core/target`（138 GB）很占磁盘，本次没动。
- **验证**：在 MacBook 上实际跑了 `status`、`sync`（两次）、`verify`（第一次因 XcodeGen 版本失败，补装后打包单独通过）、`build`（拷回并核对签名）。
