use aeroric_protocol::tools::{ToolGroup, ToolOwner, ToolScope, AERORIC_TOOLS};

use super::*;

fn offered() -> Offered {
    Offered {
        memory: true,
        wiki: false,
        references: false,
        expert: true,
    }
}

/// An injected launch has the base tools and the families its project
/// qualifies for; the others say why not. The harness's own known tools
/// are listed as its own.
#[test]
fn an_injected_launch_says_which_families_are_offered() {
    let launch = catalog("claude", Injection::Injected, offered(), 7);
    assert!(launch.injected && launch.reason.is_none());
    assert_eq!(launch.at_ms, 7);
    let tool = |name: &str| launch.tools.iter().find(|t| t.name == name).unwrap();
    assert!(tool("delegate").ready);
    assert_eq!(tool("delegate").scope, ToolScope::Writes);
    assert!(tool("memory_recall").ready);
    assert_eq!(tool("memory_recall").basis, "这个项目开启了记忆");
    assert!(!tool("read_wiki").ready);
    assert_eq!(
        tool("read_wiki").basis,
        "项目还没有 Wiki 页面，或没有允许 Agent 读"
    );
    assert!(!tool("search_references").ready);
    assert!(tool("ask_expert").ready);
    assert_eq!(tool("ask_expert").scope, ToolScope::SendsOut);
    let aeroric = launch
        .tools
        .iter()
        .filter(|t| t.owner == ToolOwner::Aeroric)
        .count();
    assert_eq!(aeroric, AERORIC_TOOLS.len());
    let bash = tool("Bash");
    assert_eq!(
        (bash.owner, bash.group, bash.ready),
        (ToolOwner::Harness, ToolGroup::Harness, true)
    );
    assert!(bash.basis.starts_with("Claude 自带"));
    assert!(catalog("codex", Injection::Injected, offered(), 7)
        .tools
        .iter()
        .any(|t| t.name == "apply_patch"));
}

/// A launch without the context server has none of Aeroric's tools, each
/// saying why; the harness's own stay listed.
#[test]
fn a_launch_without_the_server_says_why() {
    for (injection, harness, why) in [
        (
            Injection::Remote,
            "claude",
            "远端会话还没有接入 Aeroric 工具",
        ),
        (
            Injection::Unsupported,
            "omp",
            "OMP 还不能在启动时接入 Aeroric 工具",
        ),
        (
            Injection::NoServer,
            "codex",
            "宿主没有准备 Aeroric 工具服务器",
        ),
    ] {
        let launch = catalog(harness, injection, offered(), 1);
        assert!(!launch.injected);
        assert_eq!(launch.reason.as_deref(), Some(why));
        for tool in launch
            .tools
            .iter()
            .filter(|t| t.owner == ToolOwner::Aeroric)
        {
            assert!(!tool.ready, "{}", tool.name);
            assert_eq!(tool.basis, why);
        }
    }
    let omp = catalog("omp", Injection::Unsupported, offered(), 1);
    assert!(
        omp.tools.iter().all(|t| t.owner == ToolOwner::Aeroric),
        "no known own list"
    );
}

/// Each launch is kept; the latest is read back; without a folder nothing
/// is written.
#[test]
fn each_launch_keeps_its_tools() {
    let dir = std::env::temp_dir().join(format!("aeroric-tools-{}", uuid::Uuid::new_v4()));
    let log = ToolLog::default();
    log.note("t1", catalog("codex", Injection::Injected, offered(), 1));
    assert!(log.launches("t1").is_empty());
    log.set_dir(dir);
    log.note("t1", catalog("codex", Injection::Injected, offered(), 1));
    log.note("t1", catalog("codex", Injection::NoServer, offered(), 2));
    assert_eq!(log.launches("t1").len(), 2);
    let latest = log.latest("t1").unwrap();
    assert_eq!((latest.at_ms, latest.injected), (2, false));
    assert!(log.latest("t2").is_none());
}
