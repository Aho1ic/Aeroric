# remote-relay/ 与 remote-protocol/ 规则

`remote-relay` 是用户自托管的盲转发 relay；`remote-protocol` 是它的撮合控制面协议。两者各自独立 crate，不依赖 core。

- relay 只转发 E2EE 字节，不解密、不持有任何业务或 APNs 密钥。
- relay 协议与本地协议是两个独立版本空间，桥接必须显式声明映射的版本对。→ [版本边界](../.agents/notes/implemented/architecture/2026-09-22-protocol-version-boundary.md)
- 部署与安全要求见 [docs/remote-public-access.md](../docs/remote-public-access.md)（`src/main.rs` 引用它）。
- 验证：`cargo test --manifest-path remote-relay/Cargo.toml`、`cargo test --manifest-path remote-protocol/Cargo.toml`。
