//! Skill Hub 的路径守卫（自宿主 `skills.rs` 原样上收；`agent_skills_dir`
//! 以项目根参数化）。

use std::path::PathBuf;

/// 四族 agent 的项目级 skills 子目录（verbatim）。
pub fn agent_skills_dir(project_path: &std::path::Path, agent: &str) -> PathBuf {
    let sub = match agent {
        "codex" => ".codex/skills",
        // dsh-skill-filesystem 的项目级根之一(另一处是 .agents/skills)。
        "dsh" => ".dsh/skills",
        // omp 原生读取项目 .omp/skills(另一处是 .agents/skills)。
        "omp" => ".omp/skills",
        _ => ".claude/skills",
    };
    project_path.join(sub)
}

/// skill_name 必须是单段合法目录名：非空、非 `.` / `..`、不含路径分隔符。
/// 该名字会作为 `agent_skills_dir(...).join(&skill_name)` 的最后一段，必须严格限定。
pub fn validate_skill_name(skill_name: &str) -> Result<(), String> {
    if skill_name.is_empty() {
        return Err("Skill name cannot be empty".to_string());
    }
    if skill_name == "." || skill_name == ".." {
        return Err(format!("Invalid skill name: {}", skill_name));
    }
    if skill_name.contains('/') || skill_name.contains('\\') || skill_name.contains('\0') {
        return Err(format!(
            "Skill name must not contain path separators: {}",
            skill_name
        ));
    }
    Ok(())
}

/// 安装落点必须恰为 `<skills_dir>/<skill_name>/SKILL.md` 的父目录——
/// 拒绝任何带子路径/穿越的名字（与 `validate_skill_name` 双保险）。
pub fn install_target(skills_dir: &std::path::Path, skill_name: &str) -> Result<PathBuf, String> {
    validate_skill_name(skill_name)?;
    Ok(skills_dir.join(skill_name))
}
