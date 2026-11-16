# Agent Note: A1 委派结果卡跟随投递回执

Status: implemented

## Problem

[批次 A](../../proposed/bug-fix/2026-11-15-adoption-a-truthful-state.md) 的 A1。2026-11-16 在 `main@d5126573` 上复核，位置与 `8d4391c3` 相同：

- `core/crates/aeroric-core/src/sessions/live/delegation/mod.rs:658`（`send`）和 `:1046`（`try_return` 自动回传）先写 `card.sent = true` 并发布，再另起线程调 `host.prompt`。
- `core/crates/aeroric-host/src/session_live_services/delivery.rs:92–114`：`enqueue_internal` 把入队返回的 `Delivery` 丢掉（`.map(|_| ())`），`source_ref` 传 `None`。

结果卡和投递记录是两份状态。卡片写"已发给主会话"时，只说明入队调用没报错；之后投递失败、结果不明、被撤回，卡片都不知道。用户看到"已发送"，主会话其实可能没收到。

## Decision

**卡片引用投递记录，发送状态从投递记录派生。**

1. **入队**：委派结果不经 `DelegationHost::prompt`，由 `Delegations::hand_over` 直接调 `Deliveries::enqueue_once`，`source_ref = "delegation:<parentTaskId>/<id>[+<id>…]"`（合批时卡片 id 排序后用 `+` 拼接，见 `delegation::source_ref`），拿回 `Delivery`。返工说明仍走 `host.prompt`。宿主的 `enqueue_internal` 返回 `Delivery` 并接收 `source_ref`；委派返工和审查两处调用方暂不跟随记录，传 `None`。
2. **去重**：`enqueue_once` 对同一会话、同一 `source_ref`、仍在路上（`waiting`、`sending`、`accepted`、`paused`）的记录直接返回已有的那条。查找和写入在同一把锁里。普通 `enqueue` 不去重：用户消息用 `source_ref = "handoff"` 标记"带交接简报"，两条不能合成一条。
3. **协议**：`DelegationCard` 加可选字段 `returnDelivery { id, state, reason?, evidence? }`（`ReturnDelivery`）。`sent` 保留，含义是"已交给投递队列"；界面不读它判断送没送到。`needs_user()` 在 `returnDelivery.state == unknown` 时为真。
4. **跟随**：`Deliveries::set_watcher` 注册的回调在每次 `publish` 时被调用（不持有队列的锁）。`Delegations::follow` 记下队列并注册回调，执行器在组装 sink 链时调用它。回调进 `on_delivery`：只处理 `kind = delegation` 且 `source_ref` 以 `delegation:` 开头的记录，按 `returnDelivery.id` 找卡片，从队列重读这条记录的**当前**状态（回调可能乱序到达）再写进卡片、发布。`returnDelivery` 只由这条路径、`hand_over` 的绑定和读回时的重读写入。
5. **绑定**：`hand_over` 入队后在委派的锁里读一次队列的当前记录再写进卡片。锁的顺序是委派 → 队列；队列从不在持锁时回调，所以不会死锁。绑定前发生的变化已读到，绑定后的变化在 `on_delivery` 里等锁。
6. **重启**：`ensure_loaded` 读回卡片时按 `returnDelivery.id` 从队列重读。队列里没有这条记录、而它又没到 `delivered`/`withdrawn` 的，记为 `unknown`，原因 `RECORD_GONE`（"投递记录不见了"）。
7. **各状态下的"发送"**（`Delegations::send`）：
   - 没交出去：新投递。
   - `failed`：对同一条记录 `resend`（合批时整批重发）；`paused`：对同一条记录 `resume`。卡片不撤回 `sent`，也不进自动合批：投递队列自己会在下一条消息进队时重试失败项（Q-S8），卡片若再合批会让主会话收到两遍。
   - `unknown`、`withdrawn`：生成只含这张卡的**新**记录，旧记录保持原状，卡片改为跟随新记录。
   - 其他状态，或 A1 之前写下、只有 `sent = true` 没有记录的旧卡：`NothingToSend`。
   - 入队本身失败（写不进投递库）：沿用 C 批的 `unsend`，撤回 `sent`、错误加一行"没送到父会话：原因"。
8. **过期事件**：只按卡片当前的 `returnDelivery.id` 匹配。再发前的、退回重做前的旧记录后到的变化找不到卡片，直接忽略。`redo` 清空 `returnDelivery`。
9. **Swift**：`LiveDelegationCard.returnDelivery`（`LiveDelegationReturn`）。`DelegationText.state` 在卡片结束后追加发送状态：已入队、正在发送、对方已收下、已送达、没送到、结果不明、已暂停、已撤回；旧卡写"已发送（没有投递记录）"。`deliveryNote` 进脚注：没送到的原因、结果不明时"先去主会话核对再决定要不要再发"、排队时"等主会话这一轮结束再发"。`canSend` 在记录处于 `failed/paused/unknown/withdrawn` 时为真，按钮文字是"再发一次"，会话发起的委派也显示。
10. **MCP**：`context.delegations` 原样返回卡片，带 `returnDelivery`；`delegations` 工具的说明写明 `delivered` 才是已送达。

