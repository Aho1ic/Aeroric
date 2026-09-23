# ADR-014：core 的内容指纹使用 `sha2`，不自行实现摘要

- 状态：已接受（Task P2-04 门禁 Q35）
- 日期：2026-09-22
- 相关：ADR-012（`runtime.json` 是丢弃品、内容指纹才是真相）、ADR-011（`staleWrite → REVISION_CONFLICT`）、P2-02 的"零新增依赖"倾向

## 背景

ADR-012 把"旧客户端还在写同一个数据目录"这件事交给内容指纹来发现：每次写记录它覆盖掉的 `(sha256, size)`，下一次写只有在这份字节仍在原位时才允许覆盖。P2-04 落地时遇到一个实现选择：`sha2` crate，还是在 core 里手写约 70 行 SHA-256。

两边都有真实代价：

- `sha2`：core 的 `Cargo.toml` 多 8 个锁定条目（sha2 / digest / crypto-common / generic-array / typenum / cpufeatures / cfg-if / version_check）。更关键的是它会**穿过 `--locked` 传播**：`src-tauri` 以 path 依赖引 core，加完依赖后 `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib` 直接失败（"cannot update the lock file ... because --locked was passed"），必须同步更新产品的 `Cargo.lock` 才能让 P2-02/P2-05 那条门禁重新变绿。
- 手写：锁文件一字不动，回到 P2-02 "用 std 的 `File::try_lock` 而不是引 lock crate" 的方向。代价是这段代码成为我们自己维护的、位于安全边界上的实现（`tasks/handoff.rs` 剥离转义序列是同一类边界）。

## 决策

**使用 `sha2`，并同步更新 `src-tauri/Cargo.lock`。**

支撑这个选择的三个事实（都是实测，不是推断）：

1. **它不是新的第三方依赖。** `sha2 0.10.9` 已经出现在今天发货的 `src-tauri/Cargo.lock:7774`（另有 0.11.0），产品的二进制里已经有这段代码。这次改动的真实内容是把 core 的依赖图对齐到产品的依赖图，而不是引入新供给链。
2. **离线可解析。** `cargo add --offline`、`cargo check --offline --locked`、更新后的 `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib`（2412 passed）全程未触网，符合"不静默安装/不静默联网"的约束。
3. **可逆性极高，所以不必为它预付复杂度。** 摘要只在 `Fingerprint::of(bytes)` 一处被调用，两种方案产出的都是同一串 64 位小写 hex——事后换成手写实现不会改变磁盘上任何已存的指纹，也不会让任何 parity 期望重算。既然反悔几乎免费，就不该为"万一要反悔"现在就自己实现并测试一个标准算法。

## 后果

- `core/Cargo.lock` 与 `src-tauri/Cargo.lock` 各多一次提交；发布前的锁一致性检查（`scripts/native/check-patches.mjs`、`--locked` 门禁）从"没变"变成"变过且被审过"。
- P2-02 §"零新增依赖"作为**倾向**继续有效，但不再是硬约束：以后再有类似选择时，先问"它是否已经在产品的图里"——在，就不算新依赖；不在，才需要像这次这样单独记一条。
- 指纹的语义（基线是什么、什么时候允许第一次覆盖）不在本 ADR 内，见 ADR-015。
