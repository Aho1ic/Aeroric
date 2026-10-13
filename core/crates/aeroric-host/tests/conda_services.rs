//! Slice 111: authenticated UDS -> owned fake executable -> legacy bare array.
#![cfg(unix)]

#[path = "../../aeroric-core/tests/common/conda_fixture.rs"]
mod conda_fixture;
mod host_harness;

use aeroric_core::infrastructure::conda_face::CondaProbe;
use aeroric_host::dispatcher::HostWiring;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use conda_fixture::FakeConda;
use host_harness::Harness;
use serde_json::{json, Value};

const METHOD: &str = "infra.detect_conda_environments";

async fn fixture(
    tag: &str,
    read_only: bool,
    configure: impl FnOnce(&FakeConda) -> CondaProbe,
) -> (Harness, FakeConda) {
    let mut fake = None;
    let wiring = |data: &std::path::Path| {
        let owned = FakeConda::new(&data.join("Conda fixture"));
        let conda = configure(&owned);
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

async fn call(host: &Harness, method: &str, params: Value) -> Frame {
    let (mut reader, mut writer) = host.connect().await;
    assert!(matches!(
        Harness::handshake(&mut reader, &mut writer, &host.token, "conda-test").await,
        Frame::Success(_)
    ));
    Harness::call(
        &mut reader,
        &mut writer,
        &Harness::request("conda", method, params),
    )
    .await
}

fn success(frame: Frame) -> Value {
    match frame {
        Frame::Success(reply) => reply.result,
        other => panic!("expected discovery result, got {other:?}"),
    }
}

fn failure(frame: Frame, expected: ErrorCode) {
    match frame {
        Frame::Failure(reply) => assert_eq!(reply.error.code, expected, "{reply:?}"),
        other => panic!("expected refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn authenticated_discovery_runs_injected_tool_and_preserves_wire() {
    let (host, fake) = fixture("conda-ok", false, FakeConda::probe).await;
    assert_eq!(success(call(&host, METHOD, json!({})).await), fake.wire);
    assert_eq!(
        std::fs::read_to_string(&fake.marker).unwrap(),
        "env list --json\n"
    );
}

#[tokio::test]
async fn null_and_omitted_params_are_no_argument_queries() {
    let (host, fake) = fixture("conda-null", false, FakeConda::probe).await;
    assert_eq!(success(call(&host, METHOD, Value::Null).await), fake.wire);
    let (mut reader, mut writer) = host.connect().await;
    assert!(matches!(
        Harness::handshake(&mut reader, &mut writer, &host.token, "conda-null").await,
        Frame::Success(_)
    ));
    // Decode a real envelope without params, not an invented alternate shape.
    let frame = aeroric_protocol::envelope::decode_line(
        br#"{"v":1,"id":"no-params","method":"infra.detect_conda_environments"}"#,
    )
    .unwrap();
    assert_eq!(
        success(Harness::call(&mut reader, &mut writer, &frame).await),
        fake.wire
    );
    assert_eq!(
        std::fs::read_to_string(&fake.marker)
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[tokio::test]
async fn authenticated_read_only_host_may_run_discovery() {
    let (host, fake) = fixture("conda-ro", true, FakeConda::probe).await;
    assert_eq!(success(call(&host, METHOD, json!({})).await), fake.wire);
    assert!(
        fake.marker.exists(),
        "discovery does not require a business writer"
    );
}

#[tokio::test]
async fn malformed_or_unknown_arguments_are_refused_before_probe() {
    let (host, fake) = fixture("conda-bad", false, FakeConda::probe).await;
    for params in [
        json!([]),
        json!(false),
        json!(1),
        json!(""),
        json!({"extra":null}),
        json!({"candidates":[fake.program]}),
        json!({"env":{"CONDA_EXE":fake.program}}),
        json!({"connection":{"host":"untrusted"}}),
        json!({"target":{"kind":"ssh","connectionId":"untrusted","path":"/project"}}),
    ] {
        failure(
            call(&host, METHOD, params).await,
            ErrorCode::InvalidArgument,
        );
    }
    assert!(!fake.marker.exists());
}

#[tokio::test]
async fn unauthenticated_query_cannot_probe() {
    let (host, fake) = fixture("conda-auth", false, FakeConda::probe).await;
    let (mut reader, mut writer) = host.connect().await;
    failure(
        Harness::call(
            &mut reader,
            &mut writer,
            &Harness::request("pre-auth", METHOD, json!({})),
        )
        .await,
        ErrorCode::Unauthenticated,
    );
    assert!(!fake.marker.exists());
}

#[tokio::test]
async fn unavailable_permission_denied_and_bad_output_keep_empty_fallback() {
    for mode in ["missing", "permission", "invalid", "empty", "disabled"] {
        let (host, fake) = fixture("conda-empty", false, |fake| {
            let mut probe = match mode {
                "permission" => fake.probe_with_output(&fake.raw, 13),
                "invalid" => fake.probe_with_output("not JSON", 0),
                "empty" => fake.probe_with_output(r#"{"envs":[]}"#, 0),
                _ => fake.probe(),
            };
            if mode == "missing" {
                probe.candidates = Some(vec![fake.root.join("not-installed")]);
            } else if mode == "disabled" {
                probe.candidates = Some(Vec::new());
            }
            probe
        })
        .await;
        assert_eq!(
            success(call(&host, METHOD, json!({})).await),
            json!([]),
            "{mode}"
        );
        assert_eq!(
            fake.marker.exists(),
            !matches!(mode, "missing" | "disabled")
        );
    }
}

#[tokio::test]
async fn independent_hosts_do_not_share_probe_or_child_environment() {
    let (first, first_fake) = fixture("conda-one", false, FakeConda::probe).await;
    let (second, second_fake) = fixture("conda-two", false, FakeConda::probe).await;
    assert_ne!(first_fake.wire, second_fake.wire);
    let (a, b) = tokio::join!(
        call(&first, METHOD, json!({})),
        call(&second, METHOD, json!({}))
    );
    assert_eq!(success(a), first_fake.wire);
    assert_eq!(success(b), second_fake.wire);
    assert_eq!(
        std::fs::read_to_string(&first_fake.marker).unwrap(),
        "env list --json\n"
    );
    assert_eq!(
        std::fs::read_to_string(&second_fake.marker).unwrap(),
        "env list --json\n"
    );
}

#[tokio::test]
async fn ssh_twin_stays_unserved_without_local_probe() {
    let (host, fake) = fixture("conda-ssh", false, FakeConda::probe).await;
    failure(
        call(
            &host,
            "infra.detect_remote_conda_environments",
            json!({"connection":{}}),
        )
        .await,
        ErrorCode::UnsupportedCapability,
    );
    assert!(!fake.marker.exists());
}
