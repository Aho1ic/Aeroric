# ADR-008：`RpcError.detail` 是闭合白名单，未登记的键不得上线

- 状态：**accepted（用户在 P2-01 门禁第二轮拍板，2026-09-22）**
- 日期：2026-09-22
- 关联：`MIGRATION_PLAN.md` §2.3（错误码与 `detail` 字段）、ADR-001 §5（Q4 的日志写入前脱敏）、P7-04（DBX 连接串）、P8-02（provider key）、P8-03（配对材料）、P3-06（宿主构造 `detail`）

## 背景：一条只写在纸面上的禁止

契约里早就有 `frameKinds.failure.properties.error.detail.mustNotContain = ["token","password",...]`，但实测：这个列表只出现在**生成的工件、`export-schema` 与 1 条 TS 断言**里，没有任何宿主或客户端代码执行它（宿主本身是 P3-06 的事）。而 `detail` 的设计用途恰恰是携带诊断信息——锁持有者、gap 游标、冲突的 expected/actual revision、出错的字段名。

于是有一条真实的路径：P7-04 起宿主会在 DBX 连接错误里塞 `connectionString` 方便排查 → 这条 `detail` 进 `.artifacts/native/` 日志与 review evidence → 密钥跟着离开本机。计划里同类的坑已经在 Q4 用"日志写入前脱敏"关掉了，但那是**日志层**的守卫；协议层没有等价物。

## 决策

1. **`detail` 的键集合是闭合白名单**，共 10 个：`actualRevision`、`epoch`、`expectedRevision`、`field`、`fromSeq`、`generation`、`lockHolder`、`reason`、`taskId`、`throughSeq`。未登记的键**不得上线**。
2. 闭合靠**类型**而不是靠检查：Rust 侧 `RpcError.detail` 只能由 `ErrorDetail` 结构体构造（`#[serde(deny_unknown_fields)]`，全字段 `Option` + `skip_serializing_if`），TS/Swift 各自有一份同名单与 `isAllowedErrorDetailKey` / `errorDetailAllowedKeys`。加一个诊断字段必须先改这个类型，于是它是 diff 里可见的一次决定，而不是某条错误路径里的一次顺手。
3. **名单由序列化器自己导出**（`ErrorDetail::allowed_keys()`），`contracts/native/v1/envelope.schema.json` 的 `constants.errorDetailAllowedKeys` 与 `errorDetail.keys` 都来自它；生成器还会遍历 fixtures，任何 `detail` 里出现未登记键直接**拒绝发布**（实测：塞一个 `connectionString` → `a fixture puts "connectionString" into an error detail, which is not in the allowlist`）。
4. `mustNotContain` 保留，但角色降级为**说明为什么闭合**，不再是唯一的防线。
5. _unset 的可选字段不得序列化成 `null`（计划 §2.3 对"缺失与显式 null 分开"的要求在这里同样生效）。

## 为什么这里可以不前向兼容

帧级仍保留未知键（§2.3 的前向兼容），`detail` 故意相反：一个说不清来历的诊断字段不是诊断，是**长得像诊断的泄漏**。版本策略（ADR-009）已规定主版本必须严格相等，所以"新宿主先发、老客户端后更"这条在 v1 内不成立，闭合集合不会把客户端钉死。

## 备选与被否决的理由

- **写出前正则脱敏**：沿用日志那套键名规则。被否决——它只挡得住"键名看起来像秘密"，`detail.reason` 里一句手写连接串照样过去，而且看起来是安全的。
- **只写文档禁止（现状）**：被否决——P7-04 之后第一次有人为了排查往 `detail` 塞连接串，就会静默发生，且没有任何一层会拒绝。

## 后果

- 宿主（P3-06）只能经由 `RpcError::with_error_detail(&ErrorDetail{..})` 附诊断；新增诊断字段是一次可见的类型改动 + 契约再生成。
- 三方各有一条断言核对"发布的名单 == 代码里的名单"，`native:check protocol` 的 `contracts-current` 保证工件不与类型漂移。
