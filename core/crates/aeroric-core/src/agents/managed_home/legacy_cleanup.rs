//! 清理旧版 Aeroric 为 Claude/Codex 装下的 hook。
//!
//! Agent 经结构化协议驱动后不再装 hook,但旧版本留下了四处痕迹:
//! - `~/.claude/settings.json` 里带 `_aeroric_managed`(或旧产品名标记)的 hook 条目;
//! - `~/.codex/config.toml` 里 `CODEX_BEGIN … CODEX_END` 包住的块;
//! - `~/.aeroric/hooks/` 下的脚本、Claude settings 文件、Codex 片段与 `events/`;
//! - 自定义 codex-like Agent 的包装脚本追加的片段——片段文件删除后它们静默跳过。
//!
//! 只动带 Aeroric 标记的内容与 Aeroric 自己写下的文件名;用户自己的 hook、
//! 解析不了的文件一律不碰。写回保留原文件权限。
//! 见 .agents/notes/implemented/simplification/2026-10-27-remove-legacy-hook-pipeline.md。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::claude_hooks::{is_aeroric_managed, uninject_claude_value};
use super::codex_hooks::uninject_codex_text;

/// `~/.aeroric/hooks/` 下由旧版 Aeroric 写下的文件。
const LEGACY_HOOK_FILES: &[&str] = &[
    "aeroric-hook.mjs",
    "claude-settings.json",
    "claude-runtime-fast-hooks.json",
    "claude-runtime-fast.json",
    "codex-hooks.toml",
];

/// 一次清理的结果:改了什么、跳过了什么(给启动日志)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LegacyHookCleanup {
    pub changed: Vec<PathBuf>,
    pub skipped: Vec<(PathBuf, String)>,
}

/// 清理 `home` 下旧版 Aeroric 装的 Claude/Codex hook。幂等;没有痕迹时什么都不写。
pub fn remove_legacy_agent_hooks(home: &Path) -> LegacyHookCleanup {
    let mut report = LegacyHookCleanup::default();
    clean_claude_settings(&home.join(".claude").join("settings.json"), &mut report);
    clean_codex_config(&home.join(".codex").join("config.toml"), &mut report);
    clean_hooks_dir(&home.join(".aeroric").join("hooks"), &mut report);
    report
}

fn clean_claude_settings(path: &Path, report: &mut LegacyHookCleanup) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    if raw.trim().is_empty() {
        return;
    }
    let root: Value = match serde_json::from_str(&raw) {
        Ok(root) => root,
        Err(error) => {
            report
                .skipped
                .push((path.to_path_buf(), format!("not JSON: {error}")));
            return;
        }
    };
    if !has_managed_claude_entry(&root) {
        return;
    }
    let updated = uninject_claude_value(root);
    let mut content = match serde_json::to_string_pretty(&updated) {
        Ok(content) => content,
        Err(error) => {
            report.skipped.push((path.to_path_buf(), error.to_string()));
            return;
        }
    };
    content.push('\n');
    record(replace_preserving_mode(path, &content), path, report);
}

fn has_managed_claude_entry(root: &Value) -> bool {
    root.get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|hooks| {
            hooks
                .values()
                .filter_map(Value::as_array)
                .flatten()
                .any(is_aeroric_managed)
        })
}

fn clean_codex_config(path: &Path, report: &mut LegacyHookCleanup) {
    let Ok(existing) = fs::read_to_string(path) else {
        return;
    };
    let updated = uninject_codex_text(&existing);
    if updated == existing {
        return;
    }
    if let Err(error) = toml::from_str::<toml::Value>(&updated) {
        report.skipped.push((
            path.to_path_buf(),
            format!("TOML invalid after removal: {error}"),
        ));
        return;
    }
    record(replace_preserving_mode(path, &updated), path, report);
}

fn clean_hooks_dir(dir: &Path, report: &mut LegacyHookCleanup) {
    if !dir.is_dir() {
        return;
    }
    for name in LEGACY_HOOK_FILES {
        let path = dir.join(name);
        if path.is_file() {
            record(
                fs::remove_file(&path).map_err(|e| e.to_string()),
                &path,
                report,
            );
        }
    }
    let events = dir.join("events");
    if events.is_dir() {
        record(
            fs::remove_dir_all(&events).map_err(|e| e.to_string()),
            &events,
            report,
        );
    }
    // 只在空了时删目录:里面若还有不认识的文件,说明不全是我们写的。
    if fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
        record(fs::remove_dir(dir).map_err(|e| e.to_string()), dir, report);
    }
}

fn record(result: Result<(), String>, path: &Path, report: &mut LegacyHookCleanup) {
    match result {
        Ok(()) => report.changed.push(path.to_path_buf()),
        Err(error) => report.skipped.push((path.to_path_buf(), error)),
    }
}

