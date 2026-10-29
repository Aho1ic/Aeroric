# Agent Note: 会话页统计栏、原生跨段选择与未识别通知

Status: implemented

## Problem

- 输入框下的“上下文 27% · 输入 518.7k · 输出 3.5k”一行上下空隙大，信息也单薄。用户给了参考：会话统计、Token 用量、上下文三块，点开各有详情。
- 会话里只能选中一段文字，跨段落、带代码块的内容复制不了。
- 会话里不断冒出“未识别的内容：thread/goal/cleared”“item/commandExecution/terminalInteraction”。
- 模型测试一点击就显示“测试失败”，过一会儿才变成通过。

## Decision

- **统计栏**（`SessionStatsBar`）：紧贴输入框，分三块，各开一张详情卡。
  - 会话统计：轮数（用户自己的消息）和步数（工具调用）取整个会话，历史加上历史尚未接管的实时行；模型用时、工具用时、首 token 平均、输出速度由实时更新的宿主时间戳算出（`SessionStats`），从本次打开会话起计。
  - Token 用量：新增协议字段 `cachedInputTokens`（Usage 更新与会话快照，可选、旧数据不受影响）。Codex 取 `tokenUsage.total.cachedInputTokens`，Claude 取累计的 `cache_read_input_tokens`，omp 暂不提供。
  - 上下文：总量、进度条、剩余。Agent 不报告系统提示词、工具定义、对话各占多少，不编造分项（Q1）。
  - 宿主不报告的数字显示“—”，或者整块不显示，不显示 0。
- **原生跨段选择**（`SelectableMarkdown`）：一条消息由一个 NSTextView（TextKit 1）显示，内容用系统 `AttributedString(markdown:)` 转换，代码块用 `NSTextBlock` 画背景。拖选、⌘A、⌘C 可以跨段落和代码块。规则沿用：图片不加载，只开 http / https / mailto 链接。正文常规字重（中文是系统字体自带的苹方 Regular），`**重点**` 改为中等粗细（Q4）。
  - 宽度贴合文字（2026-10-29 目检后）：`sizeThatFits` 每次用一套新建的 NSLayoutManager 量宽高。直接量视图自己的容器时，第二次起 `usedRect` 返回的是上次给的宽度（实测 74 pt 的文字量成 300），用户气泡因此总是撑满 70%。
- **未识别的通知**：Codex 0.163 有 87 种通知，Aeroric 处理了不到 30 种。没有专门处理的通知归为协议簿记，不进对话；类型不认识的条目（item）仍然显示，因为那是内容。
- **其他**：
  - 发送按钮改为描边纸飞机（`PaperPlane`，用户给的参考图）；
  - 运行中出现的工具行从左到右渐显 0.45 秒（`LeadingReveal`），历史行和“减少动态效果”下直接显示；
  - 模型测试中显示“测试中”和转圈，不再当作失败；
  - 没选项目时，新建会话页借第一个本地项目的配置目录，Harness 可以选；提交仍然要求先选项目；
  - 单击项目只展开或折叠；
  - 导入会话行不再显示日期，留给标签。

