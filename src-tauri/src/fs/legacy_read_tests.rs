//! Before/after compatibility for actual legacy text and image query commands.
//! Every child runs only its own case in scratch cwd/HOME with an empty PATH.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;

use base64::Engine;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CASE: &str = "AERORIC_LEGACY_READ_CASE";
const ROOT: &str = "AERORIC_LEGACY_READ_ROOT";
const OUTSIDE: &str = "Path is outside the allowed directory";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only this scratch tree");
    }
}

fn isolated(name: &str, run: impl FnOnce(&Path)) {
    if std::env::var(CASE).as_deref() == Ok(name) {
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        assert_eq!(PathBuf::from(std::env::var_os("HOME").unwrap()), root);
        assert_eq!(
            PathBuf::from(std::env::var_os("PATH").unwrap()),
            root.join("bin")
        );
        assert_eq!(
            std::env::current_dir().unwrap().canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
        run(&root);
        assert!(
            !root.join("data").exists(),
            "no settings/history initialization"
        );
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), 0);
        return;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        Scratch(PathBuf::from("/tmp").join(format!("legacy-read-{}-{nonce}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    fs::create_dir(root.0.join("bin")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("fs::legacy_read_tests::{name}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(CASE, name)
        .env(ROOT, &root.0)
        .env("HOME", &root.0)
        .env("USERPROFILE", &root.0)
        .env("AERORIC_HOME", root.0.join("data"))
        .env("PATH", root.0.join("bin"))
        .env("SHELL", "/bin/sh")
        .env("TMPDIR", &root.0)
        .current_dir(&root.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !timed_out
            && output.status.success()
            && stdout.contains("running 1 test")
            && stdout.contains("1 passed; 0 failed; 0 ignored"),
        "{name}: {}, timeout={timed_out}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    mode: u32,
    modified: SystemTime,
    bytes: Option<Vec<u8>>,
    link: Option<PathBuf>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, all: &mut BTreeMap<PathBuf, Entry>) {
        let meta = fs::symlink_metadata(path).unwrap();
        all.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            Entry {
                mode: meta.permissions().mode(),
                modified: meta.modified().unwrap(),
                bytes: meta.is_file().then(|| fs::read(path).unwrap()),
                link: meta
                    .file_type()
                    .is_symlink()
                    .then(|| fs::read_link(path).unwrap()),
            },
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), all);
            }
        }
    }
    let mut all = BTreeMap::new();
    visit(root, root, &mut all);
    all
}

fn text(path: &Path, project: &Path, strict: bool) -> Result<String, String> {
    let path = path.to_str().unwrap().to_string();
    let project = project.to_str().unwrap().to_string();
    let rt = runtime();
    if strict {
        rt.block_on(super::read_project_file_content(path, project))
    } else {
        rt.block_on(super::read_file_content(path, project))
    }
}

fn image(path: &Path, project: &Path) -> Result<serde_json::Value, String> {
    let result = runtime().block_on(super::read_image_preview(
        path.to_str().unwrap().to_string(),
        project.to_str().unwrap().to_string(),
    ))?;
    serde_json::to_value(result).map_err(|error| error.to_string())
}

fn sized(path: &Path, bytes: u64) {
    fs::File::create(path).unwrap().set_len(bytes).unwrap();
}

fn raw_text_error(path: &Path) -> String {
    fs::File::open(path)
        .and_then(|file| std::io::BufReader::new(file).read_to_string(&mut String::new()))
        .unwrap_err()
        .to_string()
}

fn raw_image_error(path: &Path) -> String {
    fs::File::open(path)
        .and_then(|file| std::io::BufReader::new(file).read_to_end(&mut Vec::new()))
        .unwrap_err()
        .to_string()
}

