//! P3-04 第五片金样（其二）：导出层——Markdown 结构、消毒语义与路径闸门。
//!
//! 宿主 `session.rs` 既有的 7 条导出测试（metadata/prompt、tool/thinking 丢弃、
//! name 回退、换行/反引号消毒、validate 三态）经 re-export 跑在同一实现上；
//! 这里在 core API 上钉宿主测试没覆盖的三面：**ExportSource 分派**（明文 Claude／
//! 压缩 DSH）、**200 MiB 上限的具名文案**、**ExportTaskMeta 的 camelCase 输入契约**。

use aeroric_core::sessions::export::{
    export_session_markdown, format_timestamp_ms, sanitize_md_inline, validate_export_output_path,
    write_export_markdown, ExportSource, ExportTaskMeta,
};
use aeroric_core::sessions::{SessionContent, SessionMessage};
use std::fs;
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-session-export-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn meta() -> ExportTaskMeta {
    ExportTaskMeta {
        name: Some("部署演练".to_string()),
        prompt: "把服务部署到预发".to_string(),
        agent: "claude".to_string(),
        created_at: 1_761_800_000_000,
        session_id: Some("sess-`1`".to_string()),
        worktree_branch: Some("wt/a".to_string()),
        base_branch: Some("main".to_string()),
        additions: Some(3),
        deletions: Some(1),
        failure_reason: Some("超时\n换行必须折叠".to_string()),
    }
}

fn msg(role: &str, parts: Vec<SessionContent>) -> SessionMessage {
    SessionMessage {
        role: role.to_string(),
        content: parts,
        message_id: None,
    }
}

fn text(t: &str) -> SessionContent {
    SessionContent::Text {
        text: t.to_string(),
    }
}

#[test]
fn markdown_structure_is_byte_golden() {
    let messages = vec![
        msg("user", vec![text("第一问")]),
        msg(
            "assistant",
            vec![
                text("先跑构建"),
                SessionContent::ToolUse {
                    id: "t1".to_string(),
                    name: "bash".to_string(),
                    input: "cargo build".to_string(),
                },
                SessionContent::Thinking {
                    thinking: "不该出现".to_string(),
                },
            ],
        ),
        msg("assistant", vec![text("第二答\n带换行")]),
        msg(
            "system",
            vec![SessionContent::ToolResult {
                id: "t1".to_string(),
                output: "工具结果不进 md".to_string(),
            }],
        ),
    ];
    let mut out: Vec<u8> = Vec::new();
    write_export_markdown(&mut out, &meta(), &messages).unwrap();
    let md = String::from_utf8(out).unwrap();
    assert_eq!(
        md,
        concat!(
            "# 部署演练\n",
            "\n",
            "## Metadata\n",
            "\n",
            "- **Agent**: claude\n",
            "- **Created**: 2025-10-30T04:53:20Z\n",
            "- **Session ID**: `sess-'1'`\n",
            "- **Branch**: `wt/a` → `main`\n",
            "- **Diff**: +3 / −1\n",
            "- **Failure reason**: 超时 换行必须折叠\n",
            "\n",
            "## Prompt\n",
            "\n",
            "> 把服务部署到预发\n",
            "\n",
            "## Conversation\n",
            "\n",
            "### User\n",
            "\n",
            "第一问\n",
            "\n",
            "### Assistant\n",
            "\n",
            "先跑构建\n",
            "\n",
            "第二答\n",
            "带换行\n",
            "\n",
        ),
    );
}

#[test]
fn fallbacks_keep_titles_and_empty_prompts_honest() {
    let mut out: Vec<u8> = Vec::new();
    let mut m = meta();
    m.name = Some("   ".to_string()); // 空白 name ⇒ 回退 prompt
    m.prompt = "回退标题".to_string();
    write_export_markdown(&mut out, &m, &[]).unwrap();
    let md = String::from_utf8(out).unwrap();
    assert!(md.starts_with("# 回退标题\n"), "{md}");

    let mut out2: Vec<u8> = Vec::new();
    m.prompt = "  \n ".to_string();
    m.name = None;
    write_export_markdown(&mut out2, &m, &[]).unwrap();
    let md2 = String::from_utf8(out2).unwrap();
    assert!(md2.starts_with("# \n"), "空标题保持原折叠语义: {md2}");
    assert!(md2.contains("> _(empty)_"), "空 prompt 必须显式标注");
}

#[test]
fn sanitizer_folds_every_whitespace_and_control_run() {
    assert_eq!(sanitize_md_inline(" a\n\tb\u{7}c "), "a b c");
    assert_eq!(
        format_timestamp_ms(1_761_800_000_000),
        "2025-10-30T04:53:20Z"
    );
    // 只有 chrono 判为越界（不是「早于 epoch」）才退回裸数字（原 `unwrap_or_else` 语义）。
    assert_eq!(format_timestamp_ms(i64::MAX), i64::MAX.to_string());
}

