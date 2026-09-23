# ADR-013：项目身份只做无损归一，合并永远是人的动作

- 状态：**accepted（用户在 P2-03 门禁 Q27～Q34 拍板，2026-09-22）**
- 日期：2026-09-22
- 关联：`MIGRATION_PLAN.md` P2-03 Action 2（路径比较与 projectId 稳定性保留）、ADR-012（内容指纹才是跨 writer 的真相）、`core/crates/aeroric-core/src/projects/mutations.rs`、`aeroric_protocol::ProjectTarget::identity_key`

## 背景：重复项目不是今天的问题，是搬家会带来的问题

事实核对：GUI 的本地路径只来自系统目录选择器（`src/App.tsx:372-375` 的 `openDialog({ directory: true })`），它给的就是规范拼法，所以旧客户端里 `/repo/one/` 与 `/repo/one`、`/Repo/One` 与 `/repo/one` 会成两条记录这件事**理论上成立、实践里几乎不发生**。真正会带来不同拼法的是新增入口：`aeroric-cli --path`（P3-07）、移动/远程输入、以及被手工编辑过的 `projects.json`。

同时，`projects/<id>/tasks.json` 是按 **id** 存的：把两条记录"合并成一条"必然留下一个没人认领的任务目录。所以这不是格式化细节，是一次数据改写。

## 决策

1. **只做无损归一**：对 `local` target 比较身份前，允许去掉多余的尾部分隔符、允许 `\` 与 `/` 视为同一分隔符。**不改大小写**——大小写不敏感是文件系统属性而不是路径属性，跨平台统一小写会把两个真实不同的目录合成一个，那是更难发现的错误。
2. **不自动合并已有记录**：竖条里已经存在的不同拼法各自保留。归一只影响"打开时先按归一键找现有记录，再按字面找"，即新入口造成的重复只会 touch 现有项目，不会新增一条。
3. **合并是一次可审阅的动作**，属于产品功能（带合并报告、需要人确认），不属于迁移；迁移把行为搬过去时**不许顺手改变身份判定**。
4. **身份的唯一拼法仍然是 `ProjectTarget::identity_key()`**：core 里不出现第二套"怎么算同一个项目"。归一在算 key 之前发生，`ssh://…` 这类显示串不参与文件系统语义（`an_ssh_path_is_never_a_local_path` 已钉）。

## 备选与被否决的理由

- **逐字保持现状**（parity 就是不改行为）：最保守，但等于明知新入口会造成重复却不留任何防线；重复项目对用户是"我的项目怎么冒出两个"。
- **core 做完整归一 + 一次性自动合并**：把真实数据改写塞进迁移里。旧客户端在 P2-05 之前仍是同一个目录的写入者，自动合并撞上进行中的写入就是丢任务记录，风险与收益完全不成比例。

## 后果

- 归一函数必须是纯函数并有反例：`/repo/one/` 与 `/repo/one` 归一到同一身份；`/Repo/One` 与 `/repo/one` **不**归一；`C:\repo\app` 与 `/repo/app` 归一（同一 local 语义）；`ssh://c/app` 不被当作本地路径。
- 旧客户端仍按字面判定，所以在 P2-05 之前"更严格的那一侧"是 core：core 不会新增重复记录，旧客户端可能新增。这是可接受的不对称，反过来（core 制造重复）才不可接受。

## 同轮追加（Q28～Q34）

1. **新项目 id 用 uuidv7，旧 id 一律不动**（Q28）。今天 `id = String(Date.now())`：同一毫秒打开两个项目会撞成同一个 id，后写者顶掉前者的目录归属。新 core 只接受调用方给的 id（宿主/CLI 负责 uuidv7，与 §2.3 的 epoch 同一套约定），并且**写入前必须拒绝重复 id**——把唯一性责任放在生成方，把冲突检测放在写入方。旧的毫秒 id 继续可读、继续可寻址，不改写、不迁移。
2. **`handleOpen` 的双调用现在修**（Q29）：`src/App.tsx:372-381` 调了两次 `upsertLocalProject`，两次的 `Date.now()` 可以不同，于是激活用的 id 与落库的 id 可能不是同一个。改成算一次、两处共用，并加一条 mock `Date.now` 跨毫秒的反例：被激活的 id 必须出现在落库列表里。
3. **reorder 的重复 id 两侧都先去重**（Q30）。实测 `orderedProjectIds = [a, a, b]` 今天会产出三条记录、`a` 出现两次并各带一个 orderIndex；`projects.json` 是整份覆盖写，所以这条脏数据会一直留在文件里。规则：按首次出现去重，顺序不变；未知 id 照旧忽略。
4. **分组清单搬进核心独占文件 `~/.aeroric/project-groups.json`**（Q31、Q34）。不能放 `settings.json`：`AppSettings` 是强类型 struct（`app_settings/schema.rs:551`），下一次保存只写自己的字段，会把未知键**抹掉**。也不能继续只住 localStorage：SwiftUI 与 CLI 看不见它。到 P2-05 之前是**两个文件、各自单一写入者**（core 写文件、旧客户端写 localStorage），P2-05 的验收必须显式给出合并规则（首次移交以谁为准、冲突怎么办），不许留"读文件优先"这种实际不可达的说法——旧客户端不知道这个文件存在。
5. **认不出的 `location.kind` = 看得见但不可寻址**（Q32）。线协议今天就会拒绝未知 kind（`ProjectTarget` 是 tagged enum，无 `#[serde(other)]` 兜底），只有 helper 会吞掉它；TypeScript 侧则是原样透传，结果 `isRemoteProject` 当它是远程、命令派发又回落到本地裸命令——**在最坏的猜测方向上猜错了**。core 的新规则：`list`/`rail` 照常显示；`open`/`touch`/`rename`/`assign_group`/`reorder` 返回 `INVALID_ARGUMENT` + `detail.field = "location"`；不再猜 local。
6. **但 `remove` 永远可用**（Q33）。删除只按 id 抹记录、不碰磁盘（`removal_keeps_the_project_directory_and_every_other_record`），因此不需要知道项目在哪。若连删除都拒绝，一条坏记录就成了只能手改数据文件才能清掉的钉子，而 P2-02 的门禁恰恰会拒绝覆盖那样一个手改坏的文件。
