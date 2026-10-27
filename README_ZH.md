<p align="center">
  <img src="docs/images/logo.png" alt="Aeroric Logo" width="150" />
</p>

<h1 align="center">Aeroric</h1>

<p align="center">
面向 AI 编码 Agent 的原生 macOS 工作台，配套 iOS 远程控制端。
</p>

<p align="center">
  <a href="./README.md">English</a>
</p>

---

> **状态**：开发中。旧的 Tauri/React 客户端已删除，仓库只保留原生 SwiftUI 客户端和 Rust core。
>
> **测试政策（2026-10-19）**：真实窗口 UI 测试暂停，测试虚拟机已退役。
> 见 [docs/native/ui-testing-policy.md](docs/native/ui-testing-policy.md)。

## 简介

Aeroric 在 Mac 上用真实 PTY 运行 Claude Code、Codex、oh-my-pi（omp）、DeepSeek Harness 和自定义
Agent，原样显示各 Agent 自己的 TUI。在此之上提供任务管理、Git 与文件视图、托管 Agent home、账号与模型配置，
以及 iOS 伴随端：查看任务、处理审批、接收推送。所有执行都发生在 Mac 上。

## 目录结构

```
core/            Rust workspace：aeroric-protocol、aeroric-core、aeroric-host、aeroric-cli
  vendor/        打过补丁的第三方 crate（portable-pty、ssfmt、notify-types）
macos/           SwiftUI macOS App（AeroricApp）与 AeroricKit 包
ios/             SwiftUI iOS 伴随端与 AeroricCompanionKit 包
contracts/       Rust 与 Swift 共用的协议契约和金标 fixture
remote-protocol/ 中继控制面协议
remote-push/     APNs 发送端
remote-relay/    可自建的中继
scripts/native/  构建、签名、门禁运行器和 UI 测试工具（Node，无 npm 依赖）
docs/native/     ADR、迁移计划工件、功能候选库与现行设计
```

`core` 依赖仓库外的 [dbx](https://github.com/t8y2/dbx)（位于 `../dbx`），版本由
`scripts/dbx-ref.txt` 固定，补丁由 `scripts/prepare-dbx.sh` 应用。

## 开发

环境要求：macOS 26+、Xcode、Rust（版本见 `core/Cargo.toml` 的 `rust-version`）、Node 24。

```bash
pnpm native:doctor                 # 检查工具链
pnpm native:build                  # 构建并本地签名 AeroricNative.app
pnpm native:check <module>         # 运行 scripts/native/checks.json 里的门禁模块

cargo test --manifest-path core/Cargo.toml
swift test --package-path macos/AeroricKit
swift test --package-path ios/AeroricCompanionKit
```

## 分发

- macOS：Developer ID 签名加公证，在 Mac App Store 之外分发。
- iOS：上架 App Store，作为远程控制用户自己已配对 Mac 的伴随端。

见 `docs/native/adr/039-distribution-pivot-macos-ios.md`。

## 许可

专有许可，见 [LICENSE](./LICENSE)。vendor 中的第三方 crate 保留各自的许可。
