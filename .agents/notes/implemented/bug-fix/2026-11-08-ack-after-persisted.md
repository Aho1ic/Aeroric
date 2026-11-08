# Agent Note: 先落盘、先送达，再告诉调用方成功

Status: implemented

## Problem

New-addition-1102 复核研究（gugu 的"持久提交成功之后才 ACK"）指出两处先报成功、后做事的地方，2026-11-08 用假数据都复现了：

- **投递入队**：`delivery::Store::save` 写库失败只打印一行，条目照样留在内存里；`Deliveries::enqueue` 返回条目，调用方据此告诉用户"已排队"。复现用例删掉 `deliveries` 表后入队，返回成功；重启后这条不见了。
- **委派发送**：`Delegations::send` 和自动合批 `try_return` 先把卡片标成 `sent`、发布、记审计，再另起线程调用 `host.prompt`；失败只打印。复现用例让 `prompt` 失败，卡片仍写"已发送"、没有错误，也不能再发。

## Decision

- **投递**：
  - `Store::add` 先写库、成功后才放进内存；`enqueue` 用它，失败返回 `Err("没能排上队（写入投递队列失败：…）")`，条目不进队列、不发布。
  - 已入队条目之后的状态变化仍用 `Store::save`：写库失败时保留内存状态并记日志（[统一投递队列](../feature/2026-11-02-delivery-queue.md) 的原规则，对已有条目不变）。
  - 宿主侧：`session.prompt` 回 `IO_ERROR`；内部投递 `enqueue_internal` 返回 `Result`，委派和审查的 `prompt` 把它往上传；"发送审查意见"用 `change_review::take_comments(task, send)`，排上队才把评论标为已发送。
- **委派**：
  - 发送失败（包括上面的入队失败）时 `unsend`：撤回 `sent`，卡片 `error` 加一行"没送到父会话：原因"，可以再点发送；自动合批同样处理，之后的触发会再合批一次。
  - 这一行挂在卡片自己的错误下面，重新发送时去掉，不覆盖失败卡片原有的错误。
  - 审计里的"发送"记录不撤回：它记的是用户或 Aeroric 发起了发送。

## Alternatives considered

- **入队失败时照旧放进内存，只给条目标"未保存"**：最强的理由是这次还能发出去。没有采用（用户选了推荐）：重启后照样丢，而且调用方已经被告知成功，和问题本身一样。
- **委派等 `prompt` 返回后再标已发送**：最直接。没有采用：`send` 在锁里被界面同步调用，`prompt` 可能要拉起会话，会卡住调用方。
- **给卡片加一个独立的 `sendError` 字段**：语义最干净。没有采用：要改协议、契约和 Swift 解码；沿用 `error` 加一行已经能让界面说清楚。

## Consequences

- **收益**：界面说"已排队""已发送"时，条目确实已经落盘或已经交给投递队列；失败会被看见，也能再试。
- **代价**：
  - 数据库坏掉时，用户发消息会直接报错，而不是"先发出去、重启后再说"。
  - 卡片的 `error` 可能是两行（原有失败原因加没送到）；界面原样显示。
- **验证**：
  - `delivery::tests::an_item_that_cannot_be_written_down_is_not_queued`（原复现用例改为反向断言）；
  - `delegation::tests::a_send_that_never_arrives_can_be_sent_again`；
  - `change_review_tests::comments_wait_and_go_once` 覆盖排不上队时评论仍算未发送。
