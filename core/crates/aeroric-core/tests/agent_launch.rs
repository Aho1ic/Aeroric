//! P3-02 切片二:core 的四族计划函数重放 parity fixtures。
//!
//! fixtures 的期望由**旧宿主路径**(`build_*_cmd + add_*_launch_args` 组合出的
//! `CommandBuilder`,经 `get_argv`/`get_env`/`get_cwd` 提取)在迁移前亲自算出并
//! 冻结(采集器与等价守卫在宿主 `pty.rs::agent_launch_parity_fixture_harvest`)。
//! core 的计划函数对同一批输入重放,必须逐字节相等——这是"计划半边迁 core 不改
//! 语义"的机器证明。宿主侧另有一条同 fixtures 的等价断言,两侧共用一份真相。

use aeroric_core::agents::launch::{claude_plan, codex_plan, dsh_plan, omp_plan, LaunchInput};
use serde_json::Value;

fn fixture() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent-launch-parity.json");
    serde_json::from_slice(&std::fs::read(&path).expect("agent-launch-parity.json must exist"))
        .expect("parity fixture must parse")
}

fn as_string(value: &Value) -> String {
    value.as_str().expect("string field").to_string()
}

fn optional_string(value: &Value) -> Option<String> {
    value.as_str().map(ToString::to_string)
}

fn input_of(case: &Value) -> LaunchInput {
    // fixtures 由旧宿主路径采集:program 固定为各族的默认程序(family 字段),
    // custom case 用 team-agent + --profile x。fixture 未单列 input——
    // 它由 family/custom 两个字段唯一决定。
    if case["custom"].as_bool().expect("custom flag") {
        LaunchInput::new("team-agent").args(["--profile", "x"])
    } else {
        LaunchInput::new(case["family"].as_str().expect("family"))
    }
}

fn plan_of(case: &Value) -> aeroric_core::agents::LaunchPlan {
    let input = input_of(case);
    let permission = case["permission"].as_str().expect("permission");
    let facts = &case["facts"];
    let prompt = case["prompt"].as_str().map(ToString::to_string);
    let positional: Vec<String> = match case["family"].as_str() {
        Some("omp") => prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty())
            .map(|prompt| vec!["--".to_string(), prompt.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let opt = |key: &str| optional_string(&facts[key]);
    match case["family"].as_str().expect("family") {
        "claude" => claude_plan(
            &input,
            permission,
            &opt("agent").unwrap_or_default(),
            opt("model").as_deref(),
            opt("effort").as_deref(),
        ),
        "codex" => codex_plan(
            &input,
            permission,
            &as_string(&facts["projectPath"]),
            opt("model").as_deref(),
            opt("effort").as_deref(),
            opt("speed").as_deref(),
        ),
        "dsh" => {
            let patches: Vec<std::path::PathBuf> = facts["patches"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|p| std::path::PathBuf::from(as_string(p)))
                        .collect()
                })
                .unwrap_or_default();
            dsh_plan(
                &input,
                permission,
                facts["interactive"].as_bool().unwrap_or(false),
                std::path::Path::new(&as_string(&facts["home"])),
                &patches,
            )
        }
        "omp" => {
            let agent_env: Vec<(String, String)> = facts["agentEnv"]
                .as_array()
                .expect("omp agentEnv facts")
                .iter()
                .map(|pair| {
                    let pair = pair.as_array().expect("agentEnv pair");
                    (as_string(&pair[0]), as_string(&pair[1]))
                })
                .collect();
            omp_plan(
                &input,
                permission,
                &agent_env,
                &as_string(&facts["projectPath"]),
                opt("model").as_deref(),
                opt("thinking").as_deref(),
                opt("resume").as_deref(),
                &positional,
            )
        }
        other => panic!("unknown family {other}"),
    }
}

#[test]
fn every_frozen_case_matches_the_core_plan_byte_for_byte() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("cases array");
    assert!(cases.len() >= 18, "fixture 表不得缩水:实际 {}", cases.len());

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let plan = plan_of(case);
        let expected = &case["expected"];

        assert_eq!(
            plan.program,
            as_string(&expected["program"]),
            "{id}: program"
        );
        let args: Vec<String> = expected["args"]
            .as_array()
            .expect("args array")
            .iter()
            .map(as_string)
            .collect();
        assert_eq!(plan.args, args, "{id}: args");
        let cwd = expected["cwd"].as_str().map(ToString::to_string);
        assert_eq!(plan.cwd, cwd.map(std::path::PathBuf::from), "{id}: cwd");

        let env = expected["env"].as_object().expect("env object");
        let actual_env: std::collections::BTreeMap<String, String> =
            plan.env.iter().cloned().collect();
        let expected_env: std::collections::BTreeMap<String, String> = env
            .iter()
            .map(|(key, value)| (key.clone(), as_string(value)))
            .collect();
        assert_eq!(actual_env, expected_env, "{id}: env(键集与值逐字节)");
    }
}

/// 家族特有的语义单独钉两条(重放表是按 case 冻结的,这两条钉"为什么"):
/// claude 的 --model 只对 agent=="claude" 注入;omp 的 ultra 封顶 max。
#[test]
fn claude_model_is_gated_by_the_agent_and_omp_ultra_caps_at_max() {
    let input = LaunchInput::new("claude");
    let with_model = claude_plan(&input, "ask", "claude", Some("test-model"), None);
    assert!(with_model
        .args
        .windows(2)
        .any(|pair| pair == ["--model", "test-model"]));

    let custom = claude_plan(&input, "ask", "team-agent", Some("test-model"), None);
    assert!(
        !custom
            .args
            .windows(2)
            .any(|pair| pair == ["--model", "test-model"]),
        "自定义 agent 不吃 --model(旧 add_claude_launch_args 的 agent==\"claude\" 门)"
    );

    let omp = LaunchInput::new("omp");
    let capped = omp_plan(
        &omp,
        "ask",
        &[],
        "/work/proj",
        None,
        Some("ultra"),
        None,
        &[],
    );
    assert!(capped
        .args
        .windows(2)
        .any(|pair| pair == ["--thinking", "max"]));
    assert!(
        !capped.args.iter().any(|arg| arg == "ultra"),
        "ultra 不得向上游发送(OMP 不变量)"
    );
}
