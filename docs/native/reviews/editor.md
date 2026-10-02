# P6-02 评审记录 — 原生文本/Markdown 编辑、预览和本地历史（2026-10-02 收口）

## 1. 范围

P6-02 是 Phase 6 第二个 Task：core format/local_history/preview ＋ Editor 八
视图＋XCTest UI 表面。

## 2. 实现链（两刀）

- **刀一（core format/local_history/preview）**：`core/crates/aeroric-core/src/
  files/{format,local_history,preview}.rs`——format（formatter 命令发现与
  Tauri 同源 verbatim：prettier/rustfmt/gofmt/ruff；`format_file` 快照先记＋
  失败原文不变——非零退出即拒绝）＋local_history（快照记录/列出/读取/恢复/
  丢弃：CODE_LAYOUT 不限流、**恢复前兜底快照可撤回**、上限从最旧删、目录
  不存在＝空列表不是错误）＋preview（PreviewDecision 纯判定：Text/Image/
  ReadOnly 有原因、`is_allowed_link_scheme` 拒 javascript:/data:/file:）。
  `tests/file_history.rs` 7 用例。
- **刀二（Editor 八视图）**：`AeroricUI/Editor/{DocumentStore,NativeTextEditor,
  EditorCoordinator}.swift`——DocumentStore（dirty/revision/冲突簿记：
  autosave 版本冲突不可静默覆盖、BOM/CRLF 保留、关 tab 返回 dirty 由调用方
  提示）＋NativeTextEditor（**真实 NSTextView/TextKit 桥**：usesFindBar/
  incrementalSearching/allowsUndo/行号 ruler 挂 enclosingScrollView/CJK IME
  由 TextKit 自身保证——不用基本 TextEditor 冒充）＋MarkdownPreview（原生
  Text 明文渲染——永不走 HTML）＋ImagePreview（ReadOnly 有原因＋显式外部
  打开提示）＋EditorCoordinator（SaveOutcome 路由）＋EditorTabs＋
  SaveConflictSheet（覆盖/另存为/放弃/取消四选）＋LocalHistoryPanel（预览/
  恢复确认——恢复前兜底快照）。`DocumentStoreTests`(7)；XCTest UI
  `EditorTests.swift`(1 例，pbxproj 登记)。

## 3. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check editor`（新注册） | 2 case / 14 用例 ok（file_history 7＋DocumentStoreTests 7） |
| `cargo test -p aeroric-core` | 508 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 164 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（editor floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |

## 4. 验收原文逐条对照

- Rust `tests/file_history.rs`(7)＋Swift `DocumentStoreTests.swift`(7)＋
  XCTest UI `EditorTests.swift`(1) ✅；`pnpm native:check editor` ✅。
- UTF-8 BOM（保留不剥）✅、CRLF（保留不归一）✅、emoji UTF-16 selection
  （TextKit 自身处理）✅、外部修改（SaveConflictSheet 四选，不可静默覆盖）✅、
  Undo/Redo（NSTextView allowsUndo）✅、重复 save（revision 递增，幂等由
  磁盘内容保证）✅、退出取消不丢改动（close 返回 dirty 由调用方提示）✅；
  格式化失败原文不变（formatter 非零退出即拒绝，单测覆盖）✅。
- Markdown 恶意 HTML/`javascript:` link/本地文件探测不能执行或泄露（原生
  Text 明文渲染＋scheme 守卫单测）✅；local history restore 有预览/确认
  （LocalHistoryPanel 确认步）且可撤回（恢复前兜底快照，单测覆盖）✅。
- `grill-me [editor]`：已真实调用（用户：接受并收口）✅。

## 5. 已记录的边界与挂账

- 行号 ruler 挂 enclosingScrollView——无滚动视图时由宿主容器挂（makeNSView
  里 scrollView 为 nil 时不挂，用户已确认记账）。
- Markdown 复杂内容（表格/公式/图表）按 feature-matrix 单独验收——本 Task
  交付行内四类（标题/粗体/行内代码/链接），用户已确认记账。
- TextKit 桥的 CJK IME/Undo/emoji 选区真机交互 QA 随 P10-02。
- autosave（定时保存）未接——DocumentStore 已有 revision/冲突簿记，autosave
  调度随 P6-03/ide 或宿主接线逐项接。
