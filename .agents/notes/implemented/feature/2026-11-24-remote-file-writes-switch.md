# Agent Note: 设置里开关“允许手机改文件”

Status: implemented

## Problem

2026-11-23 给 `project.writeFile` 加了门禁：`remote-config.json` 的 `allow_file_writes`，默认关。可是没有界面，要打开只能手改配置文件。

设置界面第一版（笔记 `2026-11-24-settings-redesign-first-cut`）把它列为没做：要改 `remote.updateConfig` 和 `remote.status`。

## Decision

**宿主：**
- `remote.status` 多回一个字段 `allowFileWrites`。
- `remote.updateConfig` 认 `allowFileWrites`（布尔）。
- 参数收成 `control::ConfigChange` 结构，没给的字段保持原样。
- 值真的变了才记审计 `remote-file-writes {allowed}`。
- 和别的配置一样落盘；远程服务在运行时会重启，连着的手机重连后，`hello` 里的能力表跟着变（`files.write` 有或没有）。

**Mac：**
- `RemoteStatus.allowFileWrites`；
- `RemoteClient.setFileWrites`，只发这一个字段；
- `RemoteStore.setFileWrites`；
- “服务”一组加一行“允许手机改文件”：
  - 开关旁写明默认关、开关会记进审计日志、已连着的手机会断开重连；
  - 没读到状态时开关不可点；
  - 打开前弹确认（方案定的“打开要确认”，随存储页一起补上，见笔记 `2026-11-24-settings-storage-page`），关掉不用确认；
- 设置搜索登记这一行，搜“写文件”能找到。

## Alternatives considered

**改这个开关时不重启远程服务。** 没选：门禁每次请求都读当前配置，不重启也能生效，但连着的手机拿的还是旧能力表。手机上看到的和 Mac 允许的会对不上，直到它自己重连。重启一次的代价只是手机重连几秒。

**开关放在每台设备上（按设备授权）。** 没选：设备授权要改配对和设备表的格式。现在只有一台手机的用法，一个总开关够了；以后真有多台设备再细分。

## Consequences

- **收益：** 不用手改配置文件；开关有审计记录；Mac 和手机对“能不能写”的认识一致。
- **局限：**
  - 远程服务开着时切换开关，手机会掉线几秒；
  - 手机 App 里还没有写文件的入口，打开开关目前只对直接调 `project.writeFile` 的客户端有用；
  - 界面没有目检。
- **验证：**
  - Rust：`the_mac_turns_remote_file_writes_on_and_off`（状态、落盘、审计只在变化时记、改别的字段不动它）；
  - Swift：`testStatusSaysWhetherPhonesMayWriteFiles`，搜索测试里加了“写文件”。
