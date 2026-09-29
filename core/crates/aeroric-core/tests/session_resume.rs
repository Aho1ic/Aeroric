//! P3-04 第五片金样（其一）：恢复层——native resume 判定、会话 id 兜底与 recover 阶梯。
//!
//! 判据来源：P2-04 Q43 移交项「`session_supports_native_resume` 的真实判据属本任务；
//! core 只保留“解析不出 session 就拒绝续跑”这条判定」＋ `sessions.md` 手写约束 6/7。
//! 宿主侧既有的 4 条 resume/adopt 测试（paginated 拒绝、legacy 保留、无标记保留、
//! ensure 文案）经 re-export 跑在同一实现上，这里在 core API 上钉边界与兜底序。

use aeroric_core::sessions::resume::{
    ensure_session_can_be_adopted, is_uuid_like, recover_session, resolve_session_id_from_file,
    session_file_supports_native_resume, session_matches_project, session_started_at_ms,
};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-session-resume-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_jsonl(path: &Path, values: &[serde_json::Value]) {
    let mut file = fs::File::create(path).unwrap();
    for value in values {
        writeln!(file, "{value}").unwrap();
    }
}

#[test]
fn paginated_codex_is_the_only_rejected_resume_shape() {
    let dir = scratch("native");
    let meta = |mode: Option<&str>| json!({"type":"session_meta","payload":{"id":"c1","history_mode":mode}});

    let paginated = dir.join("paginated.jsonl");
    write_jsonl(&paginated, &[meta(Some("paginated"))]);
    assert_eq!(
        session_file_supports_native_resume(&paginated, true),
        Ok(false)
    );
    // `eq_ignore_ascii_case`：大小写方言同样拒绝。
    let shout = dir.join("shout.jsonl");
    write_jsonl(&shout, &[meta(Some("PAGINATED"))]);
    assert_eq!(session_file_supports_native_resume(&shout, true), Ok(false));

    // 旧版可能没有 history_mode；其它值保持历史的原生 resume 行为。
    for (tag, mode) in [("none", None), ("full", Some("full"))] {
        let path = dir.join(format!("{tag}.jsonl"));
        write_jsonl(&path, &[meta(mode)]);
        assert_eq!(session_file_supports_native_resume(&path, true), Ok(true));
    }

    // 非 codex 短路：即使盘上是 paginated 也不读文件。
    assert_eq!(
        session_file_supports_native_resume(&paginated, false),
        Ok(true)
    );

    // 探测窗只有 200 行：session_meta 在第 201 行 ⇒ 看不见 ⇒ 保持原生 resume。
    let late = dir.join("late.jsonl");
    {
        let mut file = fs::File::create(&late).unwrap();
        for i in 0..200 {
            writeln!(file, "{{\"noise\":{i}}}").unwrap();
        }
        writeln!(file, "{}", meta(Some("paginated"))).unwrap();
    }
    assert_eq!(session_file_supports_native_resume(&late, true), Ok(true));

    // 坏行跳过不中断扫描（partial 尾行是现存形状）。
    let broken = dir.join("broken.jsonl");
    write_jsonl(&broken, &[json!("not json"), meta(Some("paginated"))]);
    assert_eq!(
        session_file_supports_native_resume(&broken, true),
        Ok(false)
    );

    assert!(ensure_session_can_be_adopted(&paginated, true).is_err());
    assert_eq!(ensure_session_can_be_adopted(&paginated, false), Ok(()));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn adoption_refusal_wording_is_the_on_the_wire_contract() {
    let dir = scratch("adopt");
    let path = dir.join("s.jsonl");
    write_jsonl(
        &path,
        &[json!({"type":"session_meta","payload":{"history_mode":"paginated"}})],
    );
    let error = ensure_session_can_be_adopted(&path, true).unwrap_err();
    assert_eq!(
        error,
        "Codex paginated sessions cannot be adopted for native resume yet"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn session_id_resolution_follows_family_then_stem_fallback() {
    let dir = scratch("id");
    let uuid = "019d247a-2a83-76f3-b5c6-e4a59955af3f";

    // codex：session_meta.payload.id 优先。
    let codex = dir.join(format!("rollout-2025-10-30T13-33-20-{uuid}.jsonl"));
    write_jsonl(
        &codex,
        &[json!({"type":"session_meta","payload":{"id":"meta-id"}})],
    );
    assert_eq!(
        resolve_session_id_from_file(&codex, true).as_deref(),
        Some("meta-id")
    );
    // 空/全空白 id 不算数 ⇒ 落到文件名兜底：rollout-<20 字符时间戳>-<uuid>。
    write_jsonl(
        &codex,
        &[json!({"type":"session_meta","payload":{"id":"   "}})],
    );
    assert_eq!(
        resolve_session_id_from_file(&codex, true).as_deref(),
        Some(uuid)
    );

    // claude：sessionId 先于 session_id；都缺失时用 uuid 文件名。
    let claude = dir.join("c.jsonl");
    write_jsonl(&claude, &[json!({"session_id":"snake-id"})]);
    assert_eq!(
        resolve_session_id_from_file(&claude, false).as_deref(),
        Some("snake-id")
    );
    write_jsonl(
        &claude,
        &[json!({"session_id":"snake","sessionId":"camel"})],
    );
    assert_eq!(
        resolve_session_id_from_file(&claude, false).as_deref(),
        Some("camel")
    );
    let by_stem = dir.join(format!("{uuid}.jsonl"));
    write_jsonl(&by_stem, &[json!({"type":"user"})]);
    assert_eq!(
        resolve_session_id_from_file(&by_stem, false).as_deref(),
        Some(uuid)
    );

    // 解析不出 id ⇒ None：这就是「拒绝续跑」的判定原语。
    let nameless = dir.join("plain.jsonl");
    write_jsonl(&nameless, &[json!({"type":"user"})]);
    assert_eq!(resolve_session_id_from_file(&nameless, false), None);
    assert_eq!(resolve_session_id_from_file(&nameless, true), None);

    // 探测窗：第 201 行的 sessionId 不参与判定。
    let late = dir.join("late.jsonl");
    {
        let mut file = fs::File::create(&late).unwrap();
        for i in 0..200 {
            writeln!(file, "{{\"noise\":{i}}}").unwrap();
        }
        writeln!(file, "{}", json!({"sessionId":"too-late"})).unwrap();
    }
    assert_eq!(resolve_session_id_from_file(&late, false), None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn uuid_shape_and_timestamp_precedence_are_pinned() {
    assert!(is_uuid_like("019d247a-2a83-76f3-b5c6-e4a59955af3f"));
    assert!(!is_uuid_like("019d247a-2a83-76f3-b5c6-e4a59955af3"));
    assert!(!is_uuid_like("019d247a-2a83-76f3-b5c6-e4a59955af3g"));
    assert!(!is_uuid_like("019d247a2a8376f3b5c6e4a59955af3f"));

    let dir = scratch("time");
    let path = dir.join("s.jsonl");
    write_jsonl(
        &path,
        &[json!({"type":"user","timestamp":"2026-09-29T12:34:56.789Z"})],
    );
    let ms = session_started_at_ms(&path).unwrap();
    assert_eq!(
        ms,
        chrono::DateTime::parse_from_rfc3339("2026-09-29T12:34:56.789Z")
            .unwrap()
            .timestamp_millis()
    );

    // payload.timestamp 兜底（codex 形状）。
    let payload = dir.join("p.jsonl");
    write_jsonl(
        &payload,
        &[json!({"type":"response_item","payload":{"timestamp":"2026-01-01T00:00:00Z"}})],
    );
    assert_eq!(
        session_started_at_ms(&payload).unwrap(),
        chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis()
    );

    // 无可用时间戳 ⇒ 文件时间兜底（刚写的文件，应落在最近一分钟内）。
    let bare = dir.join("bare.jsonl");
    write_jsonl(&bare, &[json!({"type":"user"})]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let started = session_started_at_ms(&bare).unwrap();
    assert!((now - 60_000..now + 60_000).contains(&started), "{started}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn project_match_is_codex_only_and_reads_payload_cwd() {
    let dir = scratch("cwd");
    let project = dir.join("proj");
    fs::create_dir_all(&project).unwrap();
    let project = project.canonicalize().unwrap();

    let s = dir.join("s.jsonl");
    write_jsonl(
        &s,
        &[json!({"type":"session_meta","payload":{"cwd":project.to_string_lossy()}})],
    );
    assert!(session_matches_project(&s, project.to_str().unwrap(), true));
    let other = dir.join("other");
    fs::create_dir_all(&other).unwrap();
    assert!(!session_matches_project(
        &s,
        other.canonicalize().unwrap().to_str().unwrap(),
        true
    ));
    // 非 codex 永远放行（原行为）。
    assert!(session_matches_project(&s, "whatever", false));
    // 读不出任何 cwd ⇒ false（codex 判定是白名单式）。
    let mute = dir.join("mute.jsonl");
    write_jsonl(&mute, &[json!({"type":"user"})]);
    assert!(!session_matches_project(
        &mute,
        project.to_str().unwrap(),
        true
    ));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn recover_picks_nearest_in_window_and_respects_prompt() {
    let dir = scratch("recover");
    let fake_home = dir.join("home");
    let project = dir.join("proj");
    fs::create_dir_all(&project).unwrap();
    let project = project.canonicalize().unwrap();
    let base = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let enc =
        aeroric_core::sessions::discovery::encode_claude_project_path(project.to_str().unwrap());
    let sess_dir = fake_home.join(".claude").join("projects").join(&enc);
    fs::create_dir_all(&sess_dir).unwrap();
    let user = |text: &str, ts_ms: i64| {
        json!({"type":"user","timestamp":format_rfc3339(ts_ms),
               "message":{"content":[{"type":"text","text":text}]}})
    };

    let near = sess_dir.join("near.jsonl");
    write_jsonl(
        &near,
        &[
            json!({"sessionId":"near-id","timestamp":format_rfc3339(base + 2_000)}),
            user("先部署再回归", base + 2_000),
        ],
    );
    let far_ok = sess_dir.join("far-ok.jsonl");
    write_jsonl(
        &far_ok,
        &[
            json!({"sessionId":"far-id","timestamp":format_rfc3339(base + 60_000)}),
            user("回归测试", base + 60_000),
        ],
    );
    let out_of_window = sess_dir.join("too-late.jsonl");
    write_jsonl(
        &out_of_window,
        &[
            json!({"sessionId":"late-id","timestamp":format_rfc3339(base + 16 * 60_000)}),
            user("超窗任务", base + 16 * 60_000),
        ],
    );

    // 「部署」只有 near 匹配；16 分钟外的候选即使含相同文本也被窗挡掉。
    let recovered = recover_session(
        Some(&fake_home),
        project.to_str().unwrap(),
        "部署",
        base,
        false,
    )
    .expect("窗口内应命中");
    assert_eq!(recovered.session_id, "near-id");

    // 空 prompt 匹配一切 ⇒ 时间差最近者（delta 2 000 vs 60 000）。
    let any =
        recover_session(Some(&fake_home), project.to_str().unwrap(), "", base, false).unwrap();
    assert_eq!(any.session_id, "near-id");
    assert_eq!(
        any.session_path,
        near.to_string_lossy(),
        "recover 返回的必须是发现根下的真实文件路径（macOS /var→/private/var 符号链接下不得 canonicalize 比较）"
    );

    // contains 语义：「回归」两条都命中 ⇒ 取最近；「回归测试」只有 far 命中。
    let both = recover_session(
        Some(&fake_home),
        project.to_str().unwrap(),
        "回归",
        base,
        false,
    )
    .unwrap();
    assert_eq!(both.session_id, "near-id");
    let only_far = recover_session(
        Some(&fake_home),
        project.to_str().unwrap(),
        "回归测试",
        base + 58_000,
        false,
    )
    .expect("far-ok 在 ±15min 窗内应可作为候选");
    assert_eq!(
        only_far.session_id, "far-id",
        "near 文本 contains 不到「回归测试」"
    );

    // 超窗 ⇒ None：「解析不出 session 就拒绝续跑」的机器形状是没有候选，不是猜测。
    let none = recover_session(
        Some(&fake_home),
        project.to_str().unwrap(),
        "超窗",
        base,
        false,
    );
    assert_eq!(none, None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn recovered_session_serde_shape_is_camel_case() {
    let value = aeroric_core::sessions::resume::RecoveredSession {
        session_id: "s".to_string(),
        session_path: "/p/s.jsonl".to_string(),
    };
    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        r#"{"sessionId":"s","sessionPath":"/p/s.jsonl"}"#
    );
}

fn format_rfc3339(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .expect("epoch ms")
        .to_rfc3339()
}
