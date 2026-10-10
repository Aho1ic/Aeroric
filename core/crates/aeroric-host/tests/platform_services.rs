//! P8-05 slice 23: host-owned platform runtime info and runnable-file commands.
//! All fixtures are synthetic; these tests never launch a shell or a script.

use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use aeroric_core::platform::{
    default_shell_command, runnable_file_command, shell_kind_and_label, PlatformRuntimeInfo,
};
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

#[derive(Debug, PartialEq, Eq)]
enum EntrySnapshot {
    Missing,
    File {
        bytes: Vec<u8>,
        #[cfg(unix)]
        mode: u32,
    },
    Directory {
        entries: Vec<OsString>,
        #[cfg(unix)]
        mode: u32,
    },
}

fn snapshots(paths: &[PathBuf]) -> Vec<(PathBuf, EntrySnapshot)> {
    paths
        .iter()
        .map(|path| {
            let entry = match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    EntrySnapshot::Missing
                }
                Err(error) => panic!("cannot inspect scratch fixture {}: {error}", path.display()),
                Ok(metadata) if metadata.is_file() => EntrySnapshot::File {
                    bytes: fs::read(path).expect("read scratch fixture"),
                    #[cfg(unix)]
                    mode: metadata.permissions().mode(),
                },
                Ok(metadata) if metadata.is_dir() => {
                    let mut entries: Vec<_> = fs::read_dir(path)
                        .expect("scratch directory")
                        .map(|entry| entry.expect("scratch directory entry").file_name())
                        .collect();
                    entries.sort();
                    EntrySnapshot::Directory {
                        entries,
                        #[cfg(unix)]
                        mode: metadata.permissions().mode(),
                    }
                }
                Ok(_) => panic!("unexpected fixture type at {}", path.display()),
            };
            (path.clone(), entry)
        })
        .collect()
}

fn fixture_dir(path: &Path) {
    fs::create_dir_all(path).expect("create scratch fixture directory");
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o750)).expect("fixture directory mode");
}

fn write_fixture(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("write scratch fixture");
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o640)).expect("fixture file mode");
}

async fn query(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(reader, writer, &Harness::request(id, method, params)).await
}

fn success_result(reply: Frame) -> Value {
    let Frame::Success(success) = reply else {
        panic!("expected a successful platform reply: {reply:?}");
    };
    success.result
}

/// The scratch tree every case snapshots: home, data dir and a sentinel file that
/// must never move. A platform query is a read of process state, not of the data dir.
fn guarded_paths(harness: &Harness) -> Vec<PathBuf> {
    vec![
        harness.home.clone(),
        harness.data_dir.clone(),
        harness.data_dir.join("sentinel.txt"),
        harness.data_dir.join("projects"),
    ]
}

#[tokio::test]
async fn platform_runtime_info_reports_this_host_without_writes() {
    let harness = Harness::start("platform-info").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    fixture_dir(&harness.data_dir.join("projects"));
    write_fixture(&harness.data_dir.join("sentinel.txt"), b"platform sentinel");
    let paths = guarded_paths(&harness);
    let before = snapshots(&paths);

    let value = success_result(
        query(
            &mut reader,
            &mut writer,
            "platform-info",
            "platform.get_platform_runtime_info",
            json!({}),
        )
        .await,
    );
    let info: PlatformRuntimeInfo =
        serde_json::from_value(value.clone()).expect("the reply is the public PlatformRuntimeInfo");
    assert_eq!(
        info.os,
        std::env::consts::OS,
        "the host's own operating system"
    );
    assert_eq!(
        info.arch,
        std::env::consts::ARCH,
        "the host's own architecture"
    );
    assert_eq!(info.path_separator, std::path::MAIN_SEPARATOR.to_string());
    let (kind, label) = shell_kind_and_label(&default_shell_command().program);
    assert_eq!(info.shell_kind, kind, "the resolved login shell kind");
    assert_eq!(info.shell_label, label);
    assert_eq!(
        info.can_run_shell_scripts,
        info.shell_script_unavailable_reason.is_empty(),
        "the reason and the boolean must agree"
    );
    assert_eq!(snapshots(&paths), before, "platform info is read-only");

    let second = success_result(
        query(
            &mut reader,
            &mut writer,
            "platform-info-again",
            "platform.get_platform_runtime_info",
            json!(null),
        )
        .await,
    );
    assert_eq!(
        second, value,
        "parameters are ignored, the answer is the host's"
    );
    assert_eq!(snapshots(&paths), before);
}

