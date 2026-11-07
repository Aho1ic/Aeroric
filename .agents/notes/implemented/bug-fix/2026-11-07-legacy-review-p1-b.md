# Agent Note: 回溯评审 P1 B 批——编辑器草稿不丢、改名名字只一段、Git 快照按窗口和项目隔离

Status: implemented

## Problem

2026-11-06 回溯评审在中央文件标签和 Git 面板里找到六个问题：

- **B4-20**：`markSaved` 把草稿整个改写成发去保存的文本。保存要等宿主返回，期间新敲的字被抹掉。
- **B4-19**：保存前用 `try?` 重读磁盘，读失败得到 nil，冲突检查被跳过，直接写入。
- **B4-21**：移到废纸篓时，路径下的标签全部关闭，`close` 同时删掉草稿，未保存的修改无声丢失。
- **B4-22**：宿主改名只取新名字的末段，`a/b` 在磁盘上成了 `b`，界面标签却跟到 `a/b`。
- **B4-35**：改名目标路径上已有打开的标签时（文件已被外部删掉或是已删除的脏标签），`rekey` 用 `uniquingKeysWith: first` 合并，出现两个同路径标签，丢一份草稿。
- **B4-24**：`AppServices.gitStore` 全局一份。两个窗口看两个项目时快照互相覆盖，可能对 X 项目执行按 Y 快照算出的放弃文件或删除贮藏；同一窗口切项目、旧读取晚到时也会错位。

## Decision

- **B4-20**：`markSaved` 只推进基线（`draftBases`），草稿保持现状；草稿等于基线才算干净。
- **B4-19**：新增 `ProjectResourceBook.saveCheck`，返回 `unchanged`、`changed`、`missing` 或 `failed`。读失败时列一次父目录：
  - 父目录里没有这个名字 → `missing`，弹“文件已被删除”（取消 / 重新创建并保存）。
  - 其他情况 → `failed`，中止保存并提示“保存前读取失败：<原因>”。
  - 不按错误字符串判断“不存在”：本地和远程宿主的报错文本不同，列目录对两者都成立。
- **B4-21**：废纸篓之后，干净标签照常关闭；脏的 `.file` 标签留下，记入 `scene.deleted`，标签名后加“已删除”，预览顶部提示“已在磁盘上删除 · 保存会重新创建文件”。
  - `loadPreview` 遇到已删除标签时不读宿主，以基线为 `previewText`，状态为 `loaded`，编辑器照常显示草稿。
  - 保存走 B4-19 的 `missing` 分支，写成功后清除标记并刷新父目录；关闭标签则按原有的未保存确认丢弃。
- **B4-22**：宿主 `rename_path` 拒绝含 `/` 的新名（`RENAME_NAME_HAS_SLASH`，“名称不能包含 /”）；界面提交前用 `FileEditText.nameProblem` 拦下，提示相同。
- **B4-35**：改名前调用 `ProjectResourceBook.prepareRename`。目标路径（含其子路径）上的干净标签先关闭；有脏标签就拒绝，提示“「X」有未保存的修改，先保存或关闭它”。改名本身之外的 `rekey` 不变，`deleted` 集合也随改名移动。
- **B4-24**：
  - GitStore 改为每窗口一份，`TaskWorkspaceView` 用 `@State` 持有。`refreshGit`、`performGit`、`generateCommitMessage` 都显式接收 store。
  - 快照记录 `projectPath`。`beginRead(projectPath:)` 切到另一个项目时立即清空旧快照，并返回读取 ID；后到的旧读取不再写入。
  - `performGit` 在快照所属项目与要写的项目不一致时不执行，先重读，再显示“Git 状态已变化，已重新读取”。检查覆盖所有写操作，不只依赖快照的那几种：规则更简单，代价只是一次重读。

## Alternatives considered

- **B4-20 保存期间把编辑器设为只读**：打断输入，体验更差。
- **B4-21 确认框里警告会丢草稿 / 有脏标签时禁止移到废纸篓**：前者仍然丢数据，后者拦住了正当操作。留下脏标签和编辑器处理被删文件的常见做法一致。
- **B4-22 支持用 `a/b` 移动到子目录**：重命名的语义是改名字，移动另作功能。
- **B4-35 两份草稿都保留、给其中一份改名**：规则复杂，用户还要分辨哪份是哪份。
- **B4-24 只加 `projectPath` 检查、仍全局共用**：不会写错对象，但两个窗口会互相刷掉对方的显示。

## Consequences

- 已删除的脏标签不会自动消失，只能保存（重新创建）或关闭。
- 整个文件夹被移到废纸篓后，里面的脏标签保存时父目录不存在，会走 `failed` 分支，提示读取失败，不会悄悄在别处创建文件。
- 每个窗口各自读取 Git，第二个窗口打开 Git 面板时会多一次读取。

## Verification

- Rust：`a_new_name_with_a_slash_is_refused`。
- Swift：
  - `testTextTypedDuringASaveStaysAndKeepsTheTabDirty`
  - `testTheTrashKeepsADirtyTabAndClosesCleanOnes`
  - `testARenameOntoAnOpenTabClosesItOrIsRefused`
  - `testTheSaveCheckNeverTreatsAFailedReadAsUnchanged`
  - `testARenameNameIsOneComponent`
  - `testASnapshotBelongsToOneProject`
- 废纸篓后脏标签留存、重新创建保存、含斜杠改名被拦，靠 Codex Computer Use 目检验证。
