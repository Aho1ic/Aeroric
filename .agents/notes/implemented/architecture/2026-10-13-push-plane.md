# Agent Note: 推送平面——产品方只维护 APNs 发送端，通知不是授权

Status: implemented

## Problem

iOS 伴随端需要在任务完成、需要审批时提醒用户。APNs 要求一把 `.p8` 私钥签名；它放在哪、推送载荷能带什么、手机收到推送后能做什么，决定了私钥泄露面、隐私边界，以及一条推送能否被伪造成一次审批。

## Decision

- 产品方只维护 APNs 发送端 `remote-push/`（独立 crate、独立 manifest/lock、可自托管），不维护 relay，也不向用户或 relay 分发私钥。`.p8` 只从发送端部署方的环境变量/文件读入。
- 不引入账号体系：宿主在部署期经 `aeroric-remote-push enroll` 铸造 per-user 凭据存进 Mac 钥匙串，此后每个请求用 HMAC-SHA256 签名方法/路径/时间戳/nonce/规范体。
- 载荷是白名单：`PushEnvelope` 只有 `kind`（三种之一）/`eventSeq`/`collapseKey`，可见文案用客户端 bundle 的 `loc-key`。任务标题、项目名、路径、命令、提示词、凭据没有入口。
- 通知不是授权：手机侧唯一入口 `CompanionStore.ingestPushHint` 只置 `resyncNeeded = true`；审批只能来自端到端加密的快照。
- 不承诺必达、不做重试队列：发送端把 APNs 429/503 的退避要求如实回给宿主，宿主据此抑制后续推送。
- 撤销立即生效：`remote.revokeDevice` 或手机注销后发送端删绑定，在途通知也不产生审批。
- core 里不做 HTTP：`PushTransport` 是同步端口，等待由宿主移出。

## Alternatives considered

- **放在 relay 里发推送**：用户自托管一处即可。否掉是因为 relay 会从可选的盲转发变成必须信任的证书持有者。
- **每台 Mac 客户端直接发 APNs**：没有服务端。否掉是因为每台客户端都要持有私钥，泄露面从一台服务器扩大到所有客户端。

## Consequences

- **收益**：私钥只在一处；推送泄露不了业务内容，也伪造不了审批。
- **代价与已知上限**：需要产品方运维一个发送端；真实 AuthKey、生产 `aps-environment` entitlement 与公网部署尚未落地（`ios/AeroricCompanionApp/AeroricCompanion.entitlements` 写的是 `development`）。
