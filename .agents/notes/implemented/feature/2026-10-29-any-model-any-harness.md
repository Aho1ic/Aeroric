# Agent Note: 所有 Harness 都能用任意第三方模型

Status: implemented

## Problem

用户要求「所有 harness 都要能够使用任何第三方的模型，不能有限制」。此前只有 Claude、Codex 能按连接选第三方模型；OMP 只能靠手动保存的配置；DSH 不能走网关；会话里只能在当前配置的模型之间换，换到别的连接或本机登录做不到。

## Decision

- **「全部第三方模型」**：`ConnectionsDocument` 为每个能走网关的 Harness 派生 `via-<harness>-all`（`ConfigTarget::Route`，标题「全部第三方模型」），不落盘；`connection_configs()` 覆盖 Codex、Claude、OMP、DSH，先「全部」后各连接。会话目录（`agent.configurations`）为第三方配置附 `modelDetails`（所属连接、上下文窗口、已知限制如「不支持图片」），菜单按连接分组，附注与模型名同一行（`PickerOption.trailing`；用户：选项都是一行）。不预先过滤不兼容的模型（Q5）。
- **DSH 走网关**：`Harness::gateway_supported` 加入 DSH，协议选择同 OMP（`routing::config_protocol`）。宿主 PTY 启动带配置的 DSH 时，`dsh_home::prepare_gateway_home` 在托管 home（`agent-homes/<config>`）写只指向网关的 provider（`apiKeyEnv: DEEPSEEK_API_KEY`），并写这个任务的模型 patch（provider `aeroric`）以 `--patch` 传入；任务口令经 `DEEPSEEK_API_KEY` 传入、不落文件；`DSH_HOME` 指向托管 home。用户自己的 `~/.dsh` 不动。远程仍不支持（Q6）。
- **会话中换来源**（`session.moveSource`，用户 2026-10-29 两次修订）：不压缩、不截断，原模型不被调用（上游故障或额度用完时也能换）。
  - 放得下（当前上下文 ≤ 新模型窗口的 85%，窗口未知时视为放得下）且是 Claude / Codex：把 Agent 自己的会话记录复制到新来源的 home（Claude `projects/<cwd>/<id>.jsonl`，Codex `sessions/<年>/<月>/<日>/rollout-….jsonl`），任务改用新配置与模型，会话记为 `fork:<原 id>`，下一条消息在新来源分叉接着跑；原记录不动。
  - 放不下（如 1M 上下文换到 200K 模型）或其他 Harness：把完整记录（每条消息、工具调用与结果）写成交接文件（Aeroric 数据目录 `handoffs/`），任务记 `pendingHandoff`；下一条消息启动新会话，开头是交接说明与按新窗口 40% 截取的最近对话（最新优先），完整内容在文件里按需读取。
  - App：模型菜单列出当前来源、「全部第三方模型」（按连接分组）和本机登录；同来源直接换模型，第三方之间直接搬，跨登录方式先确认；搬完后在输入框上方一行说明结果。
  - 原模型留下的推理 / 思考内容在别家上游无效时，由网关 R7 去掉后重试。

## Alternatives considered

- **先 `/compact` 再带摘要**：不选。压缩要调用当前模型（消耗 token），上游不可用或额度用完时恰恰做不了。
- **只带最近 6 轮**：不选（用户：不严谨）。
- **同一进程内切换（本机登录也作为网关来源）**：后续做（用户要求记下），届时不再需要搬会话。

## Consequences

- 新的宿主方法 `session.moveSource`；任务记录可能带 `pendingHandoff`；数据目录新增 `handoffs/`。
- 复制进本机登录目录（`~/.claude/projects`、`~/.codex/sessions`）的是新文件，不改已有记录。
- 测试：派生来源、DSH 网关 home、搬会话的窗口适配与复制位置、菜单分组与一行附注。
- 目检后（2026-10-29）：
  - 网关从落地起就以 `record_usage = false` 启动，`gateway-usage.sqlite` 一条记录都没有，冷却原因无从追溯；现在打开记录，并只保留 90 天。
  - 冷却只由真实请求失败触发（`record_failure`），没有后台测活，冷却只在内存、主机重启即清空。连接列表直接写「冷却到 17:45 · 网络错误」，详情的冷却卡片加「立即恢复」（`agentConnections.clearCooldown`，连同限流休息一起解除）。
  - Codex 对每个第三方模型都会提示“找不到模型参数，按默认值运行”，用户无事可做，会话页不再显示这一行。
- 未在真实环境验证：DSH 经网关实际对话、Codex / Claude 跨来源搬会话后的第一轮。
