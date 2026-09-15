pub(super) const SOURCE_COMMIT: &str = "c291e7961a515f6d7af9304e7fd1d257929aef26";
pub(super) const PACKAGE_VERSION: &str = "0.1.5-rc.2";
pub(super) const PROTOCOL_VERSION: u32 = 2;

pub(super) const RPC_METHODS: &[&str] = &[
    "session.list",
    "session.search",
    "session.create",
    "session.selectModel",
    "session.modelCatalog",
    "session.canOpenWorkspacePath",
    "session.openWorkspacePath",
    "session.rename",
    "session.fork",
    "session.prompt",
    "session.attachment",
    "session.updateQueue",
    "session.cancel",
    "session.page",
    "session.follow",
    "session.control",
    "subagents.list",
    "subagents.prompt",
    "subagents.interruptByParent",
    "directoryPicker.pick",
    "directoryPicker.list",
    "directoryPicker.createDirectory",
    "workspace.create",
    "workspace.rename",
    "workspace.delete",
    "workspace.insertBefore",
    "workspace.insertSessionBefore",
    "workspace.archiveSession",
    "workspace.follow",
    "workspaceFiles.list",
    "workspaceFiles.read",
    "workspaceFiles.readAll",
    "workspaceFiles.readBytes",
    "workspaceFiles.readRelated",
    "workspaceFiles.stat",
    "workspaceFiles.changes",
    "skills.list",
    "agentPresets.list",
    "agentPresets.select",
    "agentPresets.read",
    "agentPresets.copy",
    "agentPresets.deletePreset",
    "goals.create",
    "goals.edit",
    "goals.pause",
    "goals.resume",
    "goals.complete",
    "goals.clear",
    "goals.get",
    "settings.describe",
    "settings.openSettingsDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "settings.canOpenAgentPresetDirectory",
    "settings.openAgentPresetDirectory",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.listProviders",
    "llm.listConfigurableProviders",
    "llm.discoverModels",
];

pub(super) const REMOTE_METHODS: &[&str] = &[
    "session.list",
    "session.search",
    "session.create",
    "session.selectModel",
    "session.modelCatalog",
    "session.canOpenWorkspacePath",
    "session.openWorkspacePath",
    "session.rename",
    "session.fork",
    "session.prompt",
    "session.attachment",
    "session.updateQueue",
    "session.cancel",
    "session.page",
    "session.follow",
    "session.control",
    "subagents.list",
    "subagents.prompt",
    "subagents.interruptByParent",
    "directoryPicker.pick",
    "directoryPicker.list",
    "directoryPicker.createDirectory",
    "workspace.create",
    "workspace.rename",
    "workspace.delete",
    "workspace.insertBefore",
    "workspace.insertSessionBefore",
    "workspace.archiveSession",
    "workspace.follow",
    "workspaceFiles.list",
    "workspaceFiles.read",
    "workspaceFiles.readAll",
    "workspaceFiles.readBytes",
    "workspaceFiles.readRelated",
    "workspaceFiles.stat",
    "workspaceFiles.changes",
    "skills.list",
    "agentPresets.list",
    "agentPresets.select",
    "agentPresets.read",
    "agentPresets.copy",
    "agentPresets.deletePreset",
    "goals.create",
    "goals.edit",
    "goals.pause",
    "goals.resume",
    "goals.complete",
    "goals.clear",
    "goals.get",
    "settings.describe",
    "settings.openSettingsDocument",
    "settings.update",
    "settings.replace",
    "settings.mutate",
    "settings.canOpenAgentPresetDirectory",
    "settings.openAgentPresetDirectory",
    "credentials.describe",
    "credentials.set",
    "credentials.unset",
    "llm.listProviders",
    "llm.listConfigurableProviders",
    "llm.discoverModels",
    "commands.list",
    "commands.execute",
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
    "fileUploads.upload",
    "sessionFeedback.record",
    "agentTeams.createTask",
    "agentTeams.updateTask",
    "agentTeams.view",
];

/// Event names, 1:1 with `REMOTE_EVENT_MODES`. Source:
/// `packages/api/remotes/src/remote-events.ts` at SOURCE_COMMIT.
pub(super) const REMOTE_EVENTS: &[&str] = &[
    "agent-preset/selected",
    "approval/request",
    "api-session/activity",
    "api-session/added",
    "api-session/error",
    "api-session/removed",
    "api-session/status",
    "commands/change",
    "credentials/reference-updated",
    "goal/activation-changed",
    "cordis/request-run",
    "cordis/request-run-resolved",
    "cordis/dynamic-package",
    "cordis/dynamic-retract",
    "cordis/inspect-query",
    "cordis/inspect-query-resolved",
    "llm/adapters-updated",
    "settings/document-updated",
    "user-questions/request",
];

