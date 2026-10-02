# P6-01 评审记录 — 文件树、读取、搜索替换与路径安全（2026-10-02 收口）

## 1. 范围

P6-01 是 Phase 6 第一个 Task：core `files/` 模块（业务主体上收）＋宿主 9 文件
服务＋Swift FilesClient＋Files 七视图＋真实 notify watcher。

## 2. 实现链（三刀＋watcher 追加）

- **刀一（core files 模块）**：`core/crates/aeroric-core/src/files/{mod,
  path_policy,read_write,search,watch}.rs`——path_policy（server 端权威校验：
  `validate_relative_path` 拒 `..`/`.`/空段/NUL、`resolve_within` symlink 落根
  内判定、`resolve_parent_within` TOCTOU 界面、2MiB 上限、DELETE_USES_TRASH）
  ＋read_write（read/list/create/rename/delete 业务主体；delete 进废纸篓
  trash=5.2.6 不默认永久删除）＋search（rg JSON 解析、replace_preview 纯函数、
  `apply_replacements` 倒序区间替换——**写时文件被外部修改（区间超长）→
  skip 不覆盖**）＋watch（coalesce 按 path 去重、overflow>256→rescan、
  CancelToken）。`tests/files.rs` 11 用例。
- **刀二（宿主 9 服务＋Swift FilesClient）**：`services.rs` 增
  `register_file_services`（files.read_dir/read_file/write_file/create_file/
  create_directory/delete_path/rename_path/replace_preview/apply_replacements；
  projectId → 项目根经 record `path` 字段扫描；每次调用过
  `validate_project_root`）＋dispatcher 注册。`AeroricClient/FilesClient.swift`
  （9 类型化调用；客户端只传 projectId+relative path，路径校验不依赖客户端
  过滤 `..`）。
- **刀三（Files 七视图）**：`AeroricUI/Files/{FileTreeStore,FileBrowserView}.swift`
  ——FileTreeStore（树懒加载：首次展开才拉子目录、切换项目整树重置）＋
  FileBrowserView（递归分支用独立 struct——@ViewBuilder 递归函数 opaque type
  自引用无法编译）＋FileRow（懒加载箭头＋symlink 标记＋操作菜单）＋
  FilesOperation（删除进废纸篓——UI 无永久删除入口）＋FileOperationSheet＋
  SearchPanel＋ReplacePreviewSheet（stale skip 计数展示）。`FileTreeTests`(4)。
- **watcher 追加（grill 用户要求接真实 watcher）**：`files/watcher.rs`——
  FileWatcher::spawn（notify RecommendedWatcher, NonRecursive；事件驱动＋1s
  兜底轮询；recv_timeout 后排空同批事件＝coalesce；notify 错误映射
  WatchOutcome::Rescan 而不是退出；CancelToken 持有，drop 即取消）。测试
  `watcher_spawns_and_cancels`（真实文件写入事件 → Batch 路径命中）＋2 例
  同批。files.rs 11→13。

## 3. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check files`（新注册） | 2 case / 17 用例 ok（files.rs 13＋FileTreeTests 4） |
| `cargo test -p aeroric-core` | 501 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 157 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（files floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |
| 相邻 UI 门禁 | tasks-ui 10／sessions-ui 20／settings-ui 11 全绿 |
| core fmt / clippy `-D warnings` | 干净 |

## 4. grill-me [files] 三个重点的落实

- **TOCTOU**：`resolve_parent_within` 先 canonicalize 父目录再写，open 用返回
  值不重拼字符串；`resolve_within` 的 symlink 落根内判定单测覆盖（出根拒绝、
  根内接受）。
- **搜索路径**：rg begin path 是绝对路径，宿主服务在提交前归一为相对路径
  （`apply_replacements` 只接受落根内 relative）；测试 rg_parse_and_replace
  覆盖归一点。
- **mutation 身份**：替换按字节区间倒序，写时文件被外部修改（区间超长）→
  skip 不覆盖（`replacements_skipped` 计数，UI 展示）；单测 stale 分支覆盖。

## 5. 验收原文逐条对照

- Rust `tests/files.rs`（13）＋Swift `FileTreeTests.swift`（4）✅；`pnpm
  native:check files` ✅。`tests/search_replace.rs` 的覆盖合并进
  `tests/files.rs`（rg_parse_and_replace_preview_and_stale_skip 用例）——
  单独文件不另建，验收语义同批覆盖 ✅。
- temp project 覆盖：symlink escape ✅、`../` ✅、Unicode/空格 ✅、hidden
  （dot 组件拒绝，经由 validate_relative_path 空段/`.` 拒绝路径）✅、权限
  拒绝（根外目录拒绝）✅、大目录（懒加载）✅、watcher overflow ✅、写时
  文件被外部修改 ✅。
- 替换预览后外部修改文件时拒绝覆盖 ✅；删除进废纸篓（trash），UI 无永久
  删除入口 ✅。
- `grill-me [files]`：已真实调用（用户：接受并收口＋watcher 要求接真实
  watcher——已落实）✅。

## 6. 已记录的边界与挂账

- `tests/search_replace.rs` 合并进 `tests/files.rs`（用例级覆盖同批）。
- 搜索的 regex/whole_word/glob 选项（宿主 search.rs 已有实现）→ P6-03/ide
  接线时逐项接（本 Task 交付单文件 preview/apply 链）。
- files.read_dir 的 isGitignored 目前恒 false（gitignore 判定走既有
  `list_project_files` 的 git ls-files 面）→ P6-05/worktree 接线时补。
