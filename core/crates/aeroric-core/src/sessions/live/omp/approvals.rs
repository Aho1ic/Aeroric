//! omp asks for tool approval with a plain `select` (RPC has no approval
//! dialog): title `Allow tool: <name>` then detail lines, options from
//! `approvalChoiceLabels` (`tools/approval.ts`): `Approve`, optionally
//! `Approve <subject> for this session`, `Deny`. Anything else — grant
//! options, unknown labels — is not an approval we can map, and is shown as
//! an ordinary question instead (never guessed at).

use serde_json::Value;

pub const APPROVE: &str = "Approve";
pub const DENY: &str = "Deny";
const TITLE_PREFIX: &str = "Allow tool: ";
const SESSION_PREFIX: &str = "Approve ";
const SESSION_SUFFIX: &str = " for this session";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolApproval {
    pub tool: String,
    /// The detail lines under the title (command, path, reason, …).
    pub summary: String,
    /// The subject an "approve for this session" answer covers, with its label.
    pub session: Option<(String, String)>,
}

impl ToolApproval {
    /// Recognise a tool approval in an `extension_ui_request`; `None` = not one.
    pub fn parse(request: &Value) -> Option<ToolApproval> {
        if request.get("method").and_then(Value::as_str) != Some("select") {
            return None;
        }
        let title = request.get("title").and_then(Value::as_str)?;
        let (first, rest) = title.split_once('\n').unwrap_or((title, ""));
        let tool = first.strip_prefix(TITLE_PREFIX)?.trim().to_string();
        let options: Vec<&str> = request
            .get("options")
            .and_then(Value::as_array)?
            .iter()
            .map(|option| option.as_str())
            .collect::<Option<_>>()?;
        let session = match options.as_slice() {
            [APPROVE, DENY] => None,
            [APPROVE, label, DENY] => {
                let subject = label
                    .strip_prefix(SESSION_PREFIX)?
                    .strip_suffix(SESSION_SUFFIX)?
                    .to_string();
                Some((subject, label.to_string()))
            }
            _ => return None,
        };
        let summary = rest
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Some(ToolApproval {
            summary: if summary.is_empty() {
                tool.clone()
            } else {
                summary
            },
            tool,
            session,
        })
    }

    /// Decisions this approval accepts, in the wire vocabulary.
    pub fn options(&self) -> Vec<String> {
        let mut options = vec!["allowOnce".to_string()];
        if self.session.is_some() {
            options.push("allowForSession".to_string());
        }
        options.push("deny".to_string());
        options
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recognises_the_three_label_approval_and_its_subject() {
        let request = json!({"type": "extension_ui_request", "id": "u1", "method": "select",
            "title": "Allow tool: bash\nCommand: echo hi", "options": ["Approve", "Approve bash for this session", "Deny"]});
        let approval = ToolApproval::parse(&request).unwrap();
        assert_eq!(approval.tool, "bash");
        assert_eq!(approval.summary, "Command: echo hi");
        assert_eq!(
            approval.session,
            Some(("bash".into(), "Approve bash for this session".into()))
        );
        assert_eq!(
            approval.options(),
            vec!["allowOnce", "allowForSession", "deny"]
        );
    }

    #[test]
    fn two_labels_have_no_session_choice() {
        let request = json!({"method": "select", "title": "Allow tool: write", "options": ["Approve", "Deny"]});
        let approval = ToolApproval::parse(&request).unwrap();
        assert_eq!(approval.session, None);
        assert_eq!(approval.summary, "write");
    }

    #[test]
    fn grants_unknown_labels_and_other_selects_are_not_approvals() {
        for request in [
            json!({"method": "select", "title": "Allow tool: edit", "options": ["Approve Directory: src", "Approve Once", "Deny"]}),
            json!({"method": "select", "title": "Allow tool: bash", "options": ["Yes", "No"]}),
            json!({"method": "select", "title": "Pick a framework", "options": ["Approve", "Deny"]}),
            json!({"method": "confirm", "title": "Allow tool: bash", "message": "?"}),
        ] {
            assert_eq!(ToolApproval::parse(&request), None, "{request}");
        }
    }
}
