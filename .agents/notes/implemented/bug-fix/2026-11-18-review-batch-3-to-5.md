# Agent Note: 全量审查 Batch 3–5——界面规范、远程链路加固与收尾

Status: implemented

## Problem

2026-11-17 全量审查交接单（`.local_docs/2026-11-17-project-review-handoff.md`，本地）的 Batch 3–5。复核后，本篇处理的这些都成立：

- **B3 界面规范**：
  - 22 处文本框用系统 `.roundedBorder`（交接单只列了 6 处），讨论的"具体要求"和底部发言框是手绘圆角矩形；
  - 重跑面板用 `.scrollIndicators(.automatic)`；popover 和 sheet 是单独的窗口，接不到主窗口根上隐藏滚动条的设置，60 多个 `ScrollView` 没有自己的设置；`NativeTextEditor` 的 AppKit 滚动视图显示滚动条；
  - 讨论的"方式 / 轮数 / 最多修订"是一排芯片，不是 `CapsuleSlider`；统计栏、问答选项行、`#` 补全行、会话选择表的悬停或选中底色是圆角矩形或没有；
  - 升级胶囊、升级卡片、导入选择表的动画没有看"减少动态效果"。
- **B4 远程链路**：
  - Relay 的 `splice` 两个方向 `join!` 到底：桌面关了、手机不发 Close，桥就永远挂着；
  - 桌面数据连接的连接槽在交给桥之后就归还了，计数偏低，512 个槽可以被耗尽；
  - Relay 告诉桌面的客户端地址是反代的回环地址；`/host` 注册时 hostId 不限字符，会写进日志；
  - 推送发送端：
    - 不限请求体大小；
    - 重放表在限流之前，表满时会淘汰还在窗口内的 nonce；
    - nonce 不分用户；
    - `token` 不校验，会进 APNs 的请求路径；
    - `device_ids` 不去重、不限数量；
    - `/v1/enroll` 不限流，能用已有用户名覆盖别人的凭据；
    - 密钥比较先比长度；
    - 读 `.p8` 失败时只报"为空"。
  - iOS 伴随端：
    - 没挂 `UIApplicationDelegateAdaptor`，APNs token 回调根本收不到；
    - 设备被撤销后，项目、任务、消息、文件、git 列表都还留着；
    - 控制按钮不看 Mac 提供了什么，失败被 `try?` 吞掉。
- **B5 收尾**：
  - `omp_managed_home()` 固定写 `~/.aeroric`，不看 `AERORIC_HOME`；
  - 讨论、委派、待办、回合账本、登录记录落盘用固定的 `.tmp` 名，并发写会互相覆盖；宿主每个请求各开一个 `Todos`，它的锁管不到别的请求；共享原子写的临时名只到微秒，同进程两次写可能撞名；
  - CI 不跑 iOS 的 `swift test`、契约和摘要检查；
  - `export-schema` 的摘要参数顺序错位；
  - `git_services` 测试被 Finder 的 `.DS_Store` 打断；
  - `Package.swift` 用弃用的 `.exact(...)`；
  - UI 测试清理用的 `pkill -f` 把数据目录路径当正则；
  - 文档写 Node 24+，而脚本实际在 22 上运行。

## Decision

- **B3-1**：
  - 新增 `CapsuleTextFieldStyle`（`.textFieldStyle(.capsule)`），外观与 `CapsuleField` 相同，但保留原 `TextField` 的焦点、提交和多行。22 处一律换成它；
  - 讨论的两个输入框背景和描边改为 `Capsule`。
- **B3-2**：
  - 重跑面板改 `.hidden`；`TurnBill` 的瀑布图加 `.hidden`；
  - 用脚本给 `AeroricUI` 和 App 里每个还没有设置的 `ScrollView` 补上 `.scrollIndicators(.hidden)`，共 66 处；
  - `NativeTextEditor` 关掉 AppKit 的横竖滚动条，行号栏保留。
- **B3-3**：
  - 讨论的"方式 / 轮数 / 最多修订"换成 `CapsuleSlider`；
  - 统计栏按钮、问答选项行改用 `Capsule`；
  - `#` 补全行和会话选择表的行改用 `HoverCapsuleRow`。
- **B3-4（部分）**：
  - 三处动画看 `accessibilityReduceMotion`：开着时不动，或只淡入；
  - 交接单列的数字定宽，只有两处直接写在 `Text` 里，已补 `.monospacedDigit()`；其余是格式化好的字符串，没有逐处改。
  - 2026-11-18 补完：交接单的行号已过时，改成在整块视图上加 `.monospacedDigit()`（讨论页、委派卡、回合撤销条与撤销表、待办页、Agent 升级页、升级提醒胶囊），只影响数字宽度；升级页和升级胶囊里的胶囊按钮一律 `.fixedSize()`，胶囊里的长句 `.lineLimit(1)`。
- **B4-2**：
  - `splice` 改成任一方向结束后，另一方向最多再排空 `SPLICE_DRAIN`（5 秒）；两边都静默超过 `SPLICE_IDLE`（120 秒）就断开；
  - 桌面数据连接的连接槽随连接一起交给桥（`DataConnection`），桥结束才归还；
  - `/host` 的 hostId 只允许字母、数字、`-`、`_`。
