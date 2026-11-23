# Agent Note: 远程写文件默认关闭（补上宿主门禁）

Status: implemented

## Problem

backlog §4.4-8 定了“远程写文件默认关闭”。但做手机“文件与 Git”页时（见 [那一步的笔记](../feature/2026-11-23-d4-phone-files-and-git.md)）查出，宿主上没有这道门。

动手前（`main@35c1f917`）核对的事实：
- **宿主照写：** 已配对的设备发 `project.writeFile {projectId, path, content}`，宿主就写：
  - 路径限在项目里，只能写已存在的普通文件，最多 200 KB；
  - 写完记一条审计；
  - 没有任何开关。
- **谁会用它：**
  - 手机端不调用它；
  - 仓库里也没有别的客户端调用它；
  - 但任何拿到设备令牌的一端都能用。
- **两个宿主都受影响：** 分发在 core 的 `remote/rpc.rs`，无头宿主（`aeroric-cli serve`）和桌面宿主都经过这里。

## Decision

**远程配置加一项 `allow_file_writes`，默认关。**
- **存在哪：** 存在 `remote-config.json`。之前写下的配置没有这一项，读出来也是关。
- **拦在哪：** `rpc::dispatch` 收到 `project.writeFile` 时，这一项没打开就拒绝：
  - 回答以 `REMOTE_FILE_WRITES_OFF` 开头，写明“这台 Mac 没有打开远程写文件”；
  - 记一条审计 `remote-file-write-refused`，带项目和路径；
  - 请求不交给宿主的处理函数。
- **打开时：** 照原来的规则写。
- **怎么打开：** 现在没有界面，要在 `remote-config.json` 里写 `"allow_file_writes": true`，这正是“默认关闭”的意思。
- **不会被冲掉：** 桌面端和无头宿主保存远程设置时，都是从现有配置改几项再存，这一项会保留。

## Alternatives considered

**删掉 `project.writeFile`。** 没选：删方法是协议变更（`contracts/AGENTS.md`），而 backlog 写的是“默认关闭”，不是“不做”。以后要在手机上改文件，打开开关就行。

**按设备授权。** 这次没做：现在没有任何客户端要写文件，一个全局开关就够。真要用时，再按设备细分。

**在宿主的 `project_write_file` 里检查。** 没选：桌面宿主和无头宿主各有一份处理函数，拦在 core 的分发处，两边一次管住。

## Consequences

- **收益：** 已配对的设备（包括令牌泄露的一端）默认不能改项目里的文件；被拒的写入有审计可查。
- **局限：**
  - 打开它没有界面；
  - 同样会改 Mac 上东西的 `agentConfig.save`、`agentConfig.create`（写 Agent 配置）仍对已配对设备开放，这次没动。
- **验证：**
  - core 新增 1 个测试：
    - 默认拒绝、回答带前缀、记审计、请求没到处理函数；
    - 默认值和旧配置读出来都是关；
    - 打开后照写。
  - MacBook 上：
    - core 的 remote 测试 126 个、`remote_headless` 集成测试 10 个都过；
    - `aeroric-host`、`aeroric-cli` 编过；
    - `cargo fmt --check` 过。
