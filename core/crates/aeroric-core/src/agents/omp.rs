//! omp 的机器可查不变量(P3-02 Action 5 的 OMP 半边;INV-05/INV-07)。
//!
//! 三条都是"错了会静默坏"的形状,所以必须是可失败的断言而不是注释:
//! 1. hook 扩展名只认 `.js`/`.ts`——`.mjs` 会被上游 `isExtensionFile` 静默丢弃,
//!    任务永远停在 running(1.4.7 开发期真踩过,见宿主 omp_home.rs 的删除注释);
//! 2. `agent_end.willContinue === true` 不是终态,hook 必须对它短路;
//! 3. omp 的 PTY 计划函数**绝不**产出 `--mode`(上游以 `mode === undefined` 判定交互
//!    TUI,任何 --mode 值都会把 omp 打回非交互通道);反过来,原生会话(ADR-043
//!    二期)的 `omp_rpc_args` **必须**以 `--mode rpc` 开头,且不带位置参数提示词。

/// 托管 hook 的安装名:装在 `<PI_CODING_AGENT_DIR>/hooks/post/` 下,`*` 是 omp
/// 的 tool 通配(`discovery/builtin.ts:707` 按 basename 解析);实际的事件订阅
/// 在脚本里用 `pi.on` 完成,与文件名里的 tool 无关。宿主 `omp_home` 以此为
/// 唯一来源构造安装路径。
pub const OMP_HOOK_FILE_NAME: &str = "*.js";

/// 该 hook 文件名能否被 omp 加载(`extensibility/extensions/loader.ts` 的
/// `isExtensionFile` 只认 `.js`/`.ts`;`.mjs` 静默丢弃)。
pub fn omp_hook_extension_is_loadable(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".js") || lower.ends_with(".ts")
}

/// hook 源码必须包含 willContinue 短路:agent_end 携带
/// `willContinue === true` 时表示会话已自行安排自动续跑(auto-retry / 空回复
/// 重试),不是用户可见的终态——报了会让角标闪一下又消失(INV-07)。
/// 检查的是源码里存在对该精确比较的短路返回;宿主安装 hook 前以真实脚本断言。
pub fn omp_hook_source_guards_will_continue(source: &str) -> bool {
    source.contains("willContinue === true")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::launch::{omp_plan, omp_rpc_args, LaunchInput};

    #[test]
    fn only_js_and_ts_hook_files_load() {
        assert!(omp_hook_extension_is_loadable("*.js"));
        assert!(omp_hook_extension_is_loadable("aeroric-omp-hook.ts"));
        // 反例:1.4.7 开发期装过 *.mjs,目录扫描能看到、hook 永远不加载。
        assert!(!omp_hook_extension_is_loadable("*.mjs"));
        assert!(!omp_hook_extension_is_loadable("aeroric-omp-hook.js.bak"));
        assert!(!omp_hook_extension_is_loadable("hook"));
        assert!(!omp_hook_extension_is_loadable(""));
    }

    #[test]
    fn the_will_continue_guard_is_detectable_and_its_absence_fails() {
        let guarded = r#"pi.on("agent_end", async (event, ctx) => {
    if (ctx?.hasUI !== true) return;
    if (event?.willContinue === true) return;
    writeStatus("Stop");
});"#;
        assert!(omp_hook_source_guards_will_continue(guarded));

        // 变体(等价写法)也认。
        assert!(omp_hook_source_guards_will_continue(
            "if (event?.willContinue === true) return;"
        ));

        // 反例:删掉短路,Stop 会在自动续跑前把角标打灭。
        let unguarded = r#"pi.on("agent_end", async (event, ctx) => {
    if (ctx?.hasUI !== true) return;
    writeStatus("Stop");
});"#;
        assert!(!omp_hook_source_guards_will_continue(unguarded));
        assert!(!omp_hook_source_guards_will_continue(""));
    }

    /// INV-05/INV-07:任何权限档、任何可选事实组合下,omp 计划都不得产出
    /// `--mode`。上游 `main.ts` 以 `parsedArgs.mode === undefined` 判定是否进入
    /// 原生交互式 TUI。
    #[test]
    fn the_omp_plan_never_emits_mode() {
        let input = LaunchInput::new("omp");
        for mode in ["ask", "auto_edit", "full_access", "unknown"] {
            for thinking in [None, Some("ultra"), Some("off")] {
                for resume in [None, Some("sess-1")] {
                    let plan = omp_plan(
                        &input,
                        mode,
                        &[],
                        "/work/proj",
                        None,
                        thinking,
                        resume,
                        &["--".to_string(), "hello".to_string()],
                    );
                    assert!(
                        !plan.args.iter().any(|arg| arg == "--mode"),
                        "omp 带 --mode 就不是交互式 TUI 了:{plan:?}"
                    );
                }
            }
        }
    }

    /// 原生会话:永远 `--mode rpc` 打头,提示词不进 argv。
    #[test]
    fn the_rpc_args_always_start_with_mode_rpc_and_carry_no_prompt() {
        let args = omp_rpc_args(
            "ask",
            "/work/proj",
            Some("fake/fake-1"),
            Some("high"),
            Some("/h/s.jsonl"),
        );
        assert_eq!(
            args,
            [
                "--mode",
                "rpc",
                "--approval-mode",
                "always-ask",
                "--cwd",
                "/work/proj",
                "--model",
                "fake/fake-1",
                "--thinking",
                "high",
                "--resume",
                "/h/s.jsonl"
            ]
        );
        let bare = omp_rpc_args("unknown", "/w", None, None, None);
        assert_eq!(bare, ["--mode", "rpc", "--cwd", "/w"]);
    }
}
