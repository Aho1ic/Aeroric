//! P8-01 验收：extensions 契约（`tests/extensions.rs`）。
//!
//! 恶意路径/无效 MCP command/env secret mask/失败不留半配置/hook 扩展名
//! 守卫/信任判定。既有单测在 `--lib`（宿主 skills/mcp/hooks/dsh_plugins 的
//! 内联测试仍在宿主跑——命令层未动）。

use std::collections::BTreeMap;
use std::path::Path;

use aeroric_core::extensions::trust::{ExtensionSource, TrustVerdict};
use aeroric_core::extensions::{hooks as hook_guard, mcp, plugins, skills, trust};

// ── 恶意路径 ─────────────────────────────────────────────────────────────────

#[test]
fn malicious_skill_names_are_rejected_before_any_path_join() {
    for bad in ["", ".", "..", "a/b", "a\\b", "evil\0name", "../../etc"] {
        assert!(
            skills::validate_skill_name(bad).is_err(),
            "{bad:?} 应被拒绝"
        );
    }
    for good in ["pdf-tools", "周报助手", "my_skill-1"] {
        assert!(
            skills::validate_skill_name(good).is_ok(),
            "{good:?} 应被接受"
        );
    }
    // install_target 的落点恒在 skills_dir 内。
    let target = skills::install_target(Path::new("/proj/.claude/skills"), "pdf").expect("target");
    assert!(target.starts_with("/proj/.claude/skills"));
    assert!(skills::install_target(Path::new("/proj/.claude/skills"), "../escape").is_err());
}

#[test]
fn plugin_directories_reject_traversal_and_versions_compare_semantically() {
    assert!(plugins::validate_plugin_dir("../evil").is_err());
    assert!(plugins::validate_plugin_dir("sub/dir").is_err());
    assert!(plugins::validate_plugin_dir("minimax").is_ok());
    assert_eq!(
        plugins::compare_versions("1.2.0", "1.10.0"),
        std::cmp::Ordering::Less,
        "语义比较而非字典序"
    );
    assert_eq!(
        plugins::compare_versions("2.0", "1.9.9"),
        std::cmp::Ordering::Greater
    );
}

// ── MCP：无效 command / env secret mask ─────────────────────────────────────

#[test]
fn mcp_servers_without_a_command_or_with_bad_env_are_rejected() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".to_string(), "k".to_string());
    assert!(mcp::validate_server("empty", "  ", &env).is_err());
    env.insert("BAD=KEY".to_string(), "v".to_string());
    assert!(
        mcp::validate_server("badenv", "uvx", &env).is_err(),
        "env 键含 = 拒绝"
    );
    let mut clean = BTreeMap::new();
    clean.insert("API_KEY".to_string(), "k".to_string());
    assert!(mcp::validate_server("ok", "uvx", &clean).is_ok());
}

#[test]
fn mcp_env_secrets_are_masked_but_plain_values_pass_through() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".to_string(), "sk-live-123".to_string());
    env.insert("LOG_LEVEL".to_string(), "debug".to_string());
    env.insert("GITHUB_TOKEN".to_string(), "ghp-x".to_string());
    env.insert("EMPTY_SECRET".to_string(), String::new());
    let masked = mcp::mask_env(&env);
    assert_eq!(masked.get("API_KEY").map(String::as_str), Some("••••••••"));
    assert_eq!(
        masked.get("GITHUB_TOKEN").map(String::as_str),
        Some("••••••••")
    );
    assert_eq!(masked.get("LOG_LEVEL").map(String::as_str), Some("debug"));
    // 空值不掩码（空＝未设置，无泄露面）。
    assert_eq!(masked.get("EMPTY_SECRET").map(String::as_str), Some(""));
}

// ── 安装失败不留半配置 ───────────────────────────────────────────────────────

#[test]
fn a_failed_install_leaves_no_staging_behind() {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-ext-install-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let target = dir.join("hub.json");
    // 成功路径：原子落盘。
    aeroric_core::extensions::atomic_install(&target, "{\"v\":1}").expect("install");
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "{\"v\":1}");
    // 失败注入：父路径被一个普通文件占用 → create_dir_all 失败 → 不留 .tmp。
    let blocker = dir.join("occupied");
    std::fs::write(&blocker, "not a dir").expect("write blocker");
    let blocked = blocker.join("hub.json");
    assert!(aeroric_core::extensions::atomic_install(&blocked, "{}").is_err());
    // 成功安装的目录里没有 .tmp 残留。
    let tmp_siblings = std::fs::read_dir(&dir)
        .expect("read dir")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .count();
    assert_eq!(tmp_siblings, 0);
    let leftovers = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(leftovers, 0, "失败的安装不留下半配置或临时文件");
}

// ── hook 资源守卫（omp `.js/.ts` 规则）──────────────────────────────────────

#[test]
fn hook_scripts_must_be_js_or_ts() {
    assert!(hook_guard::is_hook_script("aeroric-omp-hook.js"));
    assert!(hook_guard::is_hook_script("Aeroric.Hook.TS"));
    assert!(
        !hook_guard::is_hook_script("hook.py"),
        "loader 会静默丢弃非 .js/.ts"
    );
    assert!(!hook_guard::is_hook_script("hook.js.bak"));
    assert!(hook_guard::validate_hook("hook.sh", "agent_start").is_err());
    assert!(hook_guard::validate_hook("hook.js", "").is_err());
    assert!(hook_guard::validate_hook("hook.js", "agent_start").is_ok());
}

// ── 信任判定：写入范围越界一律拒绝，确认不洗白 ─────────────────────────────

#[test]
fn trust_verdict_rejects_out_of_scope_writes_even_when_confirmed() {
    let evil = ExtensionSource {
        kind: "third-party".to_string(),
        origin: "https://example.com/pkg".to_string(),
        revision: Some("abc".to_string()),
        write_scope: vec!["../bin".to_string()],
        requested_permissions: vec!["exec".to_string()],
    };
    assert_eq!(
        trust::verdict(&evil, true),
        TrustVerdict::Rejected,
        "越界写入确认也不行"
    );

    let empty_scope = ExtensionSource {
        kind: "builtin".to_string(),
        origin: "builtin".to_string(),
        revision: Some("1".to_string()),
        write_scope: vec![],
        requested_permissions: vec![],
    };
    assert_eq!(trust::verdict(&empty_scope, true), TrustVerdict::Rejected);

    let third_party = ExtensionSource {
        kind: "github".to_string(),
        origin: "github.com/x/y".to_string(),
        revision: Some("abc".to_string()),
        write_scope: vec!["skills/pdf".to_string()],
        requested_permissions: vec!["read-files".to_string()],
    };
    assert_eq!(
        trust::verdict(&third_party, false),
        TrustVerdict::RequiresConfirmation
    );
    assert_eq!(trust::verdict(&third_party, true), TrustVerdict::Trusted);

    let builtin = ExtensionSource {
        kind: "builtin".to_string(),
        origin: "builtin".to_string(),
        revision: Some("9".to_string()),
        write_scope: vec!["skills/builtin".to_string()],
        requested_permissions: vec![],
    };
    assert_eq!(trust::verdict(&builtin, false), TrustVerdict::Trusted);
}
