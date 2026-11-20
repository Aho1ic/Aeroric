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
            name: "sort".into(),
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
        managed: matches!(family, ProtocolFamily::Omp | ProtocolFamily::Dsh),
    }
}

/// A data folder holding `packs`, each pack's skill folders written.
fn data_with(packs: &[Pack]) -> PathBuf {
    let data = std::env::temp_dir().join(format!("aeroric-packs-{}", uuid::Uuid::new_v4()));
    for pack in packs {
        let at = dir(&data).join(&pack.id);
        for skill in &pack.skills {
            let folder = at.join("skills").join(skill);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join("SKILL.md"),
                format!("---\nname: {skill}\ndescription: x\n---\n分拣"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(at.join(MANIFEST), serde_json::to_string(pack).unwrap()).unwrap();
    }
    data
}

fn plan_for(packs: &[Pack], target: Target) -> Plan {
    let data = data_with(packs);
    let out = plan(&data, packs, target);
    std::fs::remove_dir_all(&data).ok();
    out
}

fn find<'a>(plan: &'a Plan, kind: PackKind) -> &'a PackItem {
    plan.items.iter().find(|i| i.kind == kind).unwrap()
}

/// Every item of a trusted pack is given or says why, in each family's own
/// place (the proposal's table).
#[test]
fn every_item_is_given_or_says_why() {
    let pack = full("gh", true);
    // (family, managed, server, skill, role given?, hook given?)
    let table = [
        (
            ProtocolFamily::Claude,
            false,
            "--mcp-config",
            "会话插件的 skills/",
            Some("aeroric:triager"),
            true,
        ),
        (
            ProtocolFamily::Claude,
            true,
            "--mcp-config",
            "受管目录的 skills/",
            Some("aeroric-triager"),
            false,
        ),
        (
            ProtocolFamily::Codex,
            false,
            "config.mcp_servers",
            "项目的 .agents/skills",
            None,
            false,
        ),
        (
            ProtocolFamily::Codex,
            true,
            "config.mcp_servers",
            "受管目录的 skills/",
            None,
            false,
        ),
        (
            ProtocolFamily::Omp,
            true,
            "mcp.json",
            "受管目录的 skills/",
            Some("aeroric-triager"),
            false,
        ),
        (
            ProtocolFamily::Dsh,
            true,
            "mcpServers",
            "受管目录的 skills/",
            None,
            false,
        ),
    ];
    for (family, managed, server_at, skill_at, role, hook) in table {
        let target = Target {
            family,
            remote: false,
            managed,
        };
        let plan = plan_for(std::slice::from_ref(&pack), target);
        let case = format!("{family:?} managed={managed}");
        assert_eq!(plan.items.len(), 5, "{case}");
        assert_eq!(plan.servers, [server("github")]);
        let mcp = find(&plan, PackKind::McpServer);
        assert!(
            mcp.ready && mcp.basis.contains(server_at),
            "{case}: {mcp:?}"
        );
        for kind in [PackKind::Skill, PackKind::Command] {
            let item = find(&plan, kind);
            assert!(
                item.ready && item.basis.contains(skill_at),
                "{case}: {item:?}"
            );
        }
        assert!(find(&plan, PackKind::Command).basis.starts_with("作为技能"));
        let given = find(&plan, PackKind::Role);
        match role {
            Some(name) => assert!(
                given.ready && given.basis.contains(name),
                "{case}: {given:?}"
            ),
            None => assert!(!given.ready && !given.basis.is_empty(), "{case}: {given:?}"),
        }
        let handler = find(&plan, PackKind::Hook);
        assert_eq!(handler.ready, hook, "{case}: {handler:?}");
        assert!(!handler.basis.contains("还没有把"), "{case}: {handler:?}");
    }
}