- **B4-5**：
  - Relay 告诉桌面的客户端地址改成解析后的真实地址；文档的 Nginx 示例补上用 `$remote_addr` 覆盖 `X-Forwarded-For`、`X-Real-IP`，并说明原因；
  - 推送发送端：
    - 路由限 64 KiB；先校验 user、nonce、device id 的长度和字符，`device_ids` 不超过 32 个；
    - 鉴权顺序改为 时间戳 → 签名 → 限流 → 重放；
    - 重放按 `(user, nonce)` 记账，表满时返回限流，不再淘汰；
    - `bind` 的 token 必须是 32–512 位偶数长度的十六进制，platform 只允许短的字母数字；
    - 推送前去重设备；
    - `/v1/enroll` 走限流，并且不覆盖已有用户（新加 `enroll_new`；本地 `enroll` 子命令仍可覆盖）；
    - 密钥比较先各自取 SHA-256 再定长比较；
    - `.p8` 读不出时报出路径和原因。
  - Pass-the-Hash 一项没改：要换凭据派生方式，宿主签名侧也得一起迁移，需要单独的方案。
- **B4-4**：
  - App 挂上 `@UIApplicationDelegateAdaptor`，启动时把推送协调器交给它；
  - `handleDeviceRevoked` 清空项目、任务、选择、消息、文件、git 列表、控制结果和适配器能力；删掉没用的旧函数；
  - 控制按钮只列 `offeredControls`；审批和控制失败显示在横幅里。
  - 客户端方法名和宿主对齐，依赖远程面接线，见 [远程面方案](../../proposed/architecture/2026-11-18-production-remote-surface.md)。
- **B4-3 / B5-1**：按用户决定只写方案：[远程面](../../proposed/architecture/2026-11-18-production-remote-surface.md)、[契约对账](../../proposed/architecture/2026-11-18-contract-surface-backfill.md)。
- **B5-2**：
  - 用户选了方案 B：宿主生产启动时注入 omp 托管 home 为 `<data-dir>/agent-homes/omp`，未注入时仍是旧根；
  - 两个数据根的全量统一另立项。见 [两个数据根](../architecture/2026-11-18-two-data-roots.md)。
  - 先试过直接改 `omp_managed_home()` 走 `aeroric_dir()`，被钉住旧路径的宿主测试拦下，已撤回。
- **B5-3**：
  - 五处落盘都改走 `storage::atomic::write_atomic`（委派与检查点共用的 `write_task_file` 改为 uuid 临时名），失败写进日志；
  - 讨论和委派各加一把写锁，读当前状态和写盘在同一把锁里，旧快照不会晚于新快照落盘；
  - `Todos` 的锁改成进程级静态锁；
  - 共享原子写的临时名加进程内递增序号。
  - "只读实例拒写"一项没做：这五个模块拿不到写者租约，要从宿主传进来，改动面较大。
  - 2026-11-18 复核后定为不做：五个模块只活在 `aeroric-cli serve` 里，而 `serve` 拿不到租约就不启动，租约是整个进程期间持有的系统文件锁；只有锁文件在运行中被删掉或换掉才会出现"别人也在写"。为这种情况把上下文穿过五个模块，代价大于收益。
- **B5-4**：
  - CI 加跑 iOS 的 `swift test`、`contracts.mjs --check`、`digest.mjs --check`；
  - 修正 `export-schema` 的参数顺序；
  - `git_services` 测试滤掉 `.DS_Store`；
  - `Package.swift` 改成 `exact:`；
  - `pkill -f` 的路径先做正则转义；
  - 文档写"Node 22.18+（推荐 24）"，`native:doctor` 校验这一版本线。
  - 交接单建议改用"记录的子进程 PID"清理。没做：要杀的是上一个 App 实例留下的 serve，本实例没有它的 PID。

## Alternatives considered

**B3-1：每处都换成 `CapsuleField`。** 最强的理由是只用一个原语。没选：`CapsuleField` 不支持多行、焦点绑定和各处已有的修饰符，逐处改写容易丢行为；做成文本框样式，外观完全一样，改动只有一个词。

**B3-2：只给交接单点名的三处加。** 最强的理由是改动最小。没选：同类问题有 60 多处，规则是"任何界面都不显示滚动条"；用脚本统一补，以后也好用同一条扫描检查。

**B4-2：每个方向各自设空闲超时。** 最强的理由是实现简单。没选：单向持续推送时，另一方向长期静默是正常的；按两个方向合计的最后活动时间判断才对。

**B4-5：重放表满时淘汰最旧的一半（原做法）。** 最强的理由是不会因表满而拒绝正常请求。没选：被淘汰的 nonce 还在时间窗内，正好可以拿来重放；改为先限流再记账后，只有通过认证、又没超额的请求能占表，表满说明确实过载，拒绝是如实的。

**B5-3：每个模块自己写 `uuid` 临时名。** 最强的理由是不碰共享代码。没选：共享原子写已经有 `sync_all`、`0600` 和残留清理，统一走它，五处行为一致。

## Consequences

- **收益**：
  - 界面规范的两条扫描（`roundedBorder`、显式显示滚动条）清零；
  - Relay 不会被半开连接耗尽；推送发送端的未认证输入有上限，重放挡得住；
  - 伴随端能收到 APNs token，撤销后不留缓存；
  - 并发落盘不再互相覆盖。
- **代价与未做**：
  - B4-3、B5-1 只有方案；
  - B4-5 的 Pass-the-Hash；
  - 两个数据根的全量统一（另立项）；
  - B5-3 的只读实例拒写；
  - 伴随端终端输入仍用 `try?`。
- **验证**：见交接单第 5 节；界面改动没有目检，记在路线文档 §8。
