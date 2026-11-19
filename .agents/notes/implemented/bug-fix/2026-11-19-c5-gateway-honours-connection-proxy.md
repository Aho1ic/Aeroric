# Agent Note: C5 第一步：连接测试、模型目录和网关走同一个出口

Status: implemented

## Problem

[调研落地批次 C](../../proposed/feature/2026-11-15-adoption-c-memory-gateway-skills.md) 的 C5 担心的是：连接测试通过了，真实会话却因为走了另一条路而失败。动手前逐一核对了三条路径各自怎么构造出站请求：

| 路径 | 入口 | 怎么建客户端 | 代理 |
|---|---|---|---|
| 连接测试 | `settings::model_detect::probe_connection`（宿主 `agentConnections.testConnection`） | `build_detect_client` → `apply_detect_proxy` | 连接勾了"使用全局代理"时套用全局代理；勾了但没配代理就拒发 |
| 模型目录 | `agent_connections::fetch` → `model_detect::fetch_connection_catalog` | 同上 | 同上 |
| Aeroric 自己的调用（记忆整理的分析模型） | `model_detect::chat_once` | 同上 | 同上 |
| 真实推理 | 网关 `local_router`：`config_routes::resolve` 选出上游，`server::attempt_target` 发请求 | `RouterRuntimeConfig` 里唯一的客户端（`build_http_client`） | **一律直连**：宿主启动网关时从不设出站代理，按配置解析上游时也不看连接的"使用全局代理" |

所以，勾了"使用全局代理"的连接（例如只能经代理访问的海外服务）：

- 测试和拉模型目录都走代理，结果通过；
- 真实会话经网关直连，连不上。

这正是 C5 要防的情况。认证头没有分歧：Messages 用 `x-api-key`，其余用 Bearer，两边一致。

## Decision

**一个判断，四处共用**（`settings/model_detect.rs`）：

- `connection_proxy(use_global_proxy, &global)`：连接勾了才用全局代理；勾了但没配代理时返回 `NO_GLOBAL_PROXY`，什么都不发，从不悄悄改成直连（Q139）。
- 连接测试、模型目录和 `chat_once` 原来各自写了一段相同的判断，现在都改为调用它。
- 测试和目录各拆出一个直接接受代理设置的版本（`probe_connection_via`、`fetch_connection_catalog_via`），原入口读全局设置后调用它们。

**同一套代理规则**：

- `gateway_proxy(&settings)` 把同一份代理设置转成网关用的 `RouterOutboundProxy`，规则和测试路径的 `apply_detect_proxy` 相同：
  - URL 先规范化；
  - 设了用户名才带基本认证，用户名和密码都去掉首尾空白；
  - `no_proxy` 在用户的规则之外，补上本机和私网地址（`detect_no_proxy_rules`）。

**网关按连接的选择走代理**：

- `config_routes::resolve_with_proxy` 为每个上游带上它的代理（`UpstreamTarget::with_proxy`）；`resolve_from_disk` 每次解析时读一次全局代理设置，设置改了，下一个请求就生效。
- 勾了代理但没配代理的连接被排除，原因写进"不可用"的说明里。
- `attempt_target` 按上游挑客户端：`RouterRuntimeConfig::client_for` 为每种代理配置只建一次客户端并缓存，没有代理的仍用原来的直连客户端。代理建不起来时，这次尝试按失败处理，不改走直连。

## Alternatives considered

**宿主启动网关时把全局代理设成整个网关的出站代理（`with_outbound_proxy`）。** 最强的理由是一行就能接上。没选：

- 这样所有连接都会走代理，没勾"使用全局代理"的也一样，和测试路径的语义相反；
- 网关只启动一次，之后改了代理设置不会生效。

**在网关里每次请求现建带代理的客户端。** 最强的理由是不用缓存。没选：每次新建都会丢掉连接池，经代理的长对话每轮都要重新握手。

## Consequences

- **收益**：勾了"使用全局代理"的连接，真实会话和测试走同一个代理；没配代理时三处都明确拒绝。
- **代价**：网关每次按配置解析上游时多读一次全局设置。原本每个请求就要读一次连接文件，这次读同属一类开销。
- **没做（第二步）**：
  - 测试结果记下出口摘要和配置修订号，配置改过后标"配置已变，需要重测"；
  - 界面上写明测试会花多少。
- **验证**：
  - `local_router::server` 新增 `the_test_the_catalog_and_the_gateway_leave_by_the_same_exit`：
    - 起一个记录请求的假代理，上游用一个解析不了的域名，只有经过代理的请求才到得了；
    - 连接测试、拉模型目录、网关三条路径都经过这个代理，目标主机相同，带着同一个密钥；
    - 修复前网关这一路是直连，这个测试会失败。
  - 新增 `the_proxy_choice_is_one_decision`：
    - 勾了但没配代理时拒发；
    - 没勾时三处都直连；
    - 网关的代理已经规范化、去掉空白，并补上了本机和私网规则。
  - `config_routes` 新增一个测试：勾了代理的连接带上代理，其他照旧直连；没配代理时这个连接被排除并写明原因。
  - 网关和按配置路由的测试共 219 个通过。没有界面改动。
