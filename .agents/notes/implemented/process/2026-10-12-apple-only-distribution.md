# Agent Note: 只做 macOS 与 iOS；iOS 上架 App Store，macOS 走 Developer ID 公证

Status: implemented

## Problem

Aeroric 曾是 Tauri 跨平台桌面端 + React Native 移动端。产品转为 Apple 原生之后，需要回答发到哪里、哪些平台退场、Mac 是否能进 Mac App Store。Mac 的底线是"一台 Mac 不依赖别的机器完成本地 Agent 编码闭环"，而这与 App Store 沙盒直接冲突。

## Decision

- 客户端只有 SwiftUI macOS App 与 iOS/iPadOS 伴随端。Windows/Linux 客户端、WSL、Android 退场；SSH 对端仍支持 Linux/Unix、macOS、Windows；Linux relay 部署与安全审计保留。
- iOS 伴随端以上架 App Store 为目标，只做远程查看与控制，依赖在线的 Mac；Mac 不可用时手机不提交执行或审批，relay 不代替宿主。
- macOS 用 Developer ID 签名、Hardened Runtime 并公证，在官网分发。宿主保持独立进程，以便将来拆出 MAS 版。
- 开发/测试签名、Developer ID、商店签名彼此分离，发布永不回退到 ad-hoc 签名。
- relay 由用户自备；产品方只维护 APNs 发送端（见 [推送平面](../../implemented/architecture/2026-10-13-push-plane.md)）。
- 后台宿主需显式 opt-in 才在 UI 退出后继续运行，有状态与停止入口；不默认登录自启。

## Alternatives considered

- **macOS 也上 Mac App Store**：分发与更新最省心，曾是两端硬目标。否掉是因为沙盒内实测：启动用户安装的四族 CLI 及 node、brew、git、python，读取 `~/.claude`、`~/.codex`、`~/.omp`、`~/.aeroric`，在内核层直接 EPERM，加 temporary-exception 也一样——进入 MAS 就违背本地 Agent 闭环底线。
- **保留 Tauri 跨平台客户端**：覆盖 Windows/Linux 用户。否掉是因为用户决定产品转为 Apple 原生，双栈维护成本不可接受。

## Consequences

- **收益**：Mac 端保留完整本地能力；平台面收窄，协议里的 WSL 能力在 macOS 上恒报不支持。
- **代价与已知上限**：官网版首发前仍需补齐 Hardened Runtime、主 App entitlements、Developer ID 签名与公证脚本，并去掉 UDS 回退到 `/tmp` 的路径；当前本机只有 Apple Development 证书。若将来确需 MAS 版，应拆出沙盒内可用的子集，而不是放宽本地闭环底线。
