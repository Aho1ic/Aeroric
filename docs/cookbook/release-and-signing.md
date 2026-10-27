# 签名与分发

分发决策见 [Apple-only 分发](../../.agents/notes/implemented/process/2026-10-12-apple-only-distribution.md)。

| 端 | 渠道 | 签名 |
|---|---|---|
| macOS | 官网下载 | Developer ID + Hardened Runtime + 公证 |
| iOS/iPadOS | App Store | 商店签名 |
| 开发/审阅构建 | 本机 | `scripts/native/sign-local.sh` 本地签名，`scripts/native/verify-bundle.sh` 校验 bundle |

- 开发/测试签名、Developer ID、商店签名彼此分离；发布永不回退到 ad-hoc。
- macOS App 包内含宿主：`Contents/MacOS/aeroric-cli`（`cargo build --locked -p aeroric-cli` 产物）。
- DMG：`scripts/create-macos-dmg.sh`。
- 首发前仍缺：Hardened Runtime 与主 App entitlements、Developer ID 签名与公证脚本、去掉 UDS 回退到 `/tmp` 的路径；iOS 生产 `aps-environment` 与真实 APNs AuthKey。
- commit、push、tag、商店提交与发布都需要用户明确授权。