#[test]
fn text_keeps_empty_bom_crlf_unicode_nul_and_file_root() {
    isolated(
        "text_keeps_empty_bom_crlf_unicode_nul_and_file_root",
        |root| {
            let project = root.join(" project 中 ");
            fs::create_dir(&project).unwrap();
            let cases = [
                ("empty.txt", ""),
                ("exact.txt", "\u{feff}first\r\n第二行\0tail"),
            ];
            for (name, contents) in cases {
                let path = project.join(name);
                fs::write(&path, contents).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            }
            let before = snapshot(root);
            for (name, expected) in cases {
                let path = project.join(name);
                for strict in [false, true] {
                    assert_eq!(text(&path, &project, strict).unwrap(), expected);
                    assert_eq!(text(&path, &path, strict).unwrap(), expected);
                }
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn text_keeps_raw_encoding_io_and_guard_errors() {
    isolated("text_keeps_raw_encoding_io_and_guard_errors", |root| {
        let project = root.join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(project.join("directory")).unwrap();
        fs::write(project.join("invalid.txt"), [0xff, 0xfe]).unwrap();
        fs::write(root.join("outside.txt"), b"scratch outside").unwrap();
        let before = snapshot(root);
        for name in ["invalid.txt", "directory", "missing.txt", "nul\0.txt"] {
            let path = project.join(name);
            let expected = raw_text_error(&path);
            assert_eq!(text(&path, &project, false).err(), Some(expected));
        }
        for strict in [false, true] {
            assert_eq!(
                text(&root.join("outside.txt"), &project, strict)
                    .err()
                    .as_deref(),
                Some(OUTSIDE)
            );
            assert_eq!(
                text(Path::new("relative.txt"), &project, strict)
                    .err()
                    .as_deref(),
                Some("Path must be absolute")
            );
        }
        assert_eq!(
            text(&project.join("missing.txt"), &project, true)
                .err()
                .as_deref(),
            Some(OUTSIDE)
        );
        assert_eq!(snapshot(root), before);
    });
}

#[test]
fn text_two_mib_edge_and_size_before_utf8_are_unchanged() {
    isolated(
        "text_two_mib_edge_and_size_before_utf8_are_unchanged",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let max = project.join("max.txt");
            let too_large = project.join("large.txt");
            sized(&max, 2 * 1024 * 1024);
            fs::write(&too_large, [0xff]).unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&too_large)
                .unwrap()
                .set_len(2 * 1024 * 1024 + 1)
                .unwrap();
            let before = snapshot(root);
            for strict in [false, true] {
                let contents = text(&max, &project, strict).unwrap();
                assert_eq!(contents.len(), 2 * 1024 * 1024);
                assert!(contents.bytes().all(|byte| byte == 0));
                assert_eq!(
                    text(&too_large, &project, strict).err().as_deref(),
                    Some("File too large (2.0 MB)")
                );
            }
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn image_catalog_standard_base64_and_exact_wire_are_legacy() {
    isolated(
        "image_catalog_standard_base64_and_exact_wire_are_legacy",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let cases = [
                ("photo.png", "image/png"),
                ("upper.PNG", "image/png"),
                ("photo.jpg", "image/jpeg"),
                ("upper.JPEG", "image/jpeg"),
                ("photo.gif", "image/gif"),
                ("photo.webp", "image/webp"),
                ("photo.bmp", "image/bmp"),
                ("photo.svg", "image/svg+xml"),
            ];
            for (name, _) in cases {
                fs::write(project.join(name), [0xfb, 0xff, 0, 1]).unwrap();
            }
            fs::write(project.join("empty.png"), b"").unwrap();
            let before = snapshot(root);
            for (name, mime) in cases {
                assert_eq!(
                    image(&project.join(name), &project).unwrap(),
                    serde_json::json!({
                        "dataUrl": format!("data:{mime};base64,+/8AAQ=="),
                        "mimeType": mime, "byteLength": 4,
                    })
                );
            }
            assert_eq!(
                image(&project.join("empty.png"), &project).unwrap(),
                serde_json::json!({
                    "dataUrl": "data:image/png;base64,", "mimeType": "image/png", "byteLength": 0,
                })
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn image_keeps_mime_before_io_size_and_original_guard_order() {
    isolated(
        "image_keeps_mime_before_io_size_and_original_guard_order",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            fs::create_dir(project.join("directory.png")).unwrap();
            for name in [
                "file.ico",
                "file.tiff",
                "file.avif",
                "file.txt",
                "no-extension",
            ] {
                fs::write(project.join(name), b"not decoded pixels").unwrap();
            }
            sized(&project.join("large.ico"), 10 * 1024 * 1024 + 1);
            let before = snapshot(root);
            for name in [
                "file.ico",
                "file.tiff",
                "file.avif",
                "file.txt",
                "no-extension",
                "absent.ico",
                "large.ico",
            ] {
                assert_eq!(
                    image(&project.join(name), &project).err().as_deref(),
                    Some("Unsupported image format")
                );
            }
            for name in ["absent.png", "directory.png", "nul\0.png"] {
                let path = project.join(name);
                assert_eq!(image(&path, &project).err(), Some(raw_image_error(&path)));
            }
            assert_eq!(
                image(Path::new("relative.ico"), &project).err().as_deref(),
                Some("Path must be absolute")
            );
            assert_eq!(
                image(&root.join("outside.ico"), &project).err().as_deref(),
                Some(OUTSIDE)
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn image_ten_mib_edge_is_not_reduced_to_the_rpc_frame_limit() {
    isolated(
        "image_ten_mib_edge_is_not_reduced_to_the_rpc_frame_limit",
        |root| {
            let project = root.join("project");
            fs::create_dir(&project).unwrap();
            let max = project.join("max.png");
            let too_large = project.join("large.png");
            let limit = 10 * 1024 * 1024;
            sized(&max, limit);
            sized(&too_large, limit + 1);
            let before = snapshot(root);
            let result = image(&max, &project).unwrap();
            assert_eq!(result["byteLength"], limit);
            assert_eq!(result["mimeType"], "image/png");
            let encoded = result["dataUrl"]
                .as_str()
                .unwrap()
                .strip_prefix("data:image/png;base64,")
                .unwrap();
            assert_eq!(encoded.len(), 13_981_016);
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap();
            assert_eq!(decoded.len(), limit as usize);
            assert!(decoded.iter().all(|byte| *byte == 0));
            assert_eq!(
                image(&too_large, &project).err().as_deref(),
                Some("Image too large (10.0 MB)")
            );
            assert_eq!(snapshot(root), before);
        },
    );
}

#[test]
fn image_mime_uses_the_canonical_or_unresolved_leaf_selected_by_the_guard() {
    isolated(
        "image_mime_uses_the_canonical_or_unresolved_leaf_selected_by_the_guard",
        |root| {
            let project = root.join("project");
            let outside = root.join("outside");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(project.join("actual.PNG"), b"inside").unwrap();
            fs::write(outside.join("payload"), b"outside").unwrap();
            fs::write(outside.join("actual.png"), b"outside png").unwrap();
            symlink(
                project.join("actual.PNG"),
                project.join("looks-like-text.txt"),
            )
            .unwrap();
            symlink(outside.join("payload"), project.join("external.svg")).unwrap();
            symlink(outside.join("actual.png"), project.join("external.txt")).unwrap();
            let before = snapshot(root);
            assert_eq!(
                image(&project.join("looks-like-text.txt"), &project).unwrap(),
                serde_json::json!({
                    "dataUrl": "data:image/png;base64,aW5zaWRl", "mimeType": "image/png", "byteLength": 6,
                })
            );
            assert_eq!(
                image(&project.join("external.svg"), &project).unwrap(),
                serde_json::json!({
                    "dataUrl": "data:image/svg+xml;base64,b3V0c2lkZQ==", "mimeType": "image/svg+xml", "byteLength": 7,
                })
            );
            assert_eq!(
                image(&project.join("external.txt"), &project)
                    .err()
                    .as_deref(),
                Some("Unsupported image format")
            );
            assert_eq!(snapshot(root), before);
        },
    );
}