#[tokio::test]
async fn platform_runnable_file_command_matches_the_pure_builder_without_writes() {
    let harness = Harness::start("platform-runnable").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    fixture_dir(&harness.data_dir.join("projects"));
    write_fixture(&harness.data_dir.join("sentinel.txt"), b"runnable sentinel");
    let paths = guarded_paths(&harness);
    let before = snapshots(&paths);

    // Remote commands are POSIX everywhere, so their bytes can be pinned exactly.
    assert_eq!(
        success_result(
            query(
                &mut reader,
                &mut writer,
                "remote-py",
                "platform.build_runnable_file_command",
                json!({
                    "filePath": "/srv/项目/my script.py",
                    "condaPath": "/opt/miniconda env",
                    "condaPythonPath": "/opt/miniconda env/bin/python",
                    "remote": true,
                }),
            )
            .await,
        ),
        json!({
            "command": "'/opt/miniconda env/bin/python' '/srv/项目/my script.py'\r",
            "unavailableReason": null,
        })
    );
    assert_eq!(
        success_result(
            query(
                &mut reader,
                &mut writer,
                "remote-sh",
                "platform.build_runnable_file_command",
                json!({
                    "filePath": "/srv/项目/setup env.sh",
                    "condaPath": "/opt/miniconda env",
                    "condaPythonPath": null,
                    "remote": true,
                }),
            )
            .await,
        ),
        json!({
            "command": "CONDA_PREFIX='/opt/miniconda env' PATH='/opt/miniconda env/bin':\"$PATH\" bash '/srv/项目/setup env.sh'\r",
            "unavailableReason": null,
        })
    );

    // Local commands follow the host's own shell; compare with the same pure builder
    // the handler must be using, including its shell resolution.
    let shell = default_shell_command();
    for (case, file_path, conda_path, conda_python) in [
        (
            "local-py",
            "/tmp/scratch项目/train.py",
            None,
            Some("/opt/py 3/python"),
        ),
        (
            "local-sh",
            "/tmp/scratch项目/setup.sh",
            Some("/opt/conda env"),
            None,
        ),
    ] {
        let expected =
            runnable_file_command(file_path, conda_path, conda_python, false, &shell.program)
                .expect("the pure builder accepts these paths");
        let reply = success_result(
            query(
                &mut reader,
                &mut writer,
                case,
                "platform.build_runnable_file_command",
                json!({
                    "filePath": file_path,
                    "condaPath": conda_path,
                    "condaPythonPath": conda_python,
                    "remote": false,
                }),
            )
            .await,
        );
        assert_eq!(
            reply,
            serde_json::to_value(&expected).expect("builder result"),
            "{case}"
        );
        assert!(
            reply["command"]
                .as_str()
                .is_some_and(|command| command.ends_with('\r')),
            "{case}: runnable commands end with a carriage return"
        );
    }

    // A non-script is a successful answer with a reason, never an error.
    assert_eq!(
        success_result(
            query(
                &mut reader,
                &mut writer,
                "unsupported-extension",
                "platform.build_runnable_file_command",
                json!({"filePath": "/tmp/notes.txt", "remote": true}),
            )
            .await,
        ),
        json!({
            "command": null,
            "unavailableReason": "Only Python or shell scripts can be run.",
        })
    );

    // Absent optional parameters are the same as null ones.
    assert_eq!(
        success_result(
            query(
                &mut reader,
                &mut writer,
                "absent-optionals",
                "platform.build_runnable_file_command",
                json!({"filePath": "/srv/run.py", "remote": true}),
            )
            .await,
        ),
        json!({
            "command": "'python3' '/srv/run.py'\r",
            "unavailableReason": null,
        })
    );

    assert_eq!(
        snapshots(&paths),
        before,
        "building a command writes nothing"
    );
}

#[tokio::test]
async fn platform_runnable_file_command_rejects_invalid_parameters_without_writes() {
    let harness = Harness::start("platform-invalid").await;
    let (mut reader, mut writer) = harness.connect().await;
    let handshake = Harness::handshake(&mut reader, &mut writer, &harness.token, "cli").await;
    assert!(matches!(handshake, Frame::Success(_)), "{handshake:?}");

    fixture_dir(&harness.data_dir.join("projects"));
    write_fixture(&harness.data_dir.join("sentinel.txt"), b"invalid sentinel");
    let paths = guarded_paths(&harness);
    let before = snapshots(&paths);

    let invalid = [
        ("missing-file-path", json!({"remote": false})),
        ("null-file-path", json!({"filePath": null})),
        ("number-file-path", json!({"filePath": 7})),
        ("object-file-path", json!({"filePath": {}})),
        (
            "script-container",
            json!("platform.build_runnable_file_command"),
        ),
        ("null-container", json!(null)),
        (
            "number-conda",
            json!({"filePath": "/srv/x.py", "condaPath": 7}),
        ),
        (
            "bool-conda-python",
            json!({"filePath": "/srv/x.py", "condaPythonPath": true}),
        ),
        (
            "array-conda",
            json!({"filePath": "/srv/x.py", "condaPath": []}),
        ),
        (
            "string-remote",
            json!({"filePath": "/srv/x.py", "remote": "true"}),
        ),
        (
            "null-remote",
            json!({"filePath": "/srv/x.py", "remote": null}),
        ),
        (
            "number-remote",
            json!({"filePath": "/srv/x.py", "remote": 1}),
        ),
        ("control-newline", json!({"filePath": "/srv/a\nb.py"})),
        ("control-carriage", json!({"filePath": "/srv/a\rb.py"})),
        ("control-nul", json!({"filePath": "/srv/a\u{0}b.py"})),
        (
            "control-conda",
            json!({"filePath": "/srv/a.py", "condaPath": "/opt/co\nnda"}),
        ),
        (
            "control-conda-python",
            json!({"filePath": "/srv/a.py", "condaPythonPath": "/opt/py\u{0}"}),
        ),
    ];
    for (case, params) in invalid {
        let reply = query(
            &mut reader,
            &mut writer,
            case,
            "platform.build_runnable_file_command",
            params,
        )
        .await;
        let Frame::Failure(failure) = reply else {
            panic!("{case} must be InvalidArgument: {reply:?}");
        };
        assert_eq!(failure.error.code, ErrorCode::InvalidArgument, "{case}");
        assert_eq!(snapshots(&paths), before, "{case}");
    }

    // The same connection recovers with a valid query.
    assert_eq!(
        success_result(
            query(
                &mut reader,
                &mut writer,
                "recover",
                "platform.build_runnable_file_command",
                json!({"filePath": "/srv/after.py", "remote": true}),
            )
            .await,
        ),
        json!({"command": "'python3' '/srv/after.py'\r", "unavailableReason": null})
    );
    assert_eq!(snapshots(&paths), before, "valid recovery is read-only too");
}
