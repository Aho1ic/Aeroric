# Agent Note: 项目身份只做无损归一，合并永远是人的动作

Status: implemented

## Problem

同一个目录可以有多种拼法（尾部分隔符、`\` 与 `/`、手工编辑过的 `projects.json`、CLI 的 `--path`、远程输入）。若不归一，新入口会制造重复项目；若归一过头（例如统一小写）或自动合并，会把两个真实不同的目录合成一个，并留下没人认领的 `projects/<id>/tasks.json`。

## Decision

- 身份的唯一拼法是 `ProjectTarget::identity_key()`；core 里不出现第二套"怎么算同一个项目"。
- 对 `local` target 只做无损归一：去掉多余尾部分隔符、`\` 与 `/` 视为同一分隔符。**不改大小写**。`ssh://…` 这类显示串不参与文件系统语义。
- 不自动合并已有记录。打开时先按归一键找现有记录，再按字面找；新入口造成的重复只会 touch 现有项目。合并是产品功能（带合并报告、需人确认）。
- 新项目 id 由调用方生成 uuidv7，写入前拒绝重复 id；旧的毫秒 id 继续可读、可寻址，不改写。
- reorder 时按首次出现去重，未知 id 忽略。
- 分组名单（含顺序）的唯一真相源是 core 独占文件 `~/.aeroric/project-groups.json`；不放进强类型的 `settings.json`（下一次保存会抹掉未知键）。旧客户端 localStorage 中的名单只在 core 文件不存在时一次性幂等导入。显示名单仍与"项目记录里出现但名单里没有的分组"取并集。
- 认不出的 `location.kind`：列表照常显示，`open`/`touch`/`rename`/`assign_group`/`reorder` 返回 `INVALID_ARGUMENT`（`detail.field = "location"`）；`remove` 永远可用，它只按 id 抹记录、不碰磁盘。

## Alternatives considered

- **逐字保持现状**：零行为变化。否掉是因为明知新入口会造成重复却不留防线。
- **完整归一（含大小写）+ 一次性自动合并**：用户看不到重复项。否掉是因为大小写不敏感是文件系统属性而不是路径属性，且自动合并是对用户数据的改写。
- **分组名单取 core 与 localStorage 的并集**：名字不丢。否掉是因为顺序仍会分叉，且"谁先进 core"取决于导入时机，日后无法解释。

## Consequences

- **收益**：core 不会新增重复记录；坏记录总能被删除，不会成为只能手改数据文件才能清掉的钉子。
- **代价与已知上限**：已存在的重复拼法会一直保留，直到产品提供合并功能。