### 同主题笔记审计

| 笔记 | 分类 | 处理 |
|---|---|---|
| [委派与契约](../feature/2026-10-31-delegation.md) | 被本篇修改 | 已原地更新"回传主会话"和委派卡按钮的事实 |
| [统一投递队列](../feature/2026-11-02-delivery-queue.md) | 被本篇修改 | 已原地补上 `enqueue_once`、观察者回调和委派结果的入队位置 |
| [先落盘、先送达](./2026-11-08-ack-after-persisted.md) | 部分取代 | 它的"发送失败就 `unsend`"已缩小为"入队失败才 `unsend`"，测试名同步，互链 |
| [批次 A](../../proposed/bug-fix/2026-11-15-adoption-a-truthful-state.md)、[落地总纲](../../proposed/architecture/2026-11-15-research-adoption-roadmap.md) | 上级方案 | 不动；A1 状态和待目检项已回写路线文档 §8 |

## Alternatives considered

**卡片只存 `returnDeliveryId`，Swift 在同一会话的投递更新里查状态。** 最强的理由是真正只有一份状态。没选它：`needs_user`（需要我处理）、`context.delegations` 都在 core 里算，core 本来就要知道状态；委派面板也会在没打开主会话时显示卡片。卡片上放一个由队列单向写入的副本，代价是一个 JSON 字段。

**`failed` 时照旧撤回 `sent`，让自动合批再发。** 最强的理由是沿用 C 批的行为，代码最少。没选它：投递队列也会重试同一条失败记录，两边各发一次就会让主会话收到两遍；合批的卡片集合一变，`source_ref` 也变，去重拦不住。

**在 `DelegationHost` 上加 `return_results`、`resend_delivery` 两个方法，由宿主转给队列。** 最强的理由是保持"委派只经宿主接口"的形状。没选它：投递队列和委派都在 core 里、由同一个执行器组装，宿主那层只是转发；测试反而要在假宿主里再写一遍队列。

**`enqueue` 直接对所有带 `source_ref` 的调用去重。** 最强的理由是不加新方法。没选它：用户消息的 `"handoff"` 引用不是"一件事"，两条用户消息不能合成一条。

## Consequences

- **收益**：卡片写"已送达"时，投递记录一定是 `delivered` 并带证据；驱动拒收、接收后断线、入队写库失败都不会留下"已发送"的卡。主会话的 Agent 也能从 `delegations` 看到结论有没有送到。
- **代价**：
  - 自动回传的时序变了：入队后由队列决定何时发。主会话忙时卡片停在"已入队"，脚注写明在等这一轮结束。
  - 合批记录的"再发一次"整批重发：点任意一张卡都会重发同一批的全部结果，它们本来就在同一条消息里。
  - 旧卡片（A1 之前）只能写"已发送（没有投递记录）"，不能再发。
  - 宿主启动时如果委派卡在投递库打开之前被读回，找不到记录会被记成"投递记录不见了"。现在两者都在宿主装配阶段完成、先于任何请求，没有观察到这种顺序。
  - 审查回环的投递仍不被卡片跟随（不在 A1 范围）。
- **验证**（2026-11-16，Mac 上跑）：
  - core `delegation::tests`：`a_card_is_delivered_only_with_its_records_evidence`（完成条件一）、`a_refused_result_says_why_and_goes_again_as_the_same_delivery`（入队成功后驱动拒收、Agent 拒收，重发同一条）、`a_lost_answer_needs_the_user_and_a_late_receipt_of_the_old_delivery_is_ignored`（接收后断线、过期事件、退回重做清空）、`sending_twice_queues_one_delivery`（连点两次）、`after_a_restart_the_card_takes_its_deliverys_state_from_the_queue`（入队后、送达前重启）、`a_card_whose_delivery_is_gone_is_unknown`、`a_batch_is_one_delivery_every_card_follows`、`a_result_that_cannot_be_queued_can_be_sent_again`（落盘失败）。测试桩用真实的 `Deliveries` 加假宿主，接法与执行器相同。
  - core `delivery::tests`：`the_same_source_on_its_way_is_queued_once`、`the_watcher_hears_every_change`。
  - protocol：`a_result_nobody_knows_arrived_needs_the_user`（含旧卡解码）；夹具加 `returnDelivery`，`sourceRef` 改为 `delegation:task-1/dg-1`。
  - 宿主端到端 `session_live`：委派结果经 Codex 送达后，卡片的 `returnDelivery` 是同一条记录、`delivered`、证据 `protocol`。
  - Swift：`DelegationTextTests.testAResultCardSaysWhereItsResultStands`、合同测试解码 `returnDelivery`。
  - 界面行为未目检，记在路线文档 §8 待目检清单。
