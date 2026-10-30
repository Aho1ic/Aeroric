# Agent Note: 本机登录接入网关

Status: implemented

## Problem

2026-10-29 上线「任意模型用于任意 Harness」之后，会话在本机登录（ChatGPT / Claude 订阅）和第三方模型之间切换，仍然要搬会话：
- 新来源的第一轮要读完整上下文；
- 每种 Harness 都要有单独的搬家代码；
- 窗口变小时还要另存交接文件。

owner 要求把本机登录也作为网关的一个来源，让同一个会话直接切换（Trellis 任务 `10-29-local-login-gateway`，决定 D1–D10）。

还有一个限制：走网关的 Codex 托管 home 里不能有 ChatGPT 登录（R1 不变量）。Codex 一旦有登录，就会自己检查额度，额度用完后在请求到达网关之前就把模型锁住。所以不能让 CLI 带着自己的登录走网关。

## Decision

**登录作为虚拟连接**
- `ConnectionsDocument::with_logins()` 在 `login_gateway.<harness>` 处于「开启且已确认风险」时，注入 `login-codex` / `login-claude`（`ConnectionKind::LinkedLocalLogin`）。这两个连接不写入文件：路由、启动、窗口估算读 `store::load_effective`，编辑和保存仍读写原文件。
- D1：登录连接只出现在本 Harness 的派生配置 `via-<harness>-login`（「本机登录」）里，「全部第三方模型」和别的 Harness 都看不到它。
- 在 `via-<harness>-login` 里，第三方连接都不提供的模型名，一律交给登录。原因是 Claude Code 会把 `opus` 解析成完整 id 再发出，登录的模型列表里没有这个 id。

**令牌**（`agent_connections::login_credentials`）
- 读取：Codex 读 `$CODEX_HOME/auth.json`，Claude 用 `/usr/bin/security` 读钥匙串条目 `Claude Code-credentials`。
- 续期：只在快过期（少于 5 分钟）或上游回 401 时续期。用 CLI 自己的锁：Codex 是 `auth.lock` 的 `flock`；Claude 是 proper-lockfile 的两个目录锁，`~/.claude/.oauth_refresh.lock` 和 `<~/.claude>.lock`。拿到锁后先重读，CLI 已经续过就直接用新令牌。
- 写回：Codex 用临时文件加原子改名（0600，保留其他字段，更新 `last_refresh`）；Claude 用 `security add-generic-password -U -X`，只改 `claudeAiOauth` 里的三项。
- 判定：续期回来的还是原令牌，不算成功；`invalid_grant` 这类错误判为登录失效。
- 令牌不进日志、用量库和 App，`Debug` 输出里也是隐藏的。
- 锁与格式来源：openai/codex `codex-rs/login/src/auth/storage.rs`，以及本机安装的 Claude Code 程序（`research/verify.md`）。

**网关转发**
- `UpstreamTarget::with_login(harness)`；`config_routes::resolve` 遇到登录连接时不读 API key。
- 每次请求都换上登录的 Bearer，并去掉 `x-api-key`。Codex 补 `chatgpt-account-id`，缺 `originator` 时补上，请求体强制 `store: false`；Claude 合并 `anthropic-beta: oauth-2025-04-20`。
- 401 时先续期，只重试一次；再失败就按现有鉴权规则冷却。

**启动**
- 新建的内置 `codex` / `claude` 会话，如果没有选配置、不是 SSH 项目、登录又开着，就自动改用 `via-<harness>-login`（`launch::login_config_for`）。
- 和第三方配置一样，进程运行在托管 home 里，Codex 的托管 home 照旧没有登录。

**会话里切换**（`session.planSwitch`）
- 已在 `via-<h>-login` 上：原地换（App 走 `configure(model:)`）。
- 直连会话或本 Harness 的第三方会话：用现有 `session.moveSource` 迁一次到登录配置，不弹框。
- 当前上下文超过新窗口的 85%：改用交接文件，并弹框说明。
- 登录没开：保持 2026-10-29 的行为，跨登录方式时确认。

**界面**（设计稿已于 2026-10-29 通过）
- 本机登录详情页新增「经网关使用」卡片：开关、令牌有效时间、会话存放位置。
- 第一次开启时弹出条款与封号风险说明，确认后由宿主写入 `accepted_at`。
- 模型菜单的「本机登录」分组不再提示搬会话。
- 遇到额度或登录类错误时，给出「换到第三方模型」按钮。

## Alternatives considered

- **透传 CLI 自己的登录**（Codex `requires_openai_auth`）：不选。额度用完后 Codex 会在本机锁住模型，请求根本到不了网关，「额度用完换第三方」正好做不到。
- **Aeroric 自己做 OAuth 登录**：不选（D2）。owner 要多登录一次；而且同一个账号有两个刷新方时，刷新令牌会互相顶掉。
- **跨 Harness 用登录**（例如 Claude Code 用 ChatGPT 订阅）：不选（D1）。要伪装请求，封号风险高。
- **额度用完自动降级**：不做（D3、R8），只给按钮。
- **另建 Aeroric 自己的刷新锁**：不选。CLI 不认这把锁，等于没有锁。

## Consequences

- 文档新增 `login_gateway` 字段：关闭时不写出，旧文件照常读取。
- 新增宿主方法：`agentConnections.setLoginGateway`、`agentConnections.loginGatewayStatus`、`session.planSwitch`。
- 开启后，经网关的 Codex 和 Claude Code 会话放在托管 home，不出现在 ChatGPT 应用的 Codex 历史里，也不出现在 Claude Code 自己的历史里。
- 网关会读取并续期本机登录令牌，写回 CLI 原来的位置。owner 已知晓条款风险（Q24 / D4）。
- 关闭后新会话回到直连。已在网关上的会话下次切换时，按「登录没开」的规则处理。
- 测试：令牌读取、续期、锁、写回、并发、失效；虚拟连接只服务自己的 Harness；兜底路由；新会话选用登录配置；网关替换令牌、401 续期一次、用量里没有令牌；切换路线；App 文案与分组标题。
