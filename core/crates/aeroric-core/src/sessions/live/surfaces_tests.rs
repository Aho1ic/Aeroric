use super::*;

fn observed(opening: Opening, injection: Injection, skills: Option<usize>) -> Observed {
    Observed {
        opening,
        injection,
        skills,
    }
}

fn entry(launch: &SurfaceLaunch, feature: Feature) -> CapabilityEntry {
    launch
        .surfaces
        .iter()
        .find(|e| e.feature == feature)
        .cloned()
        .unwrap()
}

#[test]
fn a_launch_lists_every_surface_in_order() {
    for family in ProtocolFamily::ALL {
        for place in [Place::Local, Place::Remote] {
            let kept = launch(
                *family,
                place,
                "x",
                &observed(Opening::Nothing, Injection::NoServer, None),
                1,
            );
            assert_eq!(
                kept.surfaces.iter().map(|e| e.feature).collect::<Vec<_>>(),
                Feature::SURFACES.to_vec()
            );
            for e in &kept.surfaces {
                if e.state != SupportState::Yes {
                    assert!(e.note.is_some(), "{family:?} {place:?} {e:?} says why");
                }
            }
        }
    }
}

#[test]
fn the_opening_says_what_became_of_it() {
    let at = |opening| {
        entry(
            &launch(
                ProtocolFamily::Dsh,
                Place::Local,
                "dsh",
                &observed(opening, Injection::Injected, Some(1)),
                1,
            ),
            Feature::SurfaceInstructions,
        )
    };
    let taken = at(Opening::Taken(42));
    assert_eq!(taken.state, SupportState::Yes);
    assert_eq!(
        taken.note.as_deref(),
        Some("这次启动从系统层给了开场内容（42 字）")
    );
    let in_message = at(Opening::Refused {
        brief: true,
        in_message: true,
    });
    assert_eq!(in_message.state, SupportState::No);
    assert!(
        in_message
            .note
            .as_deref()
            .unwrap()
            .contains("放在了第一条消息前"),
        "{in_message:?}"
    );
    let lost = at(Opening::Refused {
        brief: true,
        in_message: false,
    });
    assert!(lost.note.as_deref().unwrap().contains("没给出"), "{lost:?}");
    let profile_only = at(Opening::Refused {
        brief: false,
        in_message: false,
    });
    assert_eq!(
        profile_only.note.as_deref(),
        Some("这家没有系统层入口：记忆档案没给")
    );
    // Nothing to hand: the family's row, said to be untried this time.
    let nothing = at(Opening::Nothing);
    let row = support(
        ProtocolFamily::Dsh,
        Place::Local,
        Feature::SurfaceInstructions,
    );
    assert_eq!(nothing.state, row.state);
    assert_eq!(
        nothing.note,
        Some(format!("这次没有开场内容要给；{}", row.note.unwrap()))
    );
}

#[test]
fn tools_and_skills_follow_the_launch() {
    let kept = |injection, skills| {
        launch(
            ProtocolFamily::Claude,
            Place::Local,
            "claude",
            &observed(Opening::Nothing, injection, skills),
            1,
        )
    };
    let given = kept(Injection::Injected, Some(3));
    assert_eq!(
        entry(&given, Feature::ContextTools).state,
        SupportState::Yes
    );
    let skills = entry(&given, Feature::SurfaceSkills);
    assert_eq!(skills.state, SupportState::Yes);
    assert_eq!(skills.note.as_deref(), Some("这次挂了 3 个 Aeroric 技能"));

    let none = kept(Injection::Unsupported, Some(0));
    let tools = entry(&none, Feature::ContextTools);
    assert_eq!(tools.state, SupportState::No);
    assert_eq!(
        tools.note.as_deref(),
        Some("Claude 还不能在启动时接入 Aeroric 工具")
    );
    assert_eq!(entry(&none, Feature::SurfaceSkills).state, SupportState::No);

    // Not kept: the family's row.
    let unkept = kept(Injection::Injected, None);
    assert_eq!(
        entry(&unkept, Feature::SurfaceSkills),
        support(ProtocolFamily::Claude, Place::Local, Feature::SurfaceSkills)
    );

    // Remote: no skills and no tools, whatever was counted.
    let remote = launch(
        ProtocolFamily::Claude,
        Place::Remote,
        "claude",
        &observed(Opening::Taken(10), Injection::Remote, Some(5)),
        1,
    );
    assert_eq!(
        entry(&remote, Feature::SurfaceSkills).state,
        SupportState::No
    );
    assert_eq!(
        entry(&remote, Feature::ContextTools).state,
        SupportState::No
    );
    assert_eq!(
        entry(&remote, Feature::SurfaceInstructions).state,
        SupportState::Yes
    );
}

/// The rows a launch does not exercise. Each `Yes` here rests on a driver
/// test: Claude's `Task`/`Agent` mapping (`claude/map_tests.rs`), Codex's
/// `collabAgentToolCall` (`codex/driver_tests.rs`), OMP's subscription
/// at start and lifecycle mapping (`omp/driver_tests.rs`, `omp/map_tests.rs`).
#[test]
fn the_untried_surfaces_are_the_drivers_facts() {
    use SupportState::{No, Yes};
    let row = |family, feature| support(family, Place::Local, feature).state;
    let expect = [
        (ProtocolFamily::Claude, [Yes, No, Yes, No]),
        (ProtocolFamily::Codex, [Yes, No, Yes, No]),
        (ProtocolFamily::Omp, [Yes, No, Yes, No]),
        (ProtocolFamily::Dsh, [No, No, No, No]),
    ];
    for (family, [instructions, events, subagents, cli]) in expect {
        assert_eq!(
            row(family, Feature::SurfaceInstructions),
            instructions,
            "{family:?}"
        );
        assert_eq!(row(family, Feature::SurfaceEvents), events, "{family:?}");
        assert_eq!(
            row(family, Feature::SurfaceSubagents),
            subagents,
            "{family:?}"
        );
        assert_eq!(row(family, Feature::SurfaceCli), cli, "{family:?}");
    }
    for family in ProtocolFamily::ALL {
        for feature in [Feature::SurfaceEvents, Feature::SurfaceSkills] {
            assert_eq!(
                support(*family, Place::Remote, feature).state,
                No,
                "{family:?} {feature:?}"
            );
        }
    }
}

#[test]
fn the_log_keeps_the_latest_launch() {
    let dir = std::env::temp_dir().join(format!("aeroric-surfaces-{}", uuid::Uuid::new_v4()));
    let log = SurfaceLog::default();
    let o = observed(Opening::Nothing, Injection::Injected, Some(1));
    log.note(
        "t",
        launch(ProtocolFamily::Codex, Place::Local, "codex", &o, 1),
    );
    assert!(log.latest("t").is_none(), "no folder, nothing kept");
    log.set_dir(dir.clone());
    log.note(
        "t",
        launch(ProtocolFamily::Codex, Place::Local, "codex", &o, 1),
    );
    log.note(
        "t",
        launch(ProtocolFamily::Codex, Place::Local, "codex", &o, 2),
    );
    assert_eq!(log.launches("t").len(), 2);
    assert_eq!(log.latest("t").unwrap().at_ms, 2);
    std::fs::remove_dir_all(&dir).ok();
}
