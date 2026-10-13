//! `aeroric-remote-push` —— **产品方控制的 APNs 发送端**(MIGRATION_PLAN §D AP6)。
//!
//! 存在理由与边界:
//!
//! * **私钥只在这里**。APNs 认证密钥(`.p8` AuthKey)由自托管部署方的环境变量/
//!   文件注入,永远不进入手机、Mac 客户端或 relay(Q10、ADR-040 §1)。本 crate
//!   没有读取客户端目录的任何代码路径,并且测试断言「客户端源码里不存在 p8 面」。
//! * **不承诺必达**。APNs 是 best-effort(Apple 文档原文),这里把 429/503 的
//!   退避要求如实回给宿主,由宿主决定节奏,不假装可靠队列。
//! * **载荷只含路由信息**。正文由本端按 `kind` 拼 `aps.alert.loc-key`,文案留在
//!   手机 bundle;`PushEnvelope` 里没有任务标题/项目名/路径/命令的字段。
//! * **撤销立即生效**。`/v1/bind` 收空 token 即解绑;设备撤销后宿主不再推,
//!   存量在途通知在手机上也不得产生审批(手机侧按 Q7 重核对)。
//!
//! 传输:APNs 要求 HTTP/2 + TLS1.2+,所以这里用 hyper 的 h2 客户端直接控制
//! `:method`/`:path`/`authorization` 等头(Apple 对 HPACK 编码与头集合有要求,
//! 通用的 HTTP 客户端封装会把 `:path` 固定成 URL 路径形态,拿不到
//! `/3/device/<token>`)。`http://` 基址走 h2c,只用于本地联调与测试的假 APNs。

pub mod apns;
pub mod auth;
pub mod bindings;
pub mod config;
pub mod server;

pub use config::PushConfig;
pub use server::AppState;