- **2026-10-29 目检后**：
  - 点统计栏偶尔整页变“加载中”再重载：客户端每读到 64 KiB 就开一个 `Task` 把字节交给 actor，`Task` 不保证先后，大答复（整页历史、成批更新）的分块可能拼错，行解析失败，连接判为故障，Supervisor 重启宿主。现在读队列经一条 `AsyncStream` 按到达顺序交给 actor；解析失败时日志带上解析器给的原因和字节数（不带内容）。
  - 统计栏总是显示三块，未知的数字显示“—”；会话已结束（显示“无法继续”提示）时也在。`session.history` 第一页带 `stats`：宿主从 Codex rollout（`token_count`、`task_started` / `task_complete`、按 `call_id` 配对的工具调用）和 Claude jsonl（每条 `message.id` 取最后一行的 usage、`tool_use` / `tool_result`）算出整段会话的用量与用时；会话页有实时数字时用实时的用量，用时类一律用整段会话的。OMP、DSH 暂无。
  - 统计栏放到页脚最后、紧贴底部（底边距 4 pt），提示行移到输入框上方；会话页不显示滚动条，打开或重载后停在最新一条（`defaultScrollAnchor(.bottom)`，历史第一次到达时再滚一次）。
  - 深色文字调暗一档（全 App，背景不动）：正文 F2F3F5 → C2C5CA（先试过 D9DBDF / 13:1，用户仍嫌亮），次要 B8BCC2 → 94989E，提示 8B9097 → 7A7F85；对画布分别约 10.5:1、6.3:1、4.5:1（提示文字不低于小字的 4.5:1）；增强对比度变体不变。
  - 设置 › 外观 › 字体「会话中文字体」：苹方 / Mono（更纱黑体 Sarasa Mono SC）/ Maple Mono（CN 版）/ 自定义（输入字体名或从“已安装”里选含中文的字体）。只换中文：系统字体在前，所选字体放进它的 cascade list，英文与代码不变。字体不打进安装包（Maple Mono CN 全字重约 140 MB），改为 App 内下载：设置里先标下载与占用大小，点「下载并启用」后下固定版本（更纱 v1.0.43 Unhinted SC 7z 约 50 MB、Maple v7.9 CN unhinted zip 约 140 MB），校验 sha256，用系统 `tar`（libarchive，7z 与 zip 都能读）只解出要用的字重到 Application Support/<bundle id>/Fonts，按进程注册后立即生效；每次启动重新注册。没下载时按苹方显示。
  - 外观 / 材质 / 字体滑块原来不滑动：`@AppStorage` 的写入落在 `withAnimation` 之外。`SlidingCapsulePicker` 改为自己持有动画中的 `shown`，任何存储方式都会滑；切换外观在滑动结束后再应用。设置左栏改为胶囊行（`HoverCapsuleRow`），自定义字体名用 `CapsuleField`。这些作为所有者的固定偏好写进了 `.trellis/spec/aeroric-macos/frontend/index.md`。
  - 会话排版改为用户给的参考样式：历史按轮分组（每条用户消息开始一轮），最后一条带文字的回复是答复，其余（思考、工具调用、中间回复）收进「已完成，用时 N 秒 ⌄」，点开才看；正在运行的一轮照旧展开。用户消息下方是时间与复制（悬停显示，最新一条常显），答复下方是复制 · 用量 · 时间。时间与用量来自 `session.history` 的 `stats.turns`（宿主从 rollout / jsonl 按轮算出起止与 token），App 从最新一轮往前对齐。点赞、点踩、分享不做：没有去处。正文行距约 1.75，段落间半行，列表项之间不空行、换行悬挂对齐（制表位），轮与轮之间 40 pt。
  - 大屏上内容被压到底部：`defaultScrollAnchor(.bottom)` 也会把不满一屏的内容对齐到底部；改为 macOS 15 起只设 `.initialOffset` 为底、`.alignment` 为顶（`OpensAtLatest`），macOS 14 靠打开时的 scrollTo。
  - 列表序号每项只出一次，同一项后面的段落按序号宽度缩进，代码块不再带序号。

## Alternatives considered

- **“选择文本”模式**（切换成纯文本再选）：用户明确不要。否掉。
- **保留 MarkdownUI，只在容器上开 `.textSelection`**：macOS 的选择不能跨多个 Text 视图，问题不变。否掉。
- **把统计放到宿主快照里算**：重新打开页面也能保留用时，但要改三个驱动和协议。先在客户端按实时更新累计，卡片上注明“从本次打开会话起统计”。

## Consequences

- **收益**：
  - 一条消息里的内容可以连续选中、复制；
  - 统计一眼可见，而且可以点开看详情；
  - Codex 升级加新通知也不会再刷出“未识别的内容”。
- **代价与已知上限**：
  - 表格按文字逐格排出，不再画成表格；
  - 代码块里的长行会折行，不再横向滚动；
  - 跨两条消息仍然不能连续选中（每条消息是一个文本视图）；
  - 用时在重新打开 App 后从零开始；
  - omp 没有缓存命中数据。
