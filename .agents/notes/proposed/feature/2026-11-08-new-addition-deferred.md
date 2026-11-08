# Agent Note: New-addition-1102 复核里推迟的 P2 项：前提与触发信号

Status: proposed

## Problem

New-addition-1102 复核研究（Mac `~/Documents/Aeroric-research/2026-11-07-New-addition-1102-recheck/README.md` 汇总表）列了 30 项。A–D 批和 E 批做了其中的 P0、P1 和几项小的 P2；剩下的多是中等规模的新功能，2026-11-08 用户选择推迟。研究报告是工作区外的文件，代码库里需要一处记下每项为什么推迟、等什么条件再做，免得下次从头评估。

## Proposal

逐项（编号沿用汇总表）：

| # | 项 | 参照 | 前提 / 触发信号 |
|---|---|---|---|
| 11 | 推送按在场抑制：Mac 前台正看着就不推 iPhone | paseo `agent-attention-policy.ts` | Mac 和 iOS 客户端先上报前台与聚焦状态；用户抱怨在电脑前还收到推送 |
| 15 | 回合结束后的后台写入单独归属 | dsh-superagent `dsh.ts:601-672` | 用户发现改动被算进了下一个回合 |
| 16 | 宿主重启后续跑（默认关，列进"需要我处理"给"继续"） | paseo `agent-lifecycle.md:15-33` | 无人值守的委派或定时任务上线 |
| 17 | 瞬时失败退避后续跑（只限无人值守） | dsh-superagent `dsh.ts:555-584` | 同上 |
| 18 | 委派停止分"停止中 / 强制停止"两档 | paseo、Varin | 用户遇到停不下来的委派；要先出示意图 |
| 19 | 审查发现项的跨轮稳定身份与覆盖率 | pr-agent `review_finding_state.py` | 审查循环的多轮意见对不上号 |
| 20 | 委派批量计划加依赖排队（维持单写者） | pi-Coworkflow `dispatch/index.ts` | 用户要一次交多步计划 |
| 21 | 会话到提交的映射 | mallard-app | Git 页要回答"这个提交是哪个会话写的" |
| 22 | Codex 原生 Goal（`thread/goal/*`） | Codex 契约 | 能力表要开 Goal；0.167 快照里方法齐全 |
| 23 | Claude 会话导入成 Codex 原生历史 | codex-plugin-cc `externalAgentConfig/import` | 交接需要保留原生历史 |
| 24 | 配额感知调度、额度恢复后续跑 | antumi | A12 动工 |
| 25 | 出机发送账本（只记哈希和字节数，写不进就拒发） | gstack `egress-receipt.ts` | 推送、relay、诊断报告需要隐私审计 |
| 27 | 产物登记与预览租约 | VibeX | A05 动工 |
| 28 | 外部写调用门 | Mikka | 做 Forge 时 |
| 29 | 终端观察唤醒 | MonoLight | Context MCP 要能等终端输出 |

已做的不在表里：#1–#3（A、B 批）、#4–#7 与 #10（C 批）、#8–#9（D 批）、#13、#26、#30（E 批）、#12（2026-11-09 夜间，见 [独立副本](../../implemented/feature/2026-11-05-isolated-copy.md)）、#14（2026-11-09 夜间，见 [Git 锁与 worktree 安全删除](../../implemented/bug-fix/2026-11-09-git-locks-and-safe-worktree-removal.md)）。

## Alternatives considered

- **只靠研究报告，不另记**：省一篇笔记。没有采用（用户选了推荐）：研究报告在工作区外，下次评估时不一定找得到，也没有"什么时候该做"的信号。
- **写进 `docs/backlog.md`**：backlog 是总清单。没有采用：这份文件按约定只读、不覆盖。

## Acceptance criteria

- 动工其中任何一项时，从这里拿到参照位置和触发信号，做完后在本篇里划掉那一行（或整篇转为别的状态）。

## Risks

- 参照项目会更新，行号会过期；动工前要重读对方当前的代码。
- 推迟的项可能永远不会做；没有触发信号的那几项（#21、#23）真的不需要时，应当删掉这一行，而不是一直留着。
