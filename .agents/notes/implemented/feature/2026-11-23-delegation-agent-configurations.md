# Agent Note: 新委派也能选 Agent 配置和模型

Status: implemented

## Problem

2026-11-22 让审查者和定时任务都能选 Agent 配置和模型（见 `2026-11-22-reviewer-and-schedule-configurations`），进度页的“新委派”还没跟上。它的 Agent 选择器只有这几项：
- 按角色默认、与本会话同族、另一族；
- Claude Code、Codex 两个 Harness。

两头其实都已经支持，只是中间没接上：
- 协议里的 `LiveDelegateAgent.named(agent:agentConfigId:model:)` 早就能带配置和模型（B2-6，角色库保存角色时用）；
- 宿主的 `delegation.start` 收到 `agentConfigId`，会把它写进子会话的 `NewTask`。

所以用户想把一件事交给某个配置下的某个模型（例如 Magpie 下的 `gpt-6.2-sol`），只能先去角色库存一个绑定了配置的角色，再用那个角色委派。

## Decision

**选择器多一段“Agent 配置”，写法和审查者那一段一样：**
- 只有一个模型的配置，点了直接选；
- 有多个模型的，展开再选模型；
- 同族的配置标“（同族）”。

**清单从哪来：** 直接用 `review.reviewers` 返回的 `configurations`。
- 宿主给委派挑 Harness 用的就是审查者那张表（`REVIEWERS`，“本机没有可以接委派的 Claude Code 或 Codex”），能当审查者的配置就能接委派，不用另开接口；
- App 读委派面板时，每个任务读一次，存进 `DelegationPanelModel.configurations`。

**选中的显示：**
- 选择器标题写“配置名 · 模型”；
- 选中的配置不在清单里了（被删、被拦截），标题写“配置 \<id\>（现在不能用）”。宿主开子会话时用不了这个配置，会如实报错。

**纯逻辑：** 放在 `DelegationPanelModel.agentTitle` 和 `DelegationPanelModel.pick` 里，好测。

## Alternatives considered

**另加一个 `delegation.configurations` 接口。** 没选：委派和审查用的是同一张 Harness 表，配置清单也一样，两个接口只会以后对不上。

**开委派前像审查那样先核对配置。** 这次没做：委派开子会话时，宿主就会按配置启动，用不了就报错，委派卡上能看到原因。审查要核对，是因为它在后台自动开、没人看着。

## Consequences

- **收益：** 新委派可以直接交给某个配置和模型，不用先存角色。
- **局限：**
  - 选中的配置失效了，要到开子会话时才报错，表单上只在标题里写“现在不能用”；
  - 还没目检。
- **验证：**
  - Swift 新增 1 个测试：选配置时默认取第一个模型，选模型时用选的那个；发出去的 JSON 带 `agentConfigId` 和 `model`；标题写“配置名 · 模型”，失效的写“现在不能用”；不用配置时的几种标题不变。
  - App 层的改动由全量构建编过。
