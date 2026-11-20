//! H1 (2026-11-20): what one launch of a session had on each surface
//! through which a harness uses Aeroric — the opening in a system layer,
//! the context tools, the skills, the harness's own events, its subagents,
//! the command line.
//!
//! The opening, the tools and the skills are what the executor saw happen
//! at that launch (the driver took the opening, took the context server,
//! the skills mounted). The other surfaces are not exercised per launch;
//! they come from the family's table (`capabilities.rs`), whose rows tests
//! tie to the drivers.

use std::path::PathBuf;

use aeroric_protocol::live_capability::{
    CapabilityEntry, Feature, Place, SupportState, SurfaceLaunch,
};
use parking_lot::Mutex;

use super::capabilities::support;
use super::tool_catalog::Injection;
use crate::storage::launch_log;
use crate::tasks::ProtocolFamily;

/// What became of the opening (memory profile, handoff brief) at a launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    /// Nothing to hand: no profile, no brief.
    Nothing,
    /// Handed to the system layer and taken; its length in characters.
    Taken(usize),
    /// The harness has no system layer. A profile is then not given; a
    /// brief goes in front of the first message when there is one
    /// (`in_message`), and is lost when a resume carries none.
    Refused { brief: bool, in_message: bool },
}

/// What the executor saw at one launch.
#[derive(Clone, Copy, Debug)]
pub struct Observed {
    pub opening: Opening,
    pub injection: Injection,
    /// Aeroric's own skills (built-in, learned) mounted; `None` when the
    /// mount was not kept (a remote launch, a host with no data folder).
    pub skills: Option<usize>,
}

/// The surfaces of one launch, in `Feature::SURFACES` order.
pub fn launch(
    family: ProtocolFamily,
    place: Place,
    harness: &str,
    observed: &Observed,
    at_ms: u64,
) -> SurfaceLaunch {
    SurfaceLaunch {
        at_ms,
        harness: harness.to_string(),
        place,
        surfaces: Feature::SURFACES
            .iter()
            .map(|feature| entry(family, place, *feature, harness, observed))
            .collect(),
    }
}

fn entry(
    family: ProtocolFamily,
    place: Place,
    feature: Feature,
    harness: &str,
    observed: &Observed,
) -> CapabilityEntry {
    let table = support(family, place, feature);
    let said = |state, note: String| CapabilityEntry {
        feature,
        state,
        note: Some(note),
    };
    match feature {
        Feature::SurfaceInstructions => match observed.opening {
            Opening::Taken(chars) => said(
                SupportState::Yes,
                format!("这次启动从系统层给了开场内容（{chars} 字）"),
            ),
            Opening::Refused { brief, in_message } => {
                let brief = match (brief, in_message) {
                    (false, _) => "",
                    (true, true) => "交接简报放在了第一条消息前，",
                    (true, false) => "这次恢复没有消息可附，交接简报没给出，",
                };
                said(
                    SupportState::No,
                    format!("这家没有系统层入口：{brief}记忆档案没给"),
                )
            }
            Opening::Nothing => CapabilityEntry {
                note: Some(match &table.note {
                    Some(note) => format!("这次没有开场内容要给；{note}"),
                    None => "这次没有开场内容要给".into(),
                }),
                ..table
            },
        },
        Feature::ContextTools => match observed.injection.reason(harness) {
            None => said(
                SupportState::Yes,
                "这次启动接入了 Aeroric 工具服务器".into(),
            ),
            Some(why) => said(SupportState::No, why),
        },
        Feature::SurfaceSkills => match observed.skills {
            Some(0) if table.state == SupportState::Yes => said(
                SupportState::No,
                "这次没有挂 Aeroric 技能：内置技能都关掉了，也没有记忆沉淀的技能".into(),
            ),
            Some(n) if table.state == SupportState::Yes => {
                said(SupportState::Yes, format!("这次挂了 {n} 个 Aeroric 技能"))
            }
            _ => table,
        },
        _ => table,
    }
}

/// Each task's launches and their surfaces, under `<data>/surfaces/`.
#[derive(Default)]
pub struct SurfaceLog {
    data_dir: Mutex<Option<PathBuf>>,
}

const FOLDER: &str = "surfaces";

impl SurfaceLog {
    pub fn set_dir(&self, data_dir: PathBuf) {
        *self.data_dir.lock() = Some(data_dir);
    }

    pub fn note(&self, task_id: &str, launch: SurfaceLaunch) {
        let Some(data_dir) = self.data_dir.lock().clone() else {
            return;
        };
        let path = launch_log::file(&data_dir, FOLDER, task_id);
        if let Err(error) = launch_log::push(&path, launch) {
            eprintln!("[aeroric] surfaces of {task_id} not kept: {error}");
        }
    }

    pub fn launches(&self, task_id: &str) -> Vec<SurfaceLaunch> {
        let Some(data_dir) = self.data_dir.lock().clone() else {
            return Vec::new();
        };
        launch_log::read(&launch_log::file(&data_dir, FOLDER, task_id))
    }

    pub fn latest(&self, task_id: &str) -> Option<SurfaceLaunch> {
        self.launches(task_id).pop()
    }
}

#[cfg(test)]
#[path = "surfaces_tests.rs"]
mod tests;