/// Skills: a missing folder, a built-in's name, an earlier pack's name, a
/// bad name, an empty command, and anything at all on a remote session are
/// withheld with why.
#[test]
fn a_skill_is_withheld_with_its_reason() {
    let mut first = full("a", true);
    first.skills = vec!["triage".into(), "brainstorming".into()];
    first.commands[0].body = " ".into();
    let mut second = full("b", true);
    second.skills = vec![];
    second.commands = vec![
        PackCommand {
            name: "triage".into(),
            body: "x".into(),
            ..PackCommand::default()
        },
        PackCommand {
            name: "bad name".into(),
            body: "x".into(),
            ..PackCommand::default()
        },
    ];
    let mut packs = vec![first, second];
    let data = data_with(&packs);
    packs[0].skills.push("ghost".into());
    let got: Vec<(String, bool, String)> = plan(&data, &packs, local(ProtocolFamily::Codex))
        .items
        .into_iter()
        .filter(|i| matches!(i.kind, PackKind::Skill | PackKind::Command))
        .map(|i| (i.name, i.ready, i.basis))
        .collect();
    let names: Vec<(&str, bool)> = got.iter().map(|(n, r, _)| (n.as_str(), *r)).collect();
    assert_eq!(
        names,
        [
            ("triage", true),
            ("brainstorming", false),
            ("ghost", false),
            ("sort", false),
            ("triage", false),
            ("bad name", false),
        ]
    );
    assert!(got[1].2.contains("内置技能同名"));
    assert!(got[2].2.contains("没有 skills/ghost/SKILL.md"));
    assert!(got[3].2.contains("正文为空"));
    assert_eq!(got[4].2, "同名技能已由能力包“包 a”提供");
    assert!(got[5].2.contains("字母、数字"));
    let remote = Target {
        family: ProtocolFamily::Claude,
        remote: true,
        managed: false,
    };
    let far = plan(&data, &packs, remote);
    assert!(
        far.items.iter().all(|i| !i.ready),
        "nothing goes to a remote session"
    );
    assert!(find(&far, PackKind::Skill).basis.contains("远端会话"));
    std::fs::remove_dir_all(&data).ok();
}

/// A pack skill is copied into Aeroric's store by content (links to it
/// count as Aeroric's); a command becomes a SKILL.md; a change is a new folder.
#[test]
fn a_pack_skill_is_placed_in_the_store_by_content() {
    let pack = full("gh", true);
    let data = data_with(std::slice::from_ref(&pack));
    let offers = skill_offers(&data);
    let ids: Vec<&str> = offers.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["triage", "sort"]);
    for offer in &offers {
        assert!(offer
            .dir
            .starts_with(crate::extensions::builtin_skills::base_dir(&data)));
        assert!(offer.dir.join("SKILL.md").is_file());
        assert!(offer.version.starts_with("pack-"));
    }
    let command = std::fs::read_to_string(offers[1].dir.join("SKILL.md")).unwrap();
    assert!(
        command.starts_with("---\nname: sort\ndescription: \"能力包“包 gh”的命令 sort\"\n---"),
        "{command}"
    );
    assert!(command.trim_end().ends_with("分拣议题"));
    assert_eq!(
        skill_offers(&data),
        offers,
        "the same content, the same folder"
    );
    std::fs::write(
        dir(&data).join("gh/skills/triage/SKILL.md"),
        "---\nname: triage\ndescription: y\n---\n改过",
    )
    .unwrap();
    let changed = skill_offers(&data);
    assert_ne!(changed[0].dir, offers[0].dir);
    assert!(std::fs::read_to_string(changed[0].dir.join("SKILL.md"))
        .unwrap()
        .contains("改过"));
    std::fs::remove_dir_all(&data).ok();
}

