# 宿主 aeroric-host 与 aeroric-cli

## 进程形态

`aeroric-cli serve` 是宿主的唯一进程入口，没有第二个守护进程。启动顺序：取数据目录租约 → 数据格式兼容检查 → 运行时 → 一次性迁移（旧版 Agent 设置迁移失败即启动失败；omp 明文 key 替换与旧版 Claude/Codex hook 清理尽力而为，失败只记日志）→ UDS 监听 → ready（`core/crates/aeroric-host/src/startup.rs`）。macOS App 经 `HostSupervisor` 拉起并监管宿主，宿主随 App 打包在 `Contents/MacOS/aeroric-cli`。`aeroric-cli` 的其它子命令都是宿主的 RPC 客户端；`askpass` 模式为 SSH 密码连接兑换一次性票据。

## 结构

| 文件 | 职责 |
|---|---|
| `uds.rs`、`codec.rs`、`auth.rs` | UDS 监听、JSONL 帧编解码、握手与版本判定 |
| `dispatcher.rs`、`services.rs`、`*_services.rs` | 方法分发；把 core 服务按服务面暴露为 RPC 方法 |
| `subscriptions.rs` | 事件 topic 订阅 |
| `executor.rs`、`live.rs`、`session_live_services.rs` | 任务执行与结构化会话的宿主侧接线 |
| `remote_sessions.rs`、`local_targets.rs` | SSH 远程会话（先探测版本再起会话）；本地 target |
| `remote_headless.rs`、`push_host.rs` | 手机远控的宿主面；向 APNs 发送端推送提示 |
| `platform_adapter.rs`、`secret_vault.rs`、`keep_awake.rs` | 卷探测与权限回执、钥匙串、防睡眠 |
| `gateway.rs`、`diagnostics.rs` | 嵌入/外部宿主网关；诊断报告 |

## 规则

- 宿主只做适配：业务判断留在 core。端口实现不得回调 core，持叶子锁期间不取别的锁。
- 锁竞争要报告给用户，绝不自动 kill 持有者或删锁继续。
- 新增方法要同时出现在 schema（`export-schema`）、宿主注册与 CLI rpc 三侧，`pnpm native:check protocol` 同红同绿。
