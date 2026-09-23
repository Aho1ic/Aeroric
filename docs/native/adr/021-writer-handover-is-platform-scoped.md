# ADR-021：writer 角色的移交分平台推进——P2-05 只在 macOS 上切

- 状态：已接受（Task P2-05 门禁，Q47）
- 日期：2026-09-23
- 相关：ADR-010（writer 资格由位置决定）、ADR-015（覆盖基线）、INV-03（一份数据目录一个业务 writer）、P7-01/P7-02（临时 adapter 的删除点）

## 背景

计划把 P2-05 定为 writer 角色的**原子切换**点。实现前置①时出现一个新事实：Windows 的权限回执改成复用 `ensure_private_dir` 已经在调的那次 `restrict_to_current_user` 之后（见 `storage_location.rs` 与 `storage.rs::windows_private_acl_receipt`），在正常 NTFS 安装上回执会**成功**。也就是说，Windows 上的 core 不再像 P2-02 门禁时假设的那样"拿不到 writer 资格"——`writer_eligible()` 会返回 `Ok`。

于是"原子切换"这个词留下了一个歧义：一次切换是指**所有平台同时**，还是指**每个安装实例一次**？

## 决策

1. **P2-05 只在 macOS 上递交 writer 角色**。Windows/Linux 上 core 是只读组件，旧客户端继续独自写；这两侧的切换推到 P7-01/P7-02（临时 adapter 删除点）一并验收。
2. **平台判断属于宿主策略，不进 core**。`StorageLocation::writer_eligible()` 继续只回答"这个位置配不配"，`PlatformLocationProbe` 继续如实回答卷型与回执。"我这次要不要接这个角色"由 `src-tauri` 的 adapter 决定。core 里不出现 `#[cfg(target_os)]` 的平台政治。
3. **不写"Windows 只读"的假设进任何断言**。Windows 侧的探针照旧接、照旧给回执，只是暂不启用其结果。这样 P7 切换时改的是"要不要接"这一处，而不是先回头补探针。

## 后果

- 避免的风险是具体的：Windows/Linux 上我跑不了任何一条产品测试（本机只有 macOS 可执行环境，另两侧只做到 `cargo check --target` 编译验证），而 INV-03 的破坏方式是**静默覆盖用户数据**。分平台推进让"切换"这件事始终在有真实回归覆盖的平台上发生。
- 代价是过渡期存在两种宿主形态："macOS 上 core 是 writer、其余平台 core 是 reader"。因此这条差异必须写成一条机械检查（`tauri-adapter` 门禁模块里断言"非 macOS 构建不得递交 writer 角色"），否则它会以"没人记得为什么"的形式扩散。
- 回执在 Windows 上会成功这件事本身不是问题，也不是"已经可以切"的许可证：它只说明权限那一半不再是障碍。卷型、锁、指纹基线这些仍要在真正的 Windows 机器上跑过一遍才算。
