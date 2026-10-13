# ADR-040 — 推送平面:产品方发送端 + 最少载荷 + 「通知≠授权」

- 状态:**implemented(2026-10-13,AP6)**。私钥注入/真机投递仍待授权(GAP-STORE-06)。
- 日期:2026-10-13
- 关联:`MIGRATION_PLAN.md` §D AP6;ADR-039 §7 Q7/Q10;`docs/remote-public-access.md`;核心实现
  `remote-push/`(发送端)、`core/crates/aeroric-core/src/remote/push.rs`(策略)、
  `remote/push_plane.rs`(派发)、`core/crates/aeroric-host/src/push_host.rs`(网络出口)。

## 1. 决策

1. **产品方只维护 APNs 发送端,不维护 relay,也不分发私钥。** 发送端是独立 crate
   (`remote-push/`,与 `remote-relay/` 同形:独立 manifest/lock、可自托管、不含 Tauri)。
   APNs 认证密钥(`.p8`)只从发送端部署方的环境变量/文件读入,永远不进入手机、Mac
   客户端或 relay(Q10、GAP-STORE-06)。
2. **不引入账号体系。** 发送端的「用户」由配对派生:宿主在部署期铸造一份 per-user
   凭据(`aeroric-remote-push enroll`),存进 Mac 的钥匙串;此后每个请求用
   HMAC-SHA256 签名方法/路径/时间戳/nonce/规范体。这样复用既有设备信任关系,不新增
   注册、付费或云端账号。
3. **载荷是白名单,不是「注意别写进去」。** `PushEnvelope` 的字段集合就是隐私边界:
   `kind`(三种之一)/`eventSeq`/`collapseKey`,可见文案是客户端 bundle 的
   `loc-key`。任务标题、项目名、路径、命令、提示词、凭据**没有入口**;假 APNs 的
   原始字节断言里逐条检查它们不出现。
4. **通知不是授权。** 手机侧只有一个入口 `CompanionStore.ingestPushHint`,它的全部
   效果是置 `resyncNeeded = true`;审批只能来自 E2EE 快照。AP5 里
   「`eventSeq` → 伪造 `requestId` → 直接进 `pendingApprovals`」的路径已删除。
5. **不承诺必达,不做重试队列。** APNs 是 best-effort(Apple 原文);发送端把 429/
   503 的退避要求如实回给宿主,宿主据此**抑制**后续推送而不是排队重试。
6. **撤销立即生效。** `remote.revokeDevice` / 手机注销 → 空 token 绑定 → 发送端删
   绑定 → 后续事件不再产生推送;在途通知在手机上也不产生审批。

## 2. 为什么不放在 Tauri/relay 里

- **不放 relay**:relay 是用户自托管的盲转发,把它变成「有 APNs 私钥的服务」会让
  relay 从「可选的基础设施」变成「必须信任的证书持有者」,与 Q10 冲突。
- **不在 Mac 客户端里发 APNs**:那要求每台 Mac 都持有 `.p8` 私钥,泄露面从 1 台
  服务器扩大到每台客户端,而且与「不向用户分发私钥」直接矛盾。
- **不在 core 里做 HTTP**:`PushTransport` 是同步端口,真正的等待由宿主用
  `block_in_place` 移出;core 里没有 reqwest/hyper 依赖,这条边界由源码扫描钉住
  (`inv02-core-source-scan` 覆盖范围外的 crate 也不许出现,见 `remote-push` 的
  dev-only 耦合)。

## 3. 传输与协议事实(来自 Apple 文档,实现逐条对应)

| 事实 | 实现位置 |
|---|---|
| 必须 HTTP/2 + TLS1.2+;开发/生产是两台主机 | `remote-push/src/apns.rs`(`http2_only(true)`、`ApnsEnvironment::host`) |
| `:path` 必须是 `/3/device/<token>` | `ApnsClient::send` 构造 URI,假 APNs 断言该路径 |
| provider token 是 ES256 JWT(`iss`=Team ID、`iat`),不得频繁重签 | `ProviderKey::jwt`(原始 r||s 64 字节)、`TOKEN_REUSE`=50 分钟 |
| 载荷 ≤4KB 且不压缩 | 载荷只有 4 个键(见 §1.3),无需压缩 |
| `apns-push-type`/`apns-topic`/`apns-id`/`apns-expiration`/`apns-collapse-id`/`apns-priority` | `ApnsClient::send` 逐个头设置,集成测试逐项断言 |
| 410 Unregistered / 400 BadDeviceToken = token 失效 | `classify` → `ApnsOutcome::TokenInvalid` → 发送端解绑 + 回执 `invalidatedDeviceIds` |
| 429/503 = 退避 | `classify` → `Throttled` → 回执 `retryAfterMs` → 宿主 `Suppressor` |
| 其余 4xx(BadTopic/PayloadTooLarge…)重试无意义 | `classify` → `Rejected`(不重试) |

## 4. 两个实装陷阱(都被测试当场抓住)

1. **rustls provider 必须显式装。** 依赖图里 `hyper-rustls` 带 `ring`、
   `reqwest`/`rustls` 默认带 `aws-lc-rs`;两者同时可用时 rustls **不在首次 TLS 时
   自己选,而是 panic**。表现为「服务起来了,第一发就崩」。修复:
   `apns::install_tls_provider` / `push_host::install_tls_provider` 在构造连接器前
   装一次(`Once`)。
2. **`Runtime` / `reqwest::blocking::Client` 不能在 async 上下文里被构造或丢弃。**
   两者都会在 `Drop` 时关停内部 runtime,而「在 async 上下文里 drop runtime」是
   panic(`Cannot drop a runtime in a context where blocking is not allowed`)——宿主
   停止服务、重载配置都会触发。修复:进程级 `OnceLock` + `run_blocking`(单线程
   runtime 下明确报错而不是把 runtime 卡死)。`HttpPushTransport` 因此**不持有**
   runtime 与客户端,`new()` 无副作用。

## 5. relay 诊断的事实来源

`remote.relayDiagnostics` 描述的是**服务自己正在用的那份配置**(`CliRemoteHandle.relay`,
由 `serve` 在启动时把 `RemoteState.config` 传进来),不是再去读一遍
`remote-config.json`。理由两条:

1. 重读文件会与运行中的 listener 不一致 —— 用户改了文件但服务没重启时,诊断会
   报告一个服务并不在用的地址,而它存在的意义正是回答「现在到底能不能用」。
2. 「诊断读全局文件」让结果依赖进程级共享状态,测试里表现为并发用例互相覆盖
   (AP6 的集成套件正是被这一条打成 4/5 红)。

方案校验(空 URL = 没用 relay;非 loopback 必须 `wss://`)仍然只有一份实现
(`core::remote::push::relay_scheme_problem`),诊断与生产启动都用它。

## 6. 未完成 / 具名阻塞

- **GAP-STORE-06**:真实 APNs AuthKey、`aps-environment` 生产 entitlement、真机前后台/
  锁屏投递证据。本项目里 `ios/AeroricCompanionApp/AeroricCompanion.entitlements` 写的是
  `development`,因为它是本地构建可用的值;生产值随真实 App ID 在 AP9 落地,**不在这里伪造**。
- **BLOCK-AP6-2**:发送端生产部署与公网证书(AP9 授权后)。

两条都属于「需要另外授权的外部资产」,不是代码缺口;AP6 的本地闭环(假 APNs 真 h2、
真发送端、真宿主)已全部通过。