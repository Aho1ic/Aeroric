use super::*;
use aeroric_protocol::packs::PackKind;

fn temp() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aeroric-pack-store-{}", uuid::Uuid::new_v4()))
}

fn github() -> PackServer {
    PackServer {
        name: "github".into(),
        description: "查 GitHub 的议题".into(),
        command: " gh-mcp ".into(),
        args: vec!["--stdio".into()],
        env: [
            ("GH_TOKEN".to_string(), "secret".to_string()),
            ("GH_HOST".to_string(), "github.com".to_string()),
        ]
        .into(),
        enabled: true,
    }
}

/// A server added here is a trusted pack of its own; the page reads it back
/// masked; saving the mask keeps the key; each place says what it gets.
#[test]
fn an_added_server_is_its_own_trusted_pack() {
    let data = temp();
    let saved = save_server(&data, github()).unwrap();
    assert_eq!(saved.mcp_servers[0].env["GH_TOKEN"], MASK);
    assert_eq!(saved.mcp_servers[0].command, "gh-mcp");
    let on_disk = std::fs::read_to_string(packs::dir(&data).join("github/pack.json")).unwrap();
    assert!(
        on_disk.contains("secret"),
        "the key is kept, unmasked, in the private file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = packs::dir(&data).join("github/pack.json");
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let view = overview(&data);
    assert_eq!(view.packs.len(), 1);
    let pack = &view.packs[0];
    assert!(pack.trusted);
    assert_eq!(pack.pack.mcp_servers[0].env["GH_TOKEN"], MASK);
    assert_eq!(pack.places.len(), 6);
    for place in &pack.places {
        let server = place
            .items
            .iter()
            .find(|i| i.kind == PackKind::McpServer)
            .unwrap();
        assert!(server.ready, "{}: {server:?}", place.place);
    }

    // Saved back with the mask and a new description: the key stays.
    let mut again = github();
    again.env.insert("GH_TOKEN".into(), MASK.into());
    again.description = "改过".into();
    set_enabled(&data, "github", false).unwrap();
    save_server(&data, again).unwrap();
    let (found, _) = packs::load(&data);
    assert_eq!(found[0].mcp_servers[0].env["GH_TOKEN"], "secret");
    assert_eq!(found[0].description, "改过");
    assert!(!found[0].enabled, "saving keeps it off");
    std::fs::remove_dir_all(&data).ok();
}

/// A bad name, an empty command, or a name an imported pack holds are refused.
#[test]
fn a_server_that_cannot_be_saved_says_why() {
    let data = temp();
    let named = |name: &str| PackServer {
        name: name.into(),
        ..github()
    };
    assert!(save_server(&data, named("has space"))
        .unwrap_err()
        .contains("字母"));
    assert!(save_server(
        &data,
        PackServer {
            command: " ".into(),
            ..github()
        }
    )
    .unwrap_err()
    .contains("empty command"));
    let folder = temp();
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("pack.json"),
        r#"{"id": "github", "name": "别人的包"}"#,
    )
    .unwrap();
    import(&data, &folder).unwrap();
    assert!(save_server(&data, github())
        .unwrap_err()
        .contains("不是在这里添加的"));
    std::fs::remove_dir_all(&data).ok();
    std::fs::remove_dir_all(&folder).ok();
}

/// An imported pack keeps its files, takes the folder as its source whatever
/// it claims, and runs nothing until confirmed.
#[test]
fn an_imported_pack_waits_for_confirmation() {
    let data = temp();
    let folder = temp();
    std::fs::create_dir_all(folder.join("skills/triage")).unwrap();
    std::fs::write(
        folder.join("skills/triage/SKILL.md"),
        "---\nname: triage\n---\nx",
    )
    .unwrap();
    std::fs::write(
        folder.join("pack.json"),
        r#"{"id": "team", "name": "团队包", "confirmed": true,
            "source": {"kind": "builtin", "origin": "x", "revision": "r1"},
            "skills": ["triage"],
            "mcpServers": [{"name": "linear", "command": "linear-mcp"}]}"#,
    )
    .unwrap();
    let pack = import(&data, &folder).unwrap();
    assert!(!pack.confirmed);
    assert_eq!(pack.source.kind, OWN_KIND);
    assert_eq!(pack.source.origin, folder.to_string_lossy());
    assert_eq!(pack.source.revision.as_deref(), Some("r1"));
    assert!(packs::dir(&data)
        .join("team/skills/triage/SKILL.md")
        .is_file());
    let view = overview(&data);
    assert!(!view.packs[0].trusted);
    let server = |view: &Overview| {
        view.packs[0].places[0]
            .items
            .iter()
            .find(|i| i.kind == PackKind::McpServer)
            .unwrap()
            .clone()
    };
    assert!(!server(&view).ready);
    let skill = view.packs[0].places[0]
        .items
        .iter()
        .find(|i| i.kind == PackKind::Skill)
        .unwrap();
    assert!(skill.ready, "a skill is only text: {skill:?}");

    set_confirmed(&data, "team", true).unwrap();
    assert!(server(&overview(&data)).ready);
    assert!(import(&data, &folder).unwrap_err().contains("已有"));
    assert!(import(&data, &data).unwrap_err().contains("没有 pack.json"));
    assert!(
        !std::fs::read_dir(packs::dir(&data))
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".importing")),
        "no staging left behind"
    );
    std::fs::remove_dir_all(&data).ok();
    std::fs::remove_dir_all(&folder).ok();
}

/// Removing goes through the given discard; an unknown or ill-named id fails.
#[test]
fn removing_a_pack_hands_its_folder_to_the_discard() {
    let data = temp();
    save_server(&data, github()).unwrap();
    let seen = std::sync::Mutex::new(Vec::new());
    let discard = |path: &Path| -> Result<(), String> {
        seen.lock().unwrap().push(path.to_path_buf());
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    };
    remove(&data, "github", &discard).unwrap();
    assert_eq!(*seen.lock().unwrap(), [packs::dir(&data).join("github")]);
    assert!(remove(&data, "github", &discard)
        .unwrap_err()
        .contains("没有能力包"));
    assert!(remove(&data, "../x", &discard)
        .unwrap_err()
        .contains("字母"));
    assert!(set_enabled(&data, "nope", true).is_err());
    std::fs::remove_dir_all(&data).ok();
}