#[test]
fn output_path_gate_rejects_untrusted_save_dialog_results() {
    let dir = scratch("path");
    assert_eq!(
        validate_export_output_path("rel/out.md").unwrap_err(),
        "Output path must be absolute"
    );
    assert_eq!(
        validate_export_output_path(dir.join("out.txt").to_str().unwrap()).unwrap_err(),
        "Output path must end with .md"
    );
    let err =
        validate_export_output_path(dir.join("nope").join("out.md").to_str().unwrap()).unwrap_err();
    assert!(
        err.starts_with("Cannot resolve output directory:"),
        "io 消息体平台相关，前缀是契约: {err}"
    );
    let ok = validate_export_output_path(&dir.join("OUT.MD").to_string_lossy()).unwrap();
    assert_eq!(
        ok,
        dir.canonicalize().unwrap().join("OUT.MD"),
        "父目录必须 canonicalize、文件名保留（.MD 大小写不敏感）"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn oversized_transcript_reports_the_pinned_wording() {
    let dir = scratch("big");
    let path = dir.join("session.jsonl");
    let file = fs::File::create(&path).unwrap();
    file.set_len(200 * 1024 * 1024 + 1).unwrap(); // 稀疏文件，不占真实块
    let out_dir = dir.canonicalize().unwrap();
    let error = export_session_markdown(
        &path.canonicalize().unwrap(),
        out_dir.join("o.md").to_str().unwrap(),
        ExportSource::ClaudeOrCodex,
        &meta(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "Session file is too large to export (200 MB > 200 MB limit)"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn plain_claude_and_compressed_dsh_both_export_via_source_dispatch() {
    let dir = scratch("dispatch");
    let out_dir = dir.canonicalize().unwrap();

    // 明文 Claude：走内容嗅探否 → claude 解析器。
    let claude = dir.join("session.jsonl");
    fs::write(
        &claude,
        concat!(
            "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"部署\"}]}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"好的\"}]}}\n",
        ),
    )
    .unwrap();
    export_session_markdown(
        &claude.canonicalize().unwrap(),
        out_dir.join("claude.md").to_str().unwrap(),
        ExportSource::ClaudeOrCodex,
        &meta(),
    )
    .unwrap();
    let md = fs::read_to_string(out_dir.join("claude.md")).unwrap();
    assert!(md.contains("### User\n\n部署\n"), "{md}");
    assert!(md.contains("### Assistant\n\n好的\n"), "{md}");

    // 压缩 DSH：`.jsonl.zstd` 后缀判定 → 解码 → Dsh 显式解析器。
    let dsh = dir.join("session.jsonl.zstd");
    let transcript = concat!(
        "{\"type\":\"session\",\"version\":0,\"id\":\"s1\",\"createdAt\":1}\n",
        "{\"type\":\"user/message\",\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"dsh 内联\"}]}}\n",
    );
    fs::write(&dsh, zstd::encode_all(transcript.as_bytes(), 0).unwrap()).unwrap();
    export_session_markdown(
        &dsh.canonicalize().unwrap(),
        out_dir.join("dsh.md").to_str().unwrap(),
        ExportSource::Dsh,
        &meta(),
    )
    .unwrap();
    let dmd = fs::read_to_string(out_dir.join("dsh.md")).unwrap();
    assert!(dmd.contains("dsh 内联"), "{dmd}");

    // 未来版本的 dsh header 必须响亮拒绝（逐字文案随第三片一起钉过，这里保导出面）。
    let future = dir.join("future.jsonl.zstd");
    let t2 = concat!(
        "{\"type\":\"session\",\"version\":99,\"id\":\"s2\",\"createdAt\":1}\n",
        "{\"type\":\"user/message\",\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"x\"}]}}\n",
    );
    fs::write(&future, zstd::encode_all(t2.as_bytes(), 0).unwrap()).unwrap();
    let err = export_session_markdown(
        &future.canonicalize().unwrap(),
        out_dir.join("f.md").to_str().unwrap(),
        ExportSource::Dsh,
        &meta(),
    )
    .unwrap_err();
    assert!(
        err.contains("please upgrade Aeroric"),
        "fail-loud 文案必须原样: {err}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn task_meta_deserializes_from_camel_case_wire_shape() {
    let m: ExportTaskMeta = serde_json::from_str(
        r#"{"name":null,"prompt":"p","agent":"a","createdAt":5,"sessionId":null,
            "worktreeBranch":null,"baseBranch":null,"additions":null,"deletions":null,
            "failureReason":null}"#,
    )
    .unwrap();
    assert_eq!(m.created_at, 5);
    assert!(m.name.is_none());
}
