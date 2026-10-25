//! A13: `diagnostics.export_report` — one redacted Markdown report the user can
//! save and share. It collects facts only (build, capabilities, startup
//! degradations, task counts by status, detected agents, the host log tail);
//! never prompts, messages or credentials. Every byte goes through
//! `maintenance::diagnostic_report::redact`.

use std::path::PathBuf;
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::maintenance::diagnostic_report::{render, tail, Section};
use aeroric_core::maintenance::StartupDegradationLog;
use aeroric_core::projects::ProjectService;
use aeroric_core::tasks::TaskService;
use aeroric_protocol::capability::Capability;
use aeroric_protocol::envelope::{Frame, Request};
use serde_json::{json, Value};

use crate::dispatcher::ServiceSet;
use crate::services::ok;

pub const EXPORT_METHOD: &str = "diagnostics.export_report";
/// Lines of `<data>/logs/host.log` (and its backup) the report carries.
const LOG_TAIL_LINES: usize = 400;

pub struct ReportInputs {
    pub context: Arc<CoreContext>,
    pub service: Option<Arc<TaskService>>,
    pub degradations: Arc<StartupDegradationLog>,
    pub capabilities: Vec<Capability>,
    /// The user's real home (shown as `~`; agents are detected under it).
    pub home: Option<PathBuf>,
}

pub fn register_diagnostic_report_services(set: &mut ServiceSet, inputs: ReportInputs) {
    let inputs = Arc::new(inputs);
    set.register(
        EXPORT_METHOD,
        Arc::new(move |request: Request, _call| {
            let inputs = Arc::clone(&inputs);
            Box::pin(async move {
                let id = request.id.clone();
                match tokio::task::spawn_blocking(move || export(&request, &inputs)).await {
                    Ok(frame) => frame,
                    Err(e) => crate::dispatcher::failure(
                        id,
                        aeroric_protocol::error::ErrorCode::IoError,
                        &e.to_string(),
                    ),
                }
            })
        }),
    );
}

fn export(request: &Request, inputs: &ReportInputs) -> Frame {
    let (file_name, text) = build_report(inputs);
    ok(request, json!({ "fileName": file_name, "text": text }))
}

/// `(file name, redacted Markdown)`.
pub fn build_report(inputs: &ReportInputs) -> (String, String) {
    let data_dir = inputs.context.paths.data_dir();
    let generated = chrono::Local::now();
    let mut sections = vec![Section {
        title: "概况".into(),
        body: format!(
            "生成时间: {}\n宿主版本: aeroric-cli {}\n协议版本: {}\n系统: {} {}\n宿主进程: {}\n数据目录: {}",
            generated.to_rfc3339(),
            env!("CARGO_PKG_VERSION"),
            aeroric_protocol::PROTOCOL_VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::process::id(),
            data_dir.display(),
        ),
    }];
    sections.push(Section {
        title: "宿主能力".into(),
        body: inputs
            .capabilities
            .iter()
            .map(|c| format!("{} = {}", c.name, if c.enabled { "on" } else { "off" }))
            .collect::<Vec<_>>()
            .join("\n"),
    });
    let degradations = inputs.degradations.list();
    sections.push(Section {
        title: "启动降级".into(),
        body: if degradations.is_empty() {
            "无".into()
        } else {
            degradations
                .iter()
                .map(|d| format!("{}: {}（退到 {}）", d.component, d.reason, d.fallback))
                .collect::<Vec<_>>()
                .join("\n")
        },
    });
    sections.push(Section {
        title: "任务（按项目编号，只计状态）".into(),
        body: task_counts(inputs),
    });
    let detected = inputs
        .home
        .as_deref()
        .map(|home| {
            let path = std::env::var("PATH").ok();
            aeroric_core::agents::detect::detect_agents(path.as_deref(), home)
        })
        .unwrap_or_default();
    sections.push(Section {
        title: "Agent 检测".into(),
        body: serde_json::to_string_pretty(&detected).unwrap_or_default(),
    });
    sections.push(Section {
        title: format!("宿主日志（最后 {LOG_TAIL_LINES} 行）"),
        body: log_tail(&data_dir.join("logs")),
    });
    let secrets: Vec<String> = std::fs::read_to_string(data_dir.join("host.token"))
        .map(|t| vec![t.trim().to_string()])
        .unwrap_or_default();
    let text = render(
        "Aeroric 诊断报告",
        &sections,
        inputs.home.as_deref(),
        &secrets,
    );
    (
        format!(
            "aeroric-diagnostics-{}.md",
            generated.format("%Y%m%d-%H%M%S")
        ),
        text,
    )
}

fn task_counts(inputs: &ReportInputs) -> String {
    let Some(service) = inputs.service.as_ref() else {
        return "此宿主形态没有任务服务".into();
    };
    let Ok(projects) = ProjectService::new(Arc::clone(&inputs.context)).list() else {
        return "无法读取项目列表".into();
    };
    let mut lines = Vec::new();
    for (index, project) in projects.iter().enumerate() {
        let Some(id) = project.get("id").and_then(Value::as_str) else {
            continue;
        };
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        match service.load(id) {
            Ok(stamped) => {
                for record in &stamped.records {
                    let status = record
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    *counts.entry(status.to_string()).or_default() += 1;
                }
                let summary = counts
                    .iter()
                    .map(|(status, n)| format!("{status} {n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!(
                    "项目 {}: {}",
                    index + 1,
                    if summary.is_empty() {
                        "无任务".into()
                    } else {
                        summary
                    }
                ));
            }
            Err(error) => lines.push(format!("项目 {}: 读取失败（{error}）", index + 1)),
        }
    }
    if lines.is_empty() {
        "无项目".into()
    } else {
        lines.join("\n")
    }
}

/// The backup first, then the live log, so the tail is chronological.
fn log_tail(dir: &std::path::Path) -> String {
    let mut text = String::new();
    for name in ["host.log.1", "host.log"] {
        if let Ok(chunk) = std::fs::read_to_string(dir.join(name)) {
            text.push_str(&chunk);
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
        }
    }
    if text.trim().is_empty() {
        "（没有日志：宿主未写过 stderr，或日志目录不可用）".into()
    } else {
        tail(&text, LOG_TAIL_LINES)
    }
}
