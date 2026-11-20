use super::*;
use aeroric_protocol::delegation::Role;
use aeroric_protocol::packs::{PackCommand, PackHook, PackSource};

fn server(name: &str) -> PackServer {
    PackServer {
        name: name.into(),
        description: "查 GitHub 的议题和拉取请求".into(),
        command: "gh-mcp".into(),
        enabled: true,
        ..PackServer::default()
    }
}

/// A pack with one of each of the five kinds.
fn full(id: &str, confirmed: bool) -> Pack {
    Pack {
        id: id.into(),
        name: format!("包 {id}"),
        confirmed,
        enabled: true,
        skills: vec!["triage".into()],
        mcp_servers: vec![server("github")],
        roles: vec![Role {
            id: "triager".into(),
            ..Role::default()
        }],
        commands: vec![PackCommand {
            name: "triage".into(),
            body: "分拣议题".into(),
            ..PackCommand::default()
        }],
        hooks: vec![PackHook {
            event: "sessionStart".into(),
            command: "echo hi".into(),
        }],
        ..Pack::default()
    }
}

fn local(family: ProtocolFamily) -> Target {
    Target {
        family,
        remote: false,
    }
}

fn find<'a>(plan: &'a Plan, kind: PackKind) -> &'a PackItem {
    plan.items.iter().find(|i| i.kind == kind).unwrap()
}

/// Every item of a trusted pack is either given or listed with why; the
/// MCP server goes where each family reads one.
#[test]
fn every_item_is_given_or_says_why() {
    let pack = full("gh", true);
    for (family, place) in [
        (ProtocolFamily::Claude, "--mcp-config"),
        (ProtocolFamily::Codex, "config.mcp_servers"),
        (ProtocolFamily::Omp, "mcp.json"),
        (ProtocolFamily::Dsh, "mcpServers"),
    ] {
        let plan = plan(std::slice::from_ref(&pack), local(family));
        assert_eq!(plan.items.len(), 5, "{family:?}");
        assert_eq!(plan.servers, [server("github")]);
        let mcp = find(&plan, PackKind::McpServer);
        assert!(mcp.ready && mcp.basis.contains(place), "{mcp:?}");
        for kind in [
            PackKind::Skill,
            PackKind::Role,
            PackKind::Command,
            PackKind::Hook,
        ] {
            let item = find(&plan, kind);
            assert!(!item.ready && item.basis.contains("还没有"), "{item:?}");
        }
    }
}

/// An untrusted pack's servers and handlers, which run commands, are not
/// projected; confirming it, or an official revision, lets them.
#[test]
fn an_untrusted_pack_runs_nothing() {
    let pack = full("gh", false);
    let plan = plan(std::slice::from_ref(&pack), local(ProtocolFamily::Claude));
    assert!(plan.servers.is_empty());
    assert!(find(&plan, PackKind::McpServer)
        .basis
        .contains("来源还没有确认"));
    assert!(find(&plan, PackKind::Hook).basis.contains("来源还没有确认"));
    assert!(!find(&plan, PackKind::Skill).basis.contains("来源"));

    let mut official = pack.clone();
    official.source = PackSource {
        kind: "official-marketplace".into(),
        origin: "x".into(),
        revision: Some("abc".into()),
    };
    assert!(trusted(&official));
    official.source.revision = None;
    assert!(!trusted(&official), "an official pack at no known revision");
}

/// Off, remote, a bad or reserved name, a bad command, a name taken by an
/// earlier pack: each one withheld with its reason.
#[test]
fn a_server_is_withheld_with_its_reason() {
    let reason = |packs: &[Pack], target: Target| -> Vec<(String, bool, String)> {
        plan(packs, target)
            .items
            .into_iter()
            .filter(|i| i.kind == PackKind::McpServer)
            .map(|i| (i.name, i.ready, i.basis))
            .collect()
    };
    let mut pack = full("a", true);
    pack.enabled = false;
    assert!(reason(&[pack.clone()], local(ProtocolFamily::Codex))[0]
        .2
        .contains("能力包已停用"));
    pack.enabled = true;
    pack.mcp_servers[0].enabled = false;
    assert!(reason(&[pack.clone()], local(ProtocolFamily::Codex))[0]
        .2
        .contains("服务器已停用"));
    pack.mcp_servers[0].enabled = true;
    let remote = Target {
        family: ProtocolFamily::Claude,
        remote: true,
    };
    assert!(reason(&[pack.clone()], remote)[0].2.contains("远端会话"));

    pack.mcp_servers = vec![
        server("aeroric_context"),
        server("has space"),
        PackServer {
            command: " ".into(),
            ..server("empty")
        },
        server("github"),
    ];
    let mut later = full("b", true);
    later.mcp_servers = vec![server("github"), server("linear")];
    let got = reason(&[pack, later], local(ProtocolFamily::Omp));
    let names: Vec<(&str, bool)> = got.iter().map(|(n, r, _)| (n.as_str(), *r)).collect();
    assert_eq!(
        names,
        [
            ("aeroric_context", false),
            ("has space", false),
            ("empty", false),
            ("github", true),
            ("github", false),
            ("linear", true),
        ]
    );
    assert!(got[0].2.contains("Aeroric 自己的"));
    assert!(got[1].2.contains("字母、数字"));
    assert!(got[2].2.contains("empty command"));
    assert_eq!(got[4].2, "同名服务器已由能力包“包 a”提供");
}

/// A driver that will not take them leaves each planned server not given.
#[test]
fn a_refusing_driver_leaves_the_servers_not_given() {
    let mut plan = plan(&[full("gh", true)], local(ProtocolFamily::Codex));
    refused(&mut plan.items, ProtocolFamily::Codex);
    let mcp = find(&plan, PackKind::McpServer);
    assert!(!mcp.ready && mcp.basis.contains("Codex 这次启动不接受"));
}

/// Packs are read from their folders by id; a folder whose manifest is
/// missing, unreadable or of another id is listed with why.
#[test]
fn packs_are_read_from_their_folders() {
    let data = std::env::temp_dir().join(format!("aeroric-packs-{}", uuid::Uuid::new_v4()));
    assert_eq!(load(&data), (vec![], vec![]), "no folder, no packs");
    let write = |folder: &str, text: &str| {
        let at = dir(&data).join(folder);
        std::fs::create_dir_all(&at).unwrap();
        if !text.is_empty() {
            std::fs::write(at.join(MANIFEST), text).unwrap();
        }
    };
    write("zeta", &serde_json::to_string(&full("zeta", true)).unwrap());
    write(
        "alpha",
        &serde_json::to_string(&full("alpha", false)).unwrap(),
    );
    write("other", &serde_json::to_string(&full("gh", true)).unwrap());
    write("bad", "{");
    write("empty", "");
    write(".hidden", "{");
    let (packs, broken) = load(&data);
    assert_eq!(
        packs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    let folders: Vec<&str> = broken.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(folders, ["bad", "empty", "other"]);
    assert!(broken[2].1.contains("和文件夹名不一致"));
    std::fs::remove_dir_all(&data).ok();
}
