//! 扩展信任判定（P8-01 Action 3：安装前显示来源/revision/写入范围/所需权限；
//! 第三方脚本运行必须用户确认）。
//!
//! 纯判定：verdict 由调用方（UI）呈现为确认表单；core 不执行任何脚本。

/// 一条待安装扩展的来源摘要（UI 的确认表单数据源）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionSource {
    pub kind: String,
    pub origin: String,
    pub revision: Option<String>,
    pub write_scope: Vec<String>,
    pub requested_permissions: Vec<String>,
}

/// 信任判定。`user_confirmed` 由 UI 的显式确认承载——判定本身不含"默认信任"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustVerdict {
    /// 来源受信（内置/官方 marketplace）＋revision 已知。
    Trusted,
    /// 需要用户确认（第三方来源、无 revision、或写入范围越出 skills 目录）。
    RequiresConfirmation,
    /// 拒绝：写入范围落点在 skills 目录之外（含 `..`/绝对路径/空 scope）。
    Rejected,
}

pub fn verdict(source: &ExtensionSource, user_confirmed: bool) -> TrustVerdict {
    // 写入范围越界一律拒绝——确认也不能洗白越界写入。
    if source.write_scope.is_empty()
        || source.write_scope.iter().any(|scope| {
            scope.is_empty()
                || scope.starts_with('/')
                || scope.split('/').any(|seg| seg == "..")
                || !(scope.starts_with("skills/") || scope == "skills")
        })
    {
        return TrustVerdict::Rejected;
    }
    let builtin = matches!(source.kind.as_str(), "builtin" | "official-marketplace");
    if builtin && source.revision.is_some() {
        return TrustVerdict::Trusted;
    }
    if user_confirmed {
        TrustVerdict::Trusted
    } else {
        TrustVerdict::RequiresConfirmation
    }
}