/// Claude's hooks.json holds the trusted, enabled packs' handlers for events
/// Claude has, tool events with a matcher.
#[test]
fn claude_hooks_hold_only_trusted_handlers_for_its_events() {
    let mut trusted_pack = full("a", true);
    trusted_pack.hooks.push(PackHook {
        event: "preToolUse".into(),
        command: "audit".into(),
    });
    trusted_pack.hooks.push(PackHook {
        event: "noSuchEvent".into(),
        command: "x".into(),
    });
    let untrusted = full("b", false);
    let hooks = claude_hooks(&[trusted_pack, untrusted]).unwrap();
    assert_eq!(
        hooks,
        serde_json::json!({"hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": "echo hi"}]}],
            "PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "audit"}]}],
        }})
    );
    assert_eq!(claude_hooks(&[full("c", false)]), None);
}

/// Pack roles join the library for native subagents when their id is new.
#[test]
fn pack_roles_join_the_library_when_their_id_is_new() {
    let mut pack = full("gh", true);
    pack.roles.push(Role {
        id: "researcher".into(),
        ..Role::default()
    });
    let data = data_with(std::slice::from_ref(&pack));
    let library = crate::extensions::native_roles::library(&data);
    assert!(library.iter().any(|r| r.id == "triager"));
    assert_eq!(library.iter().filter(|r| r.id == "researcher").count(), 1);
    let plan = plan(
        &data,
        std::slice::from_ref(&pack),
        local(ProtocolFamily::Omp),
    );
    let roles: Vec<(&str, bool)> = plan
        .items
        .iter()
        .filter(|i| i.kind == PackKind::Role)
        .map(|i| (i.name.as_str(), i.ready))
        .collect();
    assert_eq!(roles, [("triager", true), ("researcher", false)]);
    std::fs::remove_dir_all(&data).ok();
}

/// An untrusted pack's servers and handlers, which run commands, are not
/// projected; confirming it, or an official revision, lets them.
#[test]
fn an_untrusted_pack_runs_nothing() {
    let pack = full("gh", false);
    let plan = plan_for(std::slice::from_ref(&pack), local(ProtocolFamily::Claude));
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
        plan_for(packs, target)
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
        managed: false,
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
    let mut plan = plan_for(&[full("gh", true)], local(ProtocolFamily::Codex));
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

/// Mounting puts a trusted pack's skill, command, role and handler where the
/// plan says: Claude's plugin, OMP's managed home; Codex's project copies.
#[test]
fn mounting_places_what_the_plan_says() {
    use crate::extensions::builtin_skills::{mount, Mount};
    let pack = full("gh", true);
    let data = data_with(std::slice::from_ref(&pack));
    let plugin = mount(&data, "claude", None, Mount::ClaudePlugin)
        .unwrap()
        .unwrap();
    for skill in ["triage", "sort"] {
        assert!(
            plugin.join("skills").join(skill).join("SKILL.md").is_file(),
            "{skill}"
        );
    }
    assert!(plugin.join("agents/triager.md").is_file());
    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(plugin.join("hooks/hooks.json")).unwrap())
            .unwrap();
    assert_eq!(
        hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "echo hi"
    );

    let omp = data.join("omp-home");
    mount(&data, "omp", None, Mount::Home(&omp)).unwrap();
    assert!(omp.join("skills/triage/SKILL.md").is_file());
    assert!(omp.join("agents/aeroric-triager.md").is_file());
    assert!(!omp.join("hooks").exists(), "OMP takes no command handlers");

    // The pack goes off: its skills leave the plugin, the handlers go.
    let mut off = pack.clone();
    off.enabled = false;
    std::fs::write(
        dir(&data).join("gh").join(MANIFEST),
        serde_json::to_string(&off).unwrap(),
    )
    .unwrap();
    let plugin = mount(&data, "claude", None, Mount::ClaudePlugin)
        .unwrap()
        .unwrap();
    assert!(!plugin.join("skills/triage").exists());
    assert!(!plugin.join("hooks/hooks.json").exists());
    mount(&data, "omp", None, Mount::Home(&omp)).unwrap();
    assert!(!omp.join("skills/triage").exists(), "ours is removed");
    assert!(!omp.join("agents/aeroric-triager.md").exists());
    std::fs::remove_dir_all(&data).ok();
}
