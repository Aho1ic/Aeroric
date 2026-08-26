pub(super) const SOURCE_COMMIT: &str = "b150a551b8d465e31e418e1b2eaf5e79bbb7d28e";
pub(super) const PACKAGE_VERSION: &str = "0.1.1-rc.2";
pub(super) const PROTOCOL_VERSION: u32 = 2;

pub(super) const RPC_METHODS: &[&str] = &[
    "session.list",
    "session.search",
    "session.create",
    "session.history",
    "session.models",
    "session.selectModel",
    "session.rename",
    "session.fork",
    "session.prompt",
    "session.attachment",
    "session.updateQueue",
    "session.cancel",
    "subagent.list",
    "subagent.history",
    "subagent.prompt",
    "subagent.interrupt",
    "host.describe",
    "host.pickDirectory",
    "host.listDirectory",
    "host.createDirectory",
    "host.openPath",
    "workspace.list",
    "workspace.create",
    "workspace.rename",
    "workspace.delete",
    "workspace.insertBefore",
    "workspace.insertSessionBefore",
    "workspace.archiveSession",
    "skill.list",
    "agentPreset.list",
    "agentPreset.select",
    "agentPreset.read",
    "agentPreset.copy",
    "agentPreset.openDocument",
    "agentPreset.remove",
    "goal.create",
    "goal.edit",
    "goal.pause",
    "goal.resume",
    "goal.complete",
    "goal.clear",
    "settings.describe",
    "settings.openDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.providers",
    "llm.models",
    "llm.discoverModels",
];

pub(super) const REMOTE_METHODS: &[&str] = &[
    "commands.list",
    "commands.execute",
    "goals.create",
    "goals.edit",
    "goals.pause",
    "goals.resume",
    "goals.complete",
    "goals.clear",
    "messageFeedback.list",
    "messageFeedback.put",
    "messageFeedback.delete",
    "pluginInventory.list",
    "dynamicCordisRunner.undefineFromPanel",
    "dynamicCordisRunner.runHostHalf",
    "dynamicCordisRunner.getClientCode",
    "dynamicCordisRunner.resolveRequestRun",
    "dynamicCordisRunner.settleUserRun",
    "dynamicCordisRunner.stopFromPanel",
    "dynamicCordisRunner.syncInspectManifest",
    "dynamicCordisRunner.resolveInspectQuery",
    "dynamicCordisRunner.inventory",
    "dynamicCordisRunner.reportRenderFailure",
    "dynamicCordisRunner.reportClientGuardFailure",
    "dynamicCordisRunner.invoke",
    "fileReferences.list",
    "sessionReferenceResolver.candidates",
];

pub(super) const REMOTE_EVENTS: &[&str] = &[
    "agent-preset/selected",
    "commands/change",
    "credentials/reference-updated",
    "cordis/request-run",
    "cordis/request-run-resolved",
    "cordis/dynamic-package",
    "cordis/dynamic-retract",
    "cordis/inspect-query",
    "cordis/inspect-query-resolved",
    "llm/adapters-updated",
    "settings/document-updated",
];

pub(super) const MUX_FRAMES: &[&str] = &[
    "session/event",
    "session/subscribed",
    "approval/requested",
    "approval/resolved",
    "question/requested",
    "question/resolved",
    "session/queue",
    "session/jobs",
    "session/projection",
    "stream/error",
];

pub(super) const HOST_FRAMES: &[&str] = &[
    "host/session-added",
    "host/session-removed",
    "host/session-status",
    "host/agent-error",
    "host/workspace-changed",
    "host/workspace-removed",
    "host/workspace-order-changed",
    "host/archived-sessions-changed",
    "host/remote-event",
    "stream/error",
];

