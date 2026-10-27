//! P3-04 第二片金样：四族发现路径层（Claude／Codex roots ＋ DSH／OMP 编码与根）。
//!
//! 判据全部来自 `e3836e62:docs/native/extraction/sessions-corpus.md` §一/§二 的当日普查
//! 与 `sessions.md` 手写约束 2/5/8：
//! - 编码表按上游规则逐字符钉死（含 UTF-16 `~XXXX`，corpus 实测 同=`~540C`、步=`~6B65`）；
//! - **家族不能从目录名推断**（21 个不以 `_codex` 结尾的档案放着 221 份 rollout）——
//!   roots 的族别由 `is_codex` 入参决定，测试里没有任何按后缀猜族的代码；
//! - 档案根／`.tmp`／`cache` 不在任何根里 ⇒ must-reject 用例把这条边界钉住；
//! - Claude 递归收任意 `*.jsonl`（含嵌套 subagent 转录）是现存行为，逐字保留，
//!   归属裁决留给 `grill-me [sessions]`（corpus §一·硬约束 ②）。

use aeroric_core::sessions::discovery::{
    append_custom_agent_session_roots, claude_sessions_dir_for_agent_in, collect_jsonl_files,
    collect_session_files, collect_session_files_from_roots, encode_claude_project_path,
};
use aeroric_core::sessions::dsh::{
    dsh_encode_segment, dsh_project_key, dsh_transcript_in, DSH_SESSION_FORMAT_VERSION,
    DSH_TRANSCRIPT_RAW, DSH_TRANSCRIPT_ZSTD,
};
use aeroric_core::sessions::omp::{
    omp_external_history_session_roots, omp_managed_session_roots, omp_project_dir_name,
    omp_session_id_from_file_name,
};
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-session-discovery-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"{\"type\":\"noop\"}\n").unwrap();
}

#[test]
fn claude_path_encoding_is_pinned() {
    assert_eq!(
        encode_claude_project_path("/home/u/Documents/会话目录/proj.a"),
        "-home-u-Documents------proj-a"
    );
    assert_eq!(encode_claude_project_path("a-b-c"), "a-b-c");
    assert_eq!(encode_claude_project_path(""), "");
}

#[test]
fn dsh_project_key_mirrors_upstream_utf16_escaping() {
    // corpus §一·目录形态表里 DSH 用 `--<绝对路径编码>--`；非 ASCII 按 UTF-16 码元转义。
    // （真实语料里这段是用户目录名，测试换中性词；码元规则与 同=`~540C` 一次实测同源。）
    assert_eq!(
        dsh_project_key("/tmp/会话目录"),
        "--tmp-~4F1A~8BDD~76EE~5F55--"
    );
    assert_eq!(dsh_project_key("C:\\a:b"), "--C-a-b--");
    // 分隔符折叠成一个 `-`，前导 `-` 被剥掉。
    assert_eq!(dsh_project_key("/a//b"), "--a-b--");
    // 全不安全字符 → 只剩转义；空前导剥离后为空的分支用 "///" 触发不了（转义非空），
    // 真正的空 slug 是空串输入。
    assert_eq!(dsh_project_key(""), "--root--");
    // 251 截断（按字节＝JS slice 语义，因为此处 readable 全 ASCII）。
    let long = "/".to_string() + &"a".repeat(300);
    let key = dsh_project_key(&long);
    assert_eq!(key.len(), 2 + 251 + 2, "wrapped in -- -- after truncation");
    assert!(key.starts_with("--a"));
    // `~` 不是安全字符：必须转义，防止拼出伪造的转义段。
    assert_eq!(dsh_project_key("/~x"), "--~007Ex--");
}

#[test]
fn dsh_encode_segment_special_cases() {
    assert_eq!(dsh_encode_segment(""), "");
    assert_eq!(dsh_encode_segment("."), "~002E");
    assert_eq!(dsh_encode_segment(".."), "~002E~002E");
    assert_eq!(dsh_encode_segment("0198c7a2-7f4a"), "0198c7a2-7f4a");
    assert_eq!(dsh_encode_segment("a~b"), "a~007Eb");
    assert_eq!(dsh_encode_segment("会话"), "~4F1A~8BDD");
}

