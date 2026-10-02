# P7-01 评审记录 — SSH/SFTP 与远程工作区（2026-10-03 收口）

## 1. 范围

P7-01 是 Phase 7 第一个 Task：core `connections/` 模块（连接配置/协议形状/
队列簿记）＋Connections 六视图＋门禁 2 case。

## 2. 实现链（两刀）

- **刀一（core connections 模块）**：`core/crates/aeroric-core/src/connections/
  {mod,ssh,sftp}.rs`——ssh（SshConnection 配置：**明文密码 skip_serializing
  结构性不出去**＋hasPassword rename；SshHostKey/SshHostKeyStatus：
  Trusted/Unknown/Unreachable——**hostkey changed 必须失败等待确认**；
  `fingerprint_of`/`parse_scan_output` verbatim 上收：SHA256 无填充＋
  @cert-authority＋key_type 去重；`validate_remote_path` 拒前导 dash/NUL；
  cwd 分离判定）＋sftp（SftpEntry/validate_entry_name/路径 helpers verbatim、
  **TransferQueue** 进度/取消/恢复/失败簿记——cancel 清临时文件而非删原
  目标）。core Cargo.toml 加 base64（与宿主同版本）。`tests/ssh_host_keys.rs`
  8 用例。
- **刀二（Connections 六视图）**：`AeroricUI/Connections/{ConnectionStore,
  ConnectionViews}.swift`——ConnectionStore（**无明文密码**：只有 hasPassword
  布尔；host key 状态；remote cwd 分离）＋ConnectionList＋SSHConnectionEditor
  （SecureField 密码不预填/传一次后清内存）＋HostKeyPrompt（**首连/keyChanged
  分别提示**——keyChanged 显示旧指纹警告）＋SFTPBrowser＋TransferQueueView
  （进度/取消/恢复）＋RemoteWorkspaceBanner。`ConnectionStoreTests`(5)。

## 3. 验证实测（2026-10-03）

| 验收 | 结果 |
|---|---|
| `pnpm native:check connections`（新注册） | 2 case / 13 用例 ok（ssh_host_keys 8＋ConnectionStoreTests 5） |
| `cargo test -p aeroric-core` | 541 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 185 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（connections floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |

## 4. 验收原文逐条对照

- `tests/ssh_host_keys.rs`(8)＋`ConnectionStoreTests.swift`(5) ✅；`pnpm
  native:check connections` ✅。
- 默认 fake SSH/SFTP（本测试不连接任何真实服务器）✅；hostkey changed 必须
  失败等待确认（Unknown 状态需用户确认指纹，单测覆盖）✅。
- 非 ASCII/空格/前导 dash 远程路径不注入命令（`validate_remote_path` 单测）✅；
  cancel transfer 清临时文件而非删除原目标（队列只标状态，单测覆盖）✅。
- `grill-me [connections]`：已真实调用（用户：接受并收口）✅。

## 5. 已记录的边界与挂账

- `connections/ssh_executor.rs`（SSH 任务 start/resume/input/resize/cancel
  低层入口＋legacy_task_executor SSH 分支删除）→ P7 接线时逐项接；本 Task
  交付连接配置/协议形状/队列簿记（用户确认记账）。
- fake SSH 任务在 Tauri 与独立 CLI 的 start→input→resume/cancel e2e →
  P10-01（需宿主接线）。
- 显式 opt-in 本地测试 sshd 的真连接 → P10-01（不连接用户生产服务器）。