// ── 测试 ──────────────────────────────────────────────────────────────────────
//
// 这些清单是手写的、钉在某个 DSH 源码 commit 上的镜像,`src/dshProtocol.ts` 里还有
// 一份同样的镜像。前端的 `dsh-protocol-snapshot.test.ts` 只对比"两侧字符串是否一致",
// 所以两边同时抄错的错误它一个都看不见 —— 下面这些正好补那一段:清单自身的形状、
// 以及 `DshProtocolCapabilities::snapshot()` 有没有把每份清单接到对应的字段上。

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 一份清单里出现重复项,基本只有一种来路:往后面追加时没查有没有已经有了。
    /// 重复项会让前端的兼容性诊断把同一个方法列两次。
    fn assert_unique(label: &str, entries: &[&str]) {
        let mut seen: HashSet<&str> = HashSet::new();
        for entry in entries {
            assert!(seen.insert(entry), "{label} lists {entry:?} more than once");
        }
    }

    fn all_lists() -> [(&'static str, &'static [&'static str]); 5] {
        [
            ("RPC_METHODS", RPC_METHODS),
            ("REMOTE_METHODS", REMOTE_METHODS),
            ("REMOTE_EVENTS", REMOTE_EVENTS),
            ("MUX_FRAMES", MUX_FRAMES),
            ("HOST_FRAMES", HOST_FRAMES),
        ]
    }

    #[test]
    fn no_inventory_list_repeats_an_entry() {
        for (label, entries) in all_lists() {
            assert_unique(label, entries);
        }
    }

    #[test]
    fn no_inventory_list_is_empty_or_truncated() {
        // 清单被截断(比如粘贴时丢了尾部)不会让任何断言失败,只会让诊断静默漏报,
        // 所以这里给一个下界。数字是当前长度的保守下限,不是精确值 —— 上游加方法时
        // 不该来改这里,删到只剩几条时才应该有人来解释为什么。
        for (label, entries) in all_lists() {
            assert!(
                entries.len() >= 9,
                "{label} only has {} entries; a list this short is more likely truncated than real",
                entries.len()
            );
        }
        assert!(RPC_METHODS.len() >= 40, "got {}", RPC_METHODS.len());
        assert!(REMOTE_METHODS.len() >= 20, "got {}", REMOTE_METHODS.len());
    }

    #[test]
    fn methods_and_frames_keep_their_own_naming_shape() {
        // 方法名是 `namespace.method`,事件/帧名是 `group/name`。把一条粘到错误的清单里
        // 是最容易犯、也最难看出来的错(两侧镜像都抄同一个错字符串,前端 parity 测试
        // 照样通过),形状检查能当场拦住。
        for (label, entries) in [
            ("RPC_METHODS", RPC_METHODS),
            ("REMOTE_METHODS", REMOTE_METHODS),
        ] {
            for entry in entries {
                assert_eq!(
                    entry.matches('.').count(),
                    1,
                    "{label} entry {entry:?} is not a single `namespace.method`"
                );
                assert!(
                    !entry.contains('/'),
                    "{label} entry {entry:?} looks like an event/frame name"
                );
                let (namespace, method) = entry.split_once('.').expect("checked above");
                assert!(
                    !namespace.is_empty() && !method.is_empty(),
                    "{label} entry {entry:?} has an empty half"
                );
            }
        }
        for (label, entries) in [
            ("REMOTE_EVENTS", REMOTE_EVENTS),
            ("MUX_FRAMES", MUX_FRAMES),
            ("HOST_FRAMES", HOST_FRAMES),
        ] {
            for entry in entries {
                assert_eq!(
                    entry.matches('/').count(),
                    1,
                    "{label} entry {entry:?} is not a single `group/name`"
                );
                assert!(
                    !entry.contains('.'),
                    "{label} entry {entry:?} looks like an RPC method name"
                );
                let (group, name) = entry.split_once('/').expect("checked above");
                assert!(
                    !group.is_empty() && !name.is_empty(),
                    "{label} entry {entry:?} has an empty half"
                );
            }
        }
    }

    #[test]
    fn no_entry_carries_stray_whitespace() {
        for (label, entries) in all_lists() {
            for entry in entries {
                assert!(!entry.is_empty(), "{label} has an empty entry");
                assert_eq!(
                    *entry,
                    entry.trim(),
                    "{label} entry {entry:?} carries surrounding whitespace"
                );
            }
        }
    }

    #[test]
    fn stream_error_is_the_only_frame_both_downlinks_share() {
        // mux 与 host 两条下行都会报 `stream/error`,这个重复是故意的。除它以外的重合
        // 会让前端 `isDshMuxFrame` / `isDshHostFrame` 两个类型守卫同时认领一帧,路由
        // 就没有唯一答案了。
        let mux: HashSet<&str> = MUX_FRAMES.iter().copied().collect();
        let host: HashSet<&str> = HOST_FRAMES.iter().copied().collect();
        let mut shared: Vec<&str> = mux.intersection(&host).copied().collect();
        shared.sort_unstable();
        assert_eq!(shared, vec!["stream/error"]);
    }

    #[test]
    fn the_pin_identifies_one_resolvable_dsh_checkout() {
        // 这两个值是用户看到的兼容性诊断里唯一能拿去对源码的东西。缩写 sha 或占位串
        // 会让诊断变成"看着有、其实查不到"。
        assert_eq!(
            SOURCE_COMMIT.len(),
            40,
            "SOURCE_COMMIT must be a full 40-hex sha, got {SOURCE_COMMIT:?}"
        );
        assert!(
            SOURCE_COMMIT
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "SOURCE_COMMIT must be lowercase hex, got {SOURCE_COMMIT:?}"
        );
        assert!(!PACKAGE_VERSION.is_empty());
        assert!(
            PACKAGE_VERSION.starts_with(|c: char| c.is_ascii_digit()),
            "PACKAGE_VERSION should look like a version, got {PACKAGE_VERSION:?}"
        );
    }

    #[test]
    fn the_capability_snapshot_hands_each_list_to_its_own_field() {
        // `snapshot()` 是五行几乎一样的赋值,接错一行(比如 mux_frames 接到 HOST_FRAMES)
        // 编译照过、前端 parity 测试也照过 —— 它读的是这里的常量,不是 snapshot()。
        let snapshot = super::super::DshProtocolCapabilities::snapshot();
        assert_eq!(snapshot.source_commit, SOURCE_COMMIT);
        assert_eq!(snapshot.package_version, PACKAGE_VERSION);
        assert_eq!(snapshot.protocol_version, PROTOCOL_VERSION);
        assert_eq!(snapshot.rpc_methods, RPC_METHODS);
        assert_eq!(snapshot.remote_methods, REMOTE_METHODS);
        assert_eq!(snapshot.remote_events, REMOTE_EVENTS);
        assert_eq!(snapshot.mux_frames, MUX_FRAMES);
        assert_eq!(snapshot.host_frames, HOST_FRAMES);
    }

    #[test]
    fn the_capability_snapshot_serializes_the_keys_the_frontend_reads() {
        // 前端按 camelCase 读这些字段(src/dshProtocol.ts)。`rename_all` 一旦丢掉,
        // 后端照样编译、命令照样返回,只是每个字段在前端都变成 undefined。
        let value = serde_json::to_value(super::super::DshProtocolCapabilities::snapshot())
            .expect("capabilities are serializable");
        let object = value
            .as_object()
            .expect("capabilities serialize to an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "hostFrames",
                "muxFrames",
                "packageVersion",
                "protocolVersion",
                "remoteEvents",
                "remoteMethods",
                "rpcMethods",
                "sourceCommit",
            ]
        );
        assert_eq!(
            object["rpcMethods"].as_array().map(Vec::len),
            Some(RPC_METHODS.len())
        );
    }
}
