//! omp 的机器可查不变量(P3-02 Action 5 的 OMP 半边;INV-05)。
//!
//! omp 的 PTY 计划函数**绝不**产出 `--mode`(上游以 `mode === undefined` 判定交互
//! TUI,任何 --mode 值都会把 omp 打回非交互通道);反过来,原生会话(ADR-043
//! 二期)的 `omp_rpc_args` **必须**以 `--mode rpc` 开头,且不带位置参数提示词。
//! 托管 hook 已于 2026-10-27 删除:omp 的状态只来自 RPC 协议。

#[cfg(test)]
mod tests {
    use crate::agents::launch::{omp_plan, omp_rpc_args, LaunchInput};

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
