# Agent Note: 远程会话经 SSH 反向转发使用本机网关与 Agent 预设

Status: implemented

## Problem

远程项目只能用远程机器自己的登录：Agent 预设要连本机网关，而网关只监听这台 Mac 的 127.0.0.1，远程连不到，所以选择器把预设标成"即将接入"，`route_remote` 也拒绝带预设的任务。用户想在远程项目里也用 Aeroric 管理的预设。

## Decision

- 带预设的远程会话在同一条 ssh 里加 `-R 127.0.0.1:<远程端口>:127.0.0.1:<网关端口>` 和 `ExitOnForwardFailure=yes`。远程端口随机取 20000–59999。版本探测那条 ssh 带同样的转发，端口被占时换端口重试，最多 3 次；3 次都失败就报"这台主机不允许 SSH 端口转发…可以改用远程自带登录"，不自动换成另一种登录方式（G3/G5）。
- 令牌不进 argv（G1）：远程命令先 `IFS= read -r` 读 stdin 第一行，宿主在 ssh 启动后立刻把令牌作为第一行送出，之后才是协议消息。令牌只写进该 Agent 需要的环境变量，`AERORIC_T` 用完即 unset。令牌的作废时机与本机一致：Claude/omp 随进程，Codex 随 app-server。
- 远程托管目录在 `~/.aeroric/agent-homes/<预设目录名>`，与本机布局一致（G2）。环境变量由与本机共用的纯函数生成：`managed_home_ops`、`gateway_ops`。omp 的 models.yml 等文件先在本机临时目录生成，再按"models.yml 总是覆盖、其余缺失才写"发到远程。Claude 的 transcript 路径跟着预设目录走（`remote_claude_config_dir`）。远程命令统一把 127.0.0.1/localhost 加进 NO_PROXY，避免远程代理劫持回环请求。
- 远程 Codex 按"主机 + 预设"各起一个 app-server，令牌 id 为 `codex-app-server:<连接>/<预设>`（G6）。
- 选择器（G4）：远程项目里分为「经本机网关 · 我的连接 / Agent 预设」和「远程自带登录」两组，预设排在前面；会话页左上角显示"经本机网关 · <预设>"胶囊，依据是任务记录里的 `agentConfigId`，而不是"该 Agent 的第一个配置"。

## Alternatives considered

- **令牌写进远程命令行，或用 SendEnv**：前者远程其他用户能用 `ps` 看到；后者要远程 sshd 配置 AcceptEnv，多数机器默认不允许。否掉。
- **`-R 0` 让 sshd 分配端口**：远程命令拿不到分配到的端口号，没法写进 base URL。否掉。
- **共用远程用户自己的 `~/.claude` / `~/.codex`**：预设会话与用户自己的历史、配置会混在一起，与本机的隔离规则不一致。否掉。
- **每台主机固定一个端口**：多一个设置项，还要处理端口冲突。否掉。

## Consequences

- **收益**：远程项目也能用全部预设，用量照常计入本机网关；令牌不落盘、不进 argv。
- **代价与已知上限**：
  - 依赖远程 sshd 允许 TCP 转发；
  - 版本探测与正式会话之间端口可能被抢占（概率很小），这时会话直接失败，重开即可；
  - 远程转发端口上的其他用户能访问到网关入口，安全靠任务令牌；
  - 远程预设目录不会自动清理。
