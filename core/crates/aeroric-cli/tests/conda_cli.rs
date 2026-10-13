//! Real attaching CLI -> authenticated host -> scratch-only Conda executable.
//! Never invoke production serve (OS secrets) or installed Conda.
#![cfg(unix)]

use std::process::{Output, Stdio};
use std::time::Duration;

use aeroric_host::dispatcher::HostWiring;
use serde_json::{json, Value};

#[path = "../../aeroric-core/tests/common/conda_fixture.rs"]
mod conda_fixture;
#[path = "../../aeroric-host/tests/host_harness/mod.rs"]
mod host_harness;
use conda_fixture::FakeConda;
use host_harness::Harness;

const METHOD: &str = "infra.detect_conda_environments";

async fn fixture(tag: &str, read_only: bool, unavailable: bool) -> (Harness, FakeConda) {
    let mut fake = None;
    let wiring = |data: &std::path::Path| {
        let owned = FakeConda::new(&data.join("CLI Conda 中文"));
        let mut conda = owned.probe();
        if unavailable {
            conda.candidates = Some(vec![owned.root.join("not-installed")]);
        }
        fake = Some(owned);
        HostWiring {
            conda,
            ..Default::default()
        }
    };
    let host = if read_only {
        Harness::start_read_only_with_wiring(tag, wiring).await
    } else {
        Harness::start_with_wiring(tag, wiring).await
    };
    (host, fake.unwrap())
}

async fn rpc(host: &Harness, method: &str, params: Value) -> Output {
    let input = host.home.join("cli-conda-params.json");
    std::fs::write(&input, serde_json::to_vec(&params).unwrap()).unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_aeroric-cli"));
    command
        .env_clear()
        .env("HOME", &host.home)
        .env("AERORIC_HOME", host.home.join("cli-home"))
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .arg("--data-dir")
        .arg(&host.data_dir)
        .args(["rpc", method, "--params-file"])
        .arg(input);
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .expect("CLI must finish within 10 seconds")
        .expect("CLI subprocess")
}

fn success(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("bare JSON result on stdout")
}

fn refusal(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(code), "{stderr}");
}

#[tokio::test]
async fn attaching_cli_returns_the_owned_executable_result() {
    let (host, fake) = fixture("cc-ok", false, false).await;
    for params in [json!({}), Value::Null] {
        assert_eq!(success(rpc(&host, METHOD, params).await), fake.wire);
    }
    assert_eq!(
        std::fs::read_to_string(&fake.marker).unwrap(),
        "env list --json\nenv list --json\n"
    );
}

#[tokio::test]
async fn cli_cannot_select_programs_or_remote_targets() {
    let (host, fake) = fixture("cc-no", false, false).await;
    for params in [
        json!([]),
        json!({"program":fake.program}),
        json!({"candidates":[fake.program]}),
        json!({"connection":{}}),
        json!({"env":{"CONDA_EXE":fake.program}}),
    ] {
        refusal(rpc(&host, METHOD, params).await, "InvalidArgument");
    }
    refusal(
        rpc(
            &host,
            "infra.detect_remote_conda_environments",
            json!({"connection":{}}),
        )
        .await,
        "UnsupportedCapability",
    );
    assert!(!fake.marker.exists());
}

#[tokio::test]
async fn unavailable_tool_is_a_successful_empty_cli_result() {
    let (host, fake) = fixture("cc-miss", false, true).await;
    assert_eq!(success(rpc(&host, METHOD, json!({})).await), json!([]));
    assert!(!fake.marker.exists());
}

#[tokio::test]
async fn read_only_host_discovery_is_reachable_from_cli() {
    let (host, fake) = fixture("cc-ro", true, false).await;
    assert_eq!(success(rpc(&host, METHOD, json!({})).await), fake.wire);
    assert_eq!(
        std::fs::read_to_string(&fake.marker).unwrap(),
        "env list --json\n"
    );
}
