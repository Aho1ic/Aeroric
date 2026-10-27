# remote-push/ 规则

产品方维护的 APNs 发送端，独立 crate（自有 `Cargo.toml`/`Cargo.lock`），不依赖 core。→ [推送平面](../.agents/notes/implemented/architecture/2026-10-13-push-plane.md)

- `.p8` 私钥只从部署方环境变量/文件读入，绝不进入客户端、relay 或仓库。
- `PushEnvelope` 是隐私白名单（`kind`/`eventSeq`/`collapseKey`）；不得新增携带任务标题、项目名、路径、命令、提示词或凭据的字段。
- 不做重试队列；APNs 429/503 的退避如实回给宿主。
- 验证：`cargo test --manifest-path remote-push/Cargo.toml`。