/// 同目录临时文件 + rename,沿用原文件权限:这些是用户自己的配置文件,
/// 不能顺手把它们改成 0600 或别的模式。
fn replace_preserving_mode(path: &Path, content: &str) -> Result<(), String> {
    let permissions = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{name}.{}.aeroric-tmp", std::process::id()));
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    let result = fs::set_permissions(&tmp, permissions)
        .and_then(|()| fs::rename(&tmp, path))
        .map_err(|e| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_home() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-legacy-hooks-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn claude_marked_entries_go_and_the_users_own_hooks_stay() {
        let home = temp_home();
        let settings = home.join(".claude/settings.json");
        let user_hook = json!({"hooks": [{"type": "command", "command": "my-lint"}]});
        write(
            &settings,
            &json!({
                "model": "opus",
                "hooks": {
                    "Stop": [
                        user_hook,
                        {"_aeroric_managed": "1", "hooks": [{"type": "command", "command": "node aeroric-hook.mjs"}]}
                    ],
                    "SessionStart": [{"_nezha_managed": "1", "hooks": []}]
                }
            })
            .to_string(),
        );

        let report = remove_legacy_agent_hooks(&home);

        assert_eq!(report.changed, vec![settings.clone()]);
        let after: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(after["model"], "opus");
        assert_eq!(after["hooks"]["Stop"], json!([user_hook]));
        assert_eq!(after["hooks"]["SessionStart"], json!([]));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn untouched_files_are_not_rewritten_and_unparseable_ones_are_reported() {
        let home = temp_home();
        let settings = home.join(".claude/settings.json");
        let clean = "{\"hooks\":{\"Stop\":[]}}";
        write(&settings, clean);
        let codex = home.join(".codex/config.toml");
        write(&codex, "model = \"o3\"\n");

        assert_eq!(
            remove_legacy_agent_hooks(&home),
            LegacyHookCleanup::default()
        );
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            clean,
            "no rewrite, no reformat"
        );

        write(&settings, "{ not json");
        let report = remove_legacy_agent_hooks(&home);
        assert!(report.changed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{ not json");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn codex_block_is_removed_and_the_rest_of_the_config_survives() {
        let home = temp_home();
        let codex = home.join(".codex/config.toml");
        let block =
            crate::agents::managed_home::build_codex_block("/usr/bin/node", "/h/aeroric-hook.mjs");
        write(
            &codex,
            &format!("model = \"o3\"\n\n{block}\n[profiles.fast]\nmodel = \"o4\"\n"),
        );

        let report = remove_legacy_agent_hooks(&home);

        assert_eq!(report.changed, vec![codex.clone()]);
        let after = fs::read_to_string(&codex).unwrap();
        assert!(!after.contains(crate::agents::managed_home::CODEX_BEGIN));
        assert!(!after.contains("aeroric-hook.mjs"));
        let parsed: toml::Value = toml::from_str(&after).unwrap();
        assert_eq!(parsed["model"].as_str(), Some("o3"));
        assert_eq!(parsed["profiles"]["fast"]["model"].as_str(), Some("o4"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn hooks_dir_loses_only_aeroric_files_and_survives_if_others_remain() {
        let home = temp_home();
        let dir = home.join(".aeroric/hooks");
        for name in LEGACY_HOOK_FILES {
            write(&dir.join(name), "x");
        }
        write(&dir.join("events/task-1/events.jsonl"), "{}\n");
        write(&dir.join("mine.sh"), "echo user");

        remove_legacy_agent_hooks(&home);

        for name in LEGACY_HOOK_FILES {
            assert!(!dir.join(name).exists(), "{name} should be gone");
        }
        assert!(!dir.join("events").exists());
        assert!(
            dir.join("mine.sh").is_file(),
            "unknown files are not ours to delete"
        );

        fs::remove_file(dir.join("mine.sh")).unwrap();
        write(&dir.join("aeroric-hook.mjs"), "x");
        remove_legacy_agent_hooks(&home);
        assert!(!dir.exists(), "an emptied hooks dir is removed");
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rewritten_user_files_keep_their_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let home = temp_home();
        let codex = home.join(".codex/config.toml");
        let block = crate::agents::managed_home::build_codex_block("node", "s.mjs");
        write(&codex, &format!("a = 1\n{block}"));
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o644)).unwrap();

        remove_legacy_agent_hooks(&home);

        assert_eq!(
            fs::metadata(&codex).unwrap().permissions().mode() & 0o777,
            0o644
        );
        fs::remove_dir_all(home).unwrap();
    }
}
