# Agent Note: RpcError.detail 是闭合白名单

Status: implemented

## Problem

`RpcError.detail` 用来携带诊断信息（锁持有者、冲突 revision、出错字段）。只要它允许任意键，就会有人为了排查把连接串、token 之类塞进去，随日志与评审证据离开本机。契约里早有 `mustNotContain` 列表，但没有任何代码执行它。

## Decision

- `detail` 的键是闭合集合，共 10 个：`actualRevision`、`epoch`、`expectedRevision`、`field`、`fromSeq`、`generation`、`lockHolder`、`reason`、`taskId`、`throughSeq`。
- 闭合靠类型：Rust 侧只能经 `ErrorDetail` 结构体构造（`#[serde(deny_unknown_fields)]`，全字段 `Option` + `skip_serializing_if`，未设字段不序列化为 `null`）；宿主经 `RpcError::with_error_detail(&ErrorDetail{..})` 附诊断。加诊断字段必须先改这个类型。
- 名单由 `ErrorDetail::allowed_keys()` 导出到 `contracts/native/v1/envelope.schema.json`；生成器遍历 fixtures，任何 `detail` 出现未登记键即拒绝发布。Swift/TS 各持一份同名单并断言与契约一致。
- 帧级仍保留未知键（前向兼容），`detail` 故意相反：说不清来历的诊断字段是泄漏。
- 人读的信息（例如坏文件的路径）走 `message`，不为便利扩白名单。

## Alternatives considered

- **写出前按键名正则脱敏**：沿用日志层的做法，改动小。否掉是因为它只挡"键名像秘密"的情况，`reason` 里手写的一句连接串照样通过，且看起来是安全的。
- **只在文档里禁止**：零成本。否掉是因为第一次有人往 `detail` 塞连接串时没有任何一层会拒绝。

## Consequences

- **收益**：新增诊断字段是 diff 里可见的一次类型改动与契约再生成，泄漏面由类型封死。
- **代价与已知上限**：排查信息受限于 10 个键；确需新键时要同时改 Rust 类型、契约与 Swift/TS 名单。
