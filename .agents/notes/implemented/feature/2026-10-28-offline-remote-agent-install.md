# Agent Note: 远程 Agent 可以本机下载后经 SSH 传过去安装

Status: implemented

## Problem

远程主机缺 Agent 或版本太旧时，“在远程安装”会在远程运行 npm 或官方安装脚本，这要求远程自己能访问外网。很多远程机器没有代理，到 npm、downloads.claude.ai、build.stencil.so 都连不上，直接下载就会失败。没有 npm 的机器更是装不了 Codex。

## Decision

- 安装确认框给两种方式（胶囊滑块）：`direct` 远程直接下载（原方案）；`offline` 本机下载后传输。测试连接时远程顺便检查能否访问三个下载站点，并探测 `uname -s` / `uname -m` 和 musl。直接方式不可用（没有 npm 装 Codex），或远程上次连不上它要用的站点时，推荐 `offline`，用户仍可以切换。
- 传的是各家官方独立二进制，远程不需要 Node：
  - Claude 取 `downloads.claude.ai/claude-code-releases/<v>/<platform>/claude`，sha256 来自 manifest；
  - Codex 取 GitHub Releases 的 `codex-<triple>.tar.gz`，sha256 来自资源的 `digest`，Linux 版是 musl 静态链接；
  - omp 取 `build.stencil.so` 的 latest 构建，sha256 来自接口。
- 安装位置是 `~/.local/bin/<agent>`。Claude 按官方原生布局放 `~/.local/share/claude/versions/<v>`，再链接到 `~/.local/bin/claude`。Aeroric 所有远程命令（探测、会话、安装）都在登录 shell 里先把 `$HOME/.local/bin` 放到 PATH 最前，不改远程的 shell 配置文件。
- 本机缓存在 `<data>/remote-packages/<agent>/<platform>/<version>/`，同平台的多台主机共用。缓存带 `.sha256` 标记，下载新版后删掉同 agent 同平台的旧版本。连接页显示缓存大小，可以清空。
- 本机下载跟随 macOS 系统代理（`scutil --proxy`，依次取 HTTPS、HTTP、SOCKS）。没有系统代理时由 reqwest 读环境变量。为此宿主 reqwest 打开了 `socks` feature，与 core 同 crate 同版本，Cargo.lock 不变。
- 传输复用连接自己的 ssh：密码 askpass、`StrictHostKeyChecking=yes`。安装包经 stdin 流过去，远程脚本依次：收到临时目录 → 校验 sha256 → 解压（Codex）→ 放到位 → 跑 `--version`。用 `AERORIC_STEP` 行报告阶段，任何一步失败都由 `trap` 清掉临时目录，不留半个文件。本机下载也先写 `.part`，校验通过才改名。

## Alternatives considered

- **本机 `npm pack` 后传 tarball**：远程仍需 Node，而且 `npm install -g ./x.tgz` 仍会去 registry 拉依赖，没有网照样失败。否掉。
- **另起 scp / sftp**：多一次认证，还要再处理一遍密码和主机指纹。stdin 管道复用同一套 ssh 参数更简单。否掉。
- **新增“下载代理”设置**：系统代理通常已经配好，多一个设置只会增加出错点。否掉，以后有需要再加。
- **改远程 `.zshrc` / `.bashrc` 把 `~/.local/bin` 加进 PATH**：会动用户的配置文件。否掉，改成 Aeroric 自己的远程命令统一前置 PATH。

## Consequences

- **收益**：远程没有外网、也没有 Node 时也能装和升级三家 Agent；大文件下载一次，多台主机复用；两端都校验 sha256。
- **代价与已知上限**：
  - 只支持 macOS / Linux 的 arm64 / x86_64；
  - 单个包 100–280 MB，首次下载较慢；
  - 本机下载卡住时没有超时，只设了连接超时；
  - npm 装的旧版本仍在远程，只是被 `~/.local/bin` 遮住；用户在远程自己的终端里，PATH 不一定先找到新版本；
  - 三家下载接口的格式变了需要跟进（`remote_packages::resolve`）。