pub(super) const REMOTE_EVENT_MODES: &[&str] = &[
    "emit",
    "waterfall",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "emit",
    "waterfall",
];

pub(super) const STREAM_FOLLOW_FRAMES: &[&str] = &["snapshot", "event", "assistant-stream"];

pub(super) const STREAM_CONTROL_FRAMES: &[&str] = &["baseline", "queue", "jobs", "projection"];

pub(super) const STREAM_DOWNLINK_FRAMES: &[&str] = &["ready", "emit", "waterfall", "cancel"];

/// Frames of the retired `/api/events.mux` + `/api/events.host` firehose,
/// removed from the live inventory with DSH-14: the downlink is now the
/// bidirectional `/api/remote.mux` WebSocket, whose vocabulary is the
/// `STREAM_*` lists above. Kept test-only so the snapshot cannot silently
/// re-admit a name that no longer exists on any wire.
#[cfg(test)]
pub(super) const RETIRED_MUX_FRAMES: &[&str] = &[
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

/// Second half of the retired firehose (`/api/events.host`). See
/// `RETIRED_MUX_FRAMES`.
#[cfg(test)]
pub(super) const RETIRED_HOST_FRAMES: &[&str] = &[
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

/// Methods retired with the apiproxy unary domain (ce3391e280). Kept so the
/// inventory tests can assert the snapshot does not silently re-admit them.
///
/// Test-only in Rust: nothing at runtime dispatches on a retired name. The TS
/// parity test reads this slice out of the source text by regex, so the `cfg`
/// does not hide it from `src/test/dsh-protocol-snapshot.test.ts`.
#[cfg(test)]
pub(super) const RETIRED_APIPROXY_METHODS: &[&str] = &[
    "session.history",
    "session.models",
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
    "settings.openDocument",
    "llm.providers",
    "llm.models",
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

    /// 帧名一律小写 kebab。`group/name` 形式的两段各自也要成立。大写或下划线说明
    /// 有人按 Rust/TS 的命名习惯改写了一个 wire 名字,那就再也匹配不上真实帧。
    fn assert_kebab_frame(label: &str, entry: &str) {
        for segment in entry.split('/') {
            let charset_ok = segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
            assert!(
                !segment.is_empty()
                    && !segment.starts_with('-')
                    && !segment.ends_with('-')
                    && charset_ok,
                "{label} entry {entry:?} is not lowercase kebab-case"
            );
        }
    }

    /// 活的清单:每一份都会经 `DshProtocolCapabilities::snapshot()` 报给前端。
    fn all_lists() -> [(&'static str, &'static [&'static str]); 7] {
        [
            ("RPC_METHODS", RPC_METHODS),
            ("REMOTE_METHODS", REMOTE_METHODS),
            ("REMOTE_EVENTS", REMOTE_EVENTS),
            ("REMOTE_EVENT_MODES", REMOTE_EVENT_MODES),
            ("STREAM_FOLLOW_FRAMES", STREAM_FOLLOW_FRAMES),
            ("STREAM_CONTROL_FRAMES", STREAM_CONTROL_FRAMES),
            ("STREAM_DOWNLINK_FRAMES", STREAM_DOWNLINK_FRAMES),
        ]
    }

    /// 退役清单:只在测试里当"不许回归"的黑名单,`snapshot()` 不碰它们。
    fn retired_lists() -> [(&'static str, &'static [&'static str]); 3] {
        [
            ("RETIRED_APIPROXY_METHODS", RETIRED_APIPROXY_METHODS),
            ("RETIRED_MUX_FRAMES", RETIRED_MUX_FRAMES),
            ("RETIRED_HOST_FRAMES", RETIRED_HOST_FRAMES),
        ]
    }

    /// 活的下行帧词表(`STREAM_*` 三份合起来)。退役名字不许出现在里面。
    fn live_stream_frames() -> HashSet<&'static str> {
        STREAM_FOLLOW_FRAMES
            .iter()
            .chain(STREAM_CONTROL_FRAMES)
            .chain(STREAM_DOWNLINK_FRAMES)
            .copied()
            .collect()
    }

    /// 唯一性只适用于"名字"清单。`REMOTE_EVENT_MODES` 是与 `REMOTE_EVENTS` 平行的
    /// 取值列,19 条里 17 条都是 `emit` —— 重复是它的正常形态,不是粘贴事故。
    #[test]
    fn no_inventory_list_repeats_an_entry() {
        for (label, entries) in all_lists() {
            if label == "REMOTE_EVENT_MODES" {
                continue;
            }
            assert_unique(label, entries);
        }
        for (label, entries) in retired_lists() {
            assert_unique(label, entries);
        }
    }

    #[test]
    fn no_inventory_list_is_empty_or_truncated() {
        // 清单被截断(比如粘贴时丢了尾部)不会让任何断言失败,只会让诊断静默漏报,
        // 所以这里给一个下界。数字是当前长度的保守下限,不是精确值 —— 上游加方法时
        // 不该来改这里,删到只剩几条时才应该有人来解释为什么。
        for (label, entries) in all_lists().into_iter().chain(retired_lists()) {
            assert!(
                entries.len() >= 3,
                "{label} only has {} entries; a list this short is more likely truncated than real",
                entries.len()
            );
        }
        assert!(RPC_METHODS.len() >= 40, "got {}", RPC_METHODS.len());
        assert!(REMOTE_METHODS.len() >= 60, "got {}", REMOTE_METHODS.len());
        assert!(REMOTE_EVENTS.len() >= 15, "got {}", REMOTE_EVENTS.len());
    }

    #[test]
    fn remote_event_names_and_modes_stay_aligned() {
        assert_eq!(REMOTE_EVENTS.len(), REMOTE_EVENT_MODES.len());
        for mode in REMOTE_EVENT_MODES {
            assert!(
                *mode == "emit" || *mode == "waterfall",
                "unknown remote-event mode {mode:?}"
            );
        }
        let waterfall: Vec<&str> = REMOTE_EVENTS
            .iter()
            .zip(REMOTE_EVENT_MODES)
            .filter_map(|(event, mode)| (*mode == "waterfall").then_some(*event))
            .collect();
        assert_eq!(
            waterfall,
            vec!["approval/request", "user-questions/request"]
        );
    }

    #[test]
    fn methods_and_frames_keep_their_own_naming_shape() {
        // 方法名是 `namespace.method`,事件/帧名是 `group/name`。把一条粘到错误的清单里
        // 是最容易犯、也最难看出来的错(两侧镜像都抄同一个错字符串,前端 parity 测试
        // 照样通过),形状检查能当场拦住。
        for (label, entries) in [
            ("RPC_METHODS", RPC_METHODS),
            ("REMOTE_METHODS", REMOTE_METHODS),
            ("RETIRED_APIPROXY_METHODS", RETIRED_APIPROXY_METHODS),
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
            ("RETIRED_MUX_FRAMES", RETIRED_MUX_FRAMES),
            ("RETIRED_HOST_FRAMES", RETIRED_HOST_FRAMES),
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
                assert_kebab_frame(label, entry);
            }
        }
        // `/api/remote.mux` 的帧名没有 group 前缀(`snapshot`、`assistant-stream`、
        // `baseline`…)。带上 `/` 或 `.` 说明有人把旧 firehose 的名字、或某个 RPC
        // 方法名塞进了流词表。
        for (label, entries) in [
            ("STREAM_FOLLOW_FRAMES", STREAM_FOLLOW_FRAMES),
            ("STREAM_CONTROL_FRAMES", STREAM_CONTROL_FRAMES),
            ("STREAM_DOWNLINK_FRAMES", STREAM_DOWNLINK_FRAMES),
        ] {
            for entry in entries {
                assert!(
                    !entry.contains('/') && !entry.contains('.'),
                    "{label} entry {entry:?} must be a single unprefixed frame name"
                );
                assert_kebab_frame(label, entry);
            }
        }
    }

    #[test]
    fn no_entry_carries_stray_whitespace() {
        for (label, entries) in all_lists().into_iter().chain(retired_lists()) {
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
    fn the_retired_firehose_frames_stay_retired() {
        // DSH-14 把下行换成了 `/api/remote.mux`,`/api/events.mux` 与
        // `/api/events.host` 两条 firehose 一起退役。两条旧下行都会报
        // `stream/error`,那份重复是历史原样;除它以外的重合说明两份退役清单被抄串了。
        let mux: HashSet<&str> = RETIRED_MUX_FRAMES.iter().copied().collect();
        let host: HashSet<&str> = RETIRED_HOST_FRAMES.iter().copied().collect();
        let mut shared: Vec<&str> = mux.intersection(&host).copied().collect();
        shared.sort_unstable();
        assert_eq!(shared, vec!["stream/error"]);

        // 真正的回归线:退役名字一个都不许回到活的流词表里。它们在任何 wire 上都不
        // 再存在,重新出现只意味着有人把上一个 pin 的清单粘了回来。
        let live = live_stream_frames();
        for (label, entries) in [
            ("RETIRED_MUX_FRAMES", RETIRED_MUX_FRAMES),
            ("RETIRED_HOST_FRAMES", RETIRED_HOST_FRAMES),
        ] {
            for entry in entries {
                assert!(
                    !live.contains(entry),
                    "{label} entry {entry:?} came back into the live stream vocabulary"
                );
            }
        }
    }

    #[test]
    fn first_party_methods_are_a_prefix_of_the_remote_allowlist() {
        // rpcMethods 是 named wrapper 面;remoteMethods 是 invoke_dsh_remote 的
        // 逃生舱,必须覆盖前者,否则前端诊断会把"我们自己会调的方法"标成未知。
        assert!(
            REMOTE_METHODS.starts_with(RPC_METHODS),
            "REMOTE_METHODS must start with RPC_METHODS so the allowlist is a superset"
        );
    }

    #[test]
    fn retired_apiproxy_methods_are_gone() {
        let live: HashSet<&str> = RPC_METHODS
            .iter()
            .chain(REMOTE_METHODS.iter())
            .copied()
            .collect();
        for method in RETIRED_APIPROXY_METHODS {
            assert!(
                !live.contains(method),
                "{method} was retired with apiproxy and must not re-enter the snapshot"
            );
        }
        for method in [
            "session.page",
            "session.follow",
            "session.control",
            "session.modelCatalog",
            "subagents.interruptByParent",
            "directoryPicker.pick",
            "workspaceFiles.list",
            "fileUploads.upload",
            "sessionFeedback.record",
        ] {
            assert!(
                live.contains(method),
                "{method} is a 0.1.5 Typert remote and must appear in the snapshot"
            );
        }
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
        assert_eq!(PACKAGE_VERSION, "0.1.5-rc.2");
    }

    #[test]
    fn the_capability_snapshot_hands_each_list_to_its_own_field() {
        // `snapshot()` 是几行几乎一样的赋值,接错一行(比如 session_follow 接到
        // STREAM_CONTROL_FRAMES)编译照过、前端 parity 测试也照过 —— 它读的是这里的
        // 常量,不是 snapshot()。
        let snapshot = super::super::DshProtocolCapabilities::snapshot();
        assert_eq!(snapshot.source_commit, SOURCE_COMMIT);
        assert_eq!(snapshot.package_version, PACKAGE_VERSION);
        assert_eq!(snapshot.protocol_version, PROTOCOL_VERSION);
        assert_eq!(snapshot.rpc_methods, RPC_METHODS);
        assert_eq!(snapshot.remote_methods, REMOTE_METHODS);
        assert_eq!(
            snapshot
                .remote_events
                .iter()
                .map(|entry| entry.event)
                .collect::<Vec<_>>(),
            REMOTE_EVENTS
        );
        assert_eq!(
            snapshot
                .remote_events
                .iter()
                .map(|entry| entry.mode)
                .collect::<Vec<_>>(),
            REMOTE_EVENT_MODES
        );
        // 退役的 mux/host 清单不该在 snapshot 上留下任何字段:见
        // `the_capability_snapshot_serializes_the_keys_the_frontend_reads`。
        assert_eq!(snapshot.stream_frames.session_follow, STREAM_FOLLOW_FRAMES);
        assert_eq!(
            snapshot.stream_frames.session_control,
            STREAM_CONTROL_FRAMES
        );
        assert_eq!(
            snapshot.stream_frames.remote_downlink,
            STREAM_DOWNLINK_FRAMES
        );
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
                "packageVersion",
                "protocolVersion",
                "remoteEvents",
                "remoteMethods",
                "rpcMethods",
                "sourceCommit",
                "streamFrames",
            ]
        );
        assert_eq!(
            object["rpcMethods"].as_array().map(Vec::len),
            Some(RPC_METHODS.len())
        );
        assert_eq!(
            object["remoteEvents"].as_array().map(Vec::len),
            Some(REMOTE_EVENTS.len())
        );
        assert_eq!(
            object["remoteEvents"][1]["event"].as_str(),
            Some("approval/request")
        );
        assert_eq!(
            object["remoteEvents"][1]["mode"].as_str(),
            Some("waterfall")
        );
    }
}