#[test]
fn dsh_transcript_prefers_raw_over_stale_zstd() {
    // 手写约束 8：从 zstd 切回 none 后旧压缩产物仍在——读侧两形都试、明文优先。
    let dir = scratch("dsh-transcript");
    let session = dir.join("sess");
    fs::create_dir_all(&session).unwrap();
    assert_eq!(dsh_transcript_in(&session), None);
    fs::write(session.join(DSH_TRANSCRIPT_ZSTD), b"z").unwrap();
    assert_eq!(
        dsh_transcript_in(&session),
        Some(session.join(DSH_TRANSCRIPT_ZSTD))
    );
    fs::write(session.join(DSH_TRANSCRIPT_RAW), b"r").unwrap();
    assert_eq!(
        dsh_transcript_in(&session),
        Some(session.join(DSH_TRANSCRIPT_RAW))
    );
    assert_eq!(DSH_SESSION_FORMAT_VERSION, 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn omp_dir_name_three_branches() {
    let root = scratch("omp-dir");
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let home_canon = home.canonicalize().unwrap_or_else(|_| home.clone());
    let temp_root_canon = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());

    // ① home 相对：`-<rel>`，`/ \ :` → `-`。
    let rel = home_canon.join("Downloads").join("proj-a");
    fs::create_dir_all(&rel).unwrap();
    assert_eq!(
        omp_project_dir_name(Some(&home_canon), &rel),
        "-Downloads-proj-a"
    );
    // home 本身 → "-"。
    assert_eq!(omp_project_dir_name(Some(&home_canon), &home_canon), "-");
    // ② temp 相对：`-tmp<rel>`；temp 根本身 → "-tmp"。
    let in_temp = temp_root_canon.join("sub-x");
    fs::create_dir_all(&in_temp).unwrap();
    assert_eq!(
        omp_project_dir_name(Some(&home_canon), &in_temp),
        "-tmpsub-x"
    );
    assert_eq!(
        omp_project_dir_name(Some(&home_canon), &temp_root_canon),
        "-tmp"
    );
    // ③ 兜底绝对：`--<绝对去前导斜杠>--`（不存在的目录走 canonicalize 失败回退）。
    let elsewhere = PathBuf::from("/somewhere/else");
    assert_eq!(
        omp_project_dir_name(Some(&home_canon), &elsewhere),
        "--somewhere-else--"
    );
    // home=None 时跳过 home 分支，其他规则不变。
    assert_eq!(omp_project_dir_name(None, &elsewhere), "--somewhere-else--");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn omp_session_id_from_file_name_shape() {
    let path = Path::new(
        "/home/u/.omp/agent-homes/omp/sessions/-proj/2026-09-06T10-30-00_0198c7a2-7f4a-7d3e-9f2a-3b6c5d4e4f50.jsonl",
    );
    assert_eq!(
        omp_session_id_from_file_name(path),
        Some("0198c7a2-7f4a-7d3e-9f2a-3b6c5d4e4f50".to_owned())
    );
    assert_eq!(omp_session_id_from_file_name(Path::new("a.jsonl")), None);
    assert_eq!(omp_session_id_from_file_name(Path::new("a_.jsonl")), None);
    assert_eq!(omp_session_id_from_file_name(Path::new("x_1.txt")), None);
}

fn fake_home() -> PathBuf {
    let home = scratch("roots");
    fs::create_dir_all(
        home.join(".aeroric")
            .join("agent-homes")
            .join("sunset_codex"),
    )
    .unwrap();
    fs::create_dir_all(home.join(".aeroric").join("agent-homes").join("liwan")).unwrap();
    home
}

#[test]
fn codex_roots_include_project_home_and_every_agent_home() {
    let home = fake_home();
    let project = home.join("workspace").join("sunset");
    let roots = aeroric_core::sessions::discovery::codex_sessions_roots(
        Some(&home),
        &project.to_string_lossy(),
    );
    assert_eq!(roots[0], project.join(".codex").join("sessions"));
    assert!(roots.contains(&home.join(".codex").join("sessions")));
    // 221 份 rollout 住在**不以 _codex 结尾**的档案里——所以扫描必须覆盖全部
    // agent-homes/*/sessions，而不是按档案名猜族。
    assert!(roots.contains(
        &home
            .join(".aeroric")
            .join("agent-homes")
            .join("liwan")
            .join("sessions")
    ));
    // 幂等去重：同一 root 已在列表里不得重复追加。
    let before = roots.len();
    let mut again = roots.clone();
    append_custom_agent_session_roots(&mut again, &home, &project.to_string_lossy(), true);
    assert_eq!(again.len(), before);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn claude_roots_use_encoded_project_dir_under_every_home() {
    let home = fake_home();
    let project = home.join("workspace").join("sunset");
    let encoded = encode_claude_project_path(&project.to_string_lossy());
    let roots = aeroric_core::sessions::discovery::claude_sessions_dirs_for_project(
        Some(&home),
        &project.to_string_lossy(),
    );
    assert_eq!(
        roots[0],
        home.join(".claude").join("projects").join(&encoded)
    );
    assert!(roots.contains(
        &home
            .join(".aeroric")
            .join("agent-homes")
            .join("sunset_codex")
            .join("projects")
            .join(&encoded)
    ));
    assert_eq!(
        aeroric_core::sessions::discovery::claude_sessions_dirs_for_project(None, "x"),
        Vec::<PathBuf>::new()
    );
    // 内建 claude → ~/.claude；自定义档案 → agent-homes/{name}（name 由宿主注入，
    // core 不复述 agent 名录语义——手写约束 2 的唯一真相关系）。
    assert_eq!(
        claude_sessions_dir_for_agent_in(&home, None, "/p"),
        Some(home.join(".claude").join("projects").join("-p"))
    );
    assert_eq!(
        claude_sessions_dir_for_agent_in(&home, Some("liwan"), "/p"),
        Some(
            home.join(".aeroric")
                .join("agent-homes")
                .join("liwan")
                .join("projects")
                .join("-p")
        )
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn rollout_filter_and_must_reject_shapes() {
    let home = fake_home();
    let root = home.join("codex-root");
    // 正形状：日期分桶里的 rollout-*.jsonl（含嵌套）。
    touch(
        &root
            .join("2026")
            .join("09")
            .join("06")
            .join("rollout-a.jsonl"),
    );
    // 噪声形状：非 rollout 名、非 jsonl、以及 corpus §二 的档案根直躺转录。
    touch(&root.join("session.jsonl"));
    touch(&root.join("readme.txt"));
    touch(
        &home
            .join(".aeroric")
            .join("agent-homes")
            .join("stray-usage.jsonl"),
    );
    let mut out = Vec::new();
    collect_session_files(&root, &mut out);
    assert_eq!(
        out,
        vec![root
            .join("2026")
            .join("09")
            .join("06")
            .join("rollout-a.jsonl")]
    );
    // from_roots 只是对多根的并集收集。
    assert_eq!(
        collect_session_files_from_roots(std::slice::from_ref(&root)),
        out
    );
    // Claude 侧递归收任意 *.jsonl——包括嵌套 subagent 转录（现存行为，逐字保留；
    // 「算不算一个会话」归 grill-me [sessions] 裁决）。
    let croot = home.join("claude-projects");
    touch(
        &croot
            .join("uuid-1")
            .join("subagents")
            .join("worker-9.jsonl"),
    );
    touch(&croot.join("note.md"));
    let mut cfiles = Vec::new();
    collect_jsonl_files(&croot, &mut cfiles);
    assert_eq!(
        cfiles,
        vec![croot
            .join("uuid-1")
            .join("subagents")
            .join("worker-9.jsonl")]
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn omp_root_primitives_match_host_composition() {
    let home = fake_home();
    // 托管扫描不做 is_dir 过滤：档案存在而 sessions 不存在时根照列（时序钉）。
    let managed = omp_managed_session_roots(&home);
    assert_eq!(managed.len(), 2);
    assert!(managed.contains(
        &home
            .join(".aeroric")
            .join("agent-homes")
            .join("sunset_codex")
            .join("sessions")
    ));
    // 外部历史两支：默认根恒在＋profiles 里只认目录（stray.txt 不得成根）。
    fs::create_dir_all(
        home.join(".omp")
            .join("profiles")
            .join("sota")
            .join("agent"),
    )
    .unwrap();
    fs::write(home.join(".omp").join("profiles").join("stray.txt"), b"x").unwrap();
    let external = omp_external_history_session_roots(&home);
    assert_eq!(
        external,
        vec![
            home.join(".omp").join("agent").join("sessions"),
            home.join(".omp")
                .join("profiles")
                .join("sota")
                .join("agent")
                .join("sessions"),
        ]
    );
    // 无 profiles 目录：不 panic，只剩默认外部根一项。
    let bare = scratch("omp-bare");
    assert_eq!(
        omp_external_history_session_roots(&bare),
        vec![bare.join(".omp").join("agent").join("sessions")]
    );
    assert!(omp_managed_session_roots(&bare).is_empty());
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&bare);
}
