//! 外部协议面(AP7 / P9-04):ACP、OAuth 状态、有效沙箱、browser/media 工具、voice。
//!
//! ## 这一层的共同主题:**不假装做到了**
//!
//! 全部五项共享同一条规则:能力没就绪时**拒绝**,而不是降级成「看起来能跑」。
//! - ACP 不支持音频 → 拒绝带音频的附件,而不是静默丢掉音轨;
//! - 沙箱已启用但后端未就绪 → 不执行工具(fail closed);
//! - OAuth token 过期 → 状态显示过期,**token 本身不出现在任何返回值里**;
//! - browser/media → 原生端只显示请求/权限/进度,不代上游执行;
//! - voice → 麦克风被拒/取消/零音频/网络失败,一律**不发空 prompt**。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

// ── ACP ──────────────────────────────────────────────────────────────────────

/// ACP 的版本协商与能力协商**分开**:版本交集决定能不能说话,能力表决定能说什么。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpNegotiation {
    /// 本端支持的协议版本(降序)。
    pub supported_versions: Vec<u8>,
    /// 对端声明的版本。
    pub peer_version: u8,
    /// 对端声明的能力名。
    pub peer_capabilities: Vec<String>,
    /// 允许的工作目录与附加目录(ACP 会话的沙箱边界)。
    pub cwd: String,
    pub extra_dirs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum AcpRefusal {
    /// 版本无交集:明确说不支持,而不是按最低版本硬上。
    NoCommonVersion { ours: Vec<u8>, peer: u8 },
    /// ACP 选项不得注入非 ACP 族(如 OMP)。
    OptionsInjectedIntoAnotherFamily { family: String },
    /// 附加目录越界(不在允许的根之下)。
    ExtraDirOutsideRoots { dir: String, roots: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSession {
    pub negotiated_version: u8,
    pub cwd: String,
    pub extra_dirs: Vec<String>,
}

impl AcpNegotiation {
    /// 协商:版本必须**有交集**,并校验目录边界。
    pub fn negotiate(&self, allowed_roots: &[String]) -> Result<AcpSession, AcpRefusal> {
        if !self.supported_versions.contains(&self.peer_version) {
            return Err(AcpRefusal::NoCommonVersion {
                ours: self.supported_versions.clone(),
                peer: self.peer_version,
            });
        }
        for dir in &self.extra_dirs {
            let inside = allowed_roots
                .iter()
                .any(|root| dir == root || dir.starts_with(&format!("{root}/")));
            if !inside {
                return Err(AcpRefusal::ExtraDirOutsideRoots {
                    dir: dir.clone(),
                    roots: allowed_roots.to_vec(),
                });
            }
        }
        Ok(AcpSession {
            negotiated_version: self.peer_version,
            cwd: self.cwd.clone(),
            extra_dirs: self.extra_dirs.clone(),
        })
    }

    /// ACP 选项只对 ACP 族有效:别的族传进来即拒绝(P9-04 明文)。
    pub fn assert_family(&self, family: &str) -> Result<(), AcpRefusal> {
        if family == "acp" {
            Ok(())
        } else {
            Err(AcpRefusal::OptionsInjectedIntoAnotherFamily {
                family: family.to_string(),
            })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Text,
    Image,
    Audio,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAttachment {
    pub kind: AttachmentKind,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum AttachmentRefusal {
    /// 对端不支持该类型 → **拒绝整条 prompt**,而不是把附件删掉继续发。
    UnsupportedKind {
        kind: AttachmentKind,
        name: String,
        supported: Vec<AttachmentKind>,
    },
}

/// 附件准入。任何一个附件不被支持,整条消息被拒绝。
///
/// 「上游不支持 audio 时拒绝而非丢附件」是 P9-04 的明文要求:静默丢弃会让用户以为
/// 自己发了音频,而 Agent 从没见过它。
pub fn admit_attachments(
    attachments: &[AcpAttachment],
    peer_capabilities: &[String],
) -> Result<Vec<AcpAttachment>, AttachmentRefusal> {
    let supported: BTreeSet<AttachmentKind> = peer_capabilities
        .iter()
        .filter_map(|capability| match capability.as_str() {
            "audio" => Some(AttachmentKind::Audio),
            "image" => Some(AttachmentKind::Image),
            "video" => Some(AttachmentKind::Video),
            _ => None,
        })
        .collect();
    for attachment in attachments {
        if attachment.kind == AttachmentKind::Text {
            continue;
        }
        if !supported.contains(&attachment.kind) {
            return Err(AttachmentRefusal::UnsupportedKind {
                kind: attachment.kind,
                name: attachment.name.clone(),
                supported: supported.into_iter().collect(),
            });
        }
    }
    Ok(attachments.to_vec())
}

// ── OAuth ────────────────────────────────────────────────────────────────────

/// 凭据是否可用。**只有状态,没有 token**——类型上就不给 UI 拿 token 的机会。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OAuthStatus {
    NotConfigured,
    Configured {
        /// token 的 owner(既有凭据保管者),UI 只能看到名字。
        owner: String,
        /// 过期时间(Unix 秒)。`None` = 未知/不过期。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<i64>,
        /// 本次登录的 generation:迟到响应按它丢弃。
        generation: u64,
    },
    Expired {
        owner: String,
        generation: u64,
    },
}

impl OAuthStatus {
    pub fn is_usable(&self) -> bool {
        matches!(self, OAuthStatus::Configured { .. })
    }

    /// 该状态是否应当被 `generation` 更新的登录覆盖。过期/旧代次都不得覆盖新登录。
    pub fn should_be_replaced_by(&self, incoming_generation: u64) -> bool {
        match self {
            OAuthStatus::Configured { generation, .. } | OAuthStatus::Expired { generation, .. } => {
                incoming_generation > *generation
            }
            OAuthStatus::NotConfigured => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthFlow {
    /// 系统默认浏览器 + 回调(`ASWebAuthenticationSession` 口径)。
    SystemBrowser,
    /// 无浏览器环境下的 device code。
    DeviceCode,
}

/// 登录启动指令:UI 只拿得到「去哪」与「显示什么」,拿不到 token。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthLaunch {
    pub flow: OAuthFlow,
    pub authorization_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum OAuthError {
    /// 凭据由既有 owner 保管,任何导出请求都拒绝。
    TokenExportRefused,
    /// 迟到登录不得覆盖更新的会话。
    StaleGeneration { current: u64, incoming: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WebviewCredentialRequest {
    /// webview 只能问「配好了吗、谁保管的、什么时候过期」。
    Status,
    /// 任何要求把 token/refresh token 交给 webview 的请求。
    Export { reason: String },
}

/// 处理来自 webview 的凭据请求。**导出一律拒绝**,状态照给。
pub fn answer_webview_credential_request(
    request: &WebviewCredentialRequest,
    status: &OAuthStatus,
) -> Result<OAuthStatus, OAuthError> {
    match request {
        WebviewCredentialRequest::Status => Ok(status.clone()),
        WebviewCredentialRequest::Export { .. } => Err(OAuthError::TokenExportRefused),
    }
}

/// 用新登录结果更新状态。旧 generation 的迟到结果被拒绝。
pub fn apply_login(
    current: &OAuthStatus,
    incoming_generation: u64,
    owner: &str,
    expires_at: Option<i64>,
) -> Result<OAuthStatus, OAuthError> {
    if !current.should_be_replaced_by(incoming_generation) {
        let current_generation = match current {
            OAuthStatus::Configured { generation, .. } | OAuthStatus::Expired { generation, .. } => {
                *generation
            }
            OAuthStatus::NotConfigured => 0,
        };
        return Err(OAuthError::StaleGeneration {
            current: current_generation,
            incoming: incoming_generation,
        });
    }
    Ok(OAuthStatus::Configured {
        owner: owner.to_string(),
        expires_at,
        generation: incoming_generation,
    })
}

// ── 有效沙箱 ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    /// 用户没开沙箱。
    Disabled,
    /// 开了,后端就绪:可以跑。
    Ready,
    /// 开了但后端没就绪(授权缺失/扩展没装/平台不支持)。
    NotReady,
    /// 平台根本不支持沙箱。
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxReport {
    /// 用户/设置要求的沙箱。
    pub requested_enabled: bool,
    pub state: SandboxState,
    /// 实际生效的后端名(如 `seatbelt`、`landlock`、`none`)。
    pub backend: String,
    /// 未就绪的原因,给 UI 原样展示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 生效后允许的写入根。
    pub writable_roots: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum SandboxError {
    /// 要求了沙箱但没就绪 → **fail closed**,不降级运行。
    EnabledButNotReady { backend: String, note: Option<String> },
    /// 平台不支持又要求沙箱。
    UnsupportedButRequired,
}

impl SandboxReport {
    /// 能否执行工具。**只有 `Ready` 或者「用户本来就没要求沙箱」才放行**。
    ///
    /// 这一条是 fail closed 的全部实现:要求了却没就绪,就是拒绝,而不是「那就
    /// 不沙箱跑吧」。
    pub fn require_ready(&self) -> Result<(), SandboxError> {
        match (self.requested_enabled, self.state) {
            (false, _) => Ok(()),
            (true, SandboxState::Ready) => Ok(()),
            (true, SandboxState::Unsupported) => Err(SandboxError::UnsupportedButRequired),
            (true, _) => Err(SandboxError::EnabledButNotReady {
                backend: self.backend.clone(),
                note: self.note.clone(),
            }),
        }
    }

    /// 写路径必须在生效的 writable roots 之内。
    pub fn allows_write(&self, path: &str) -> bool {
        if !self.requested_enabled || self.state != SandboxState::Ready {
            return false;
        }
        self.writable_roots
            .iter()
            .any(|root| path == root || path.starts_with(&format!("{root}/")))
    }
}

// ── browser / media 工具 ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionKind {
    ScreenRecording,
    Accessibility,
    Camera,
    Microphone,
    Automation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionGrant {
    Granted,
    Denied,
    /// 还没问过。
    NotDetermined,
    /// 系统设置里显示已授权,但当前二进制签名不稳定导致实际无效(见项目签名基线)。
    UnstableSignature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivityNotice {
    /// 哪个外部工具在请求什么(展示给用户)。
    pub tool: String,
    pub summary: String,
    /// 执行者永远是外部 Agent,原生端不是执行者。
    pub executor: String,
    pub permissions: Vec<(ToolPermissionKind, ToolPermissionGrant)>,
}

impl ToolActivityNotice {
    /// 原生端能做的**唯一**动作:把用户送到系统设置面板。
    ///
    /// 这里刻意不提供「申请」API:程序化开启 Accessibility/Screen Recording 既不可靠
    /// 也不可接受。缺权限时返回要打开的面板,权限本身由用户给。
    pub fn handoff(&self, kind: ToolPermissionKind) -> ToolPermissionHandoff {
        let grant = self
            .permissions
            .iter()
            .find(|(candidate, _)| *candidate == kind)
            .map(|(_, grant)| *grant)
            .unwrap_or(ToolPermissionGrant::NotDetermined);
        match grant {
            ToolPermissionGrant::Granted => ToolPermissionHandoff::Ready,
            ToolPermissionGrant::UnstableSignature => ToolPermissionHandoff::Reauthorize {
                kind,
                hint: "the app's signature changed; re-authorize after this build".to_string(),
            },
            _ => ToolPermissionHandoff::OpenSystemSettings { kind },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "handoff", rename_all = "snake_case")]
pub enum ToolPermissionHandoff {
    Ready,
    OpenSystemSettings { kind: ToolPermissionKind },
    Reauthorize { kind: ToolPermissionKind, hint: String },
}

// ── voice ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicPermission {
    Granted,
    Denied,
    NotDetermined,
    Restricted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoicePhase {
    /// 用户显式点开才进入(默认关闭)。
    Idle,
    RequestingPermission,
    Recording,
    /// 转写结果进草稿,**没有**提交。
    Draft,
    Submitting,
    Sent,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum VoiceError {
    /// 用户没开语音输入。
    NotOptedIn,
    MicDenied { permission: MicPermission },
    /// 没录到任何音频(或全是静音)。
    NoAudio,
    /// 转写失败/网络失败。
    TranscriptionFailed { detail: String },
    /// 草稿是空的 —— 不发送空 prompt。
    EmptyDraft,
    /// 顺序错误(例如没在 Recording 就 stop)。
    WrongPhase { expected: Vec<VoicePhase>, actual: VoicePhase },
}

/// 语音输入状态机。**终态只有 `Sent` 与 `Cancelled`**,且只有 `Sent` 才产生请求。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSession {
    pub opted_in: bool,
    pub phase: VoicePhase,
    pub mic: MicPermission,
    /// 录到的有效音频帧数;0 视为没录到。
    pub audio_frames: u64,
    pub transcript: String,
    /// 提交过的 prompt(证据:为空说明什么都没发)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_prompt: Option<String>,
}

impl VoiceSession {
    /// 默认关闭:没显式开启就没有会话。
    pub fn new(opted_in: bool) -> Self {
        Self {
            opted_in,
            phase: VoicePhase::Idle,
            mic: MicPermission::NotDetermined,
            audio_frames: 0,
            transcript: String::new(),
            submitted_prompt: None,
        }
    }

    /// 开始录音。麦克风权限按需申请;被拒即停在这一步,**不会**有后续提交。
    pub fn start(&mut self, mic: MicPermission) -> Result<(), VoiceError> {
        if !self.opted_in {
            return Err(VoiceError::NotOptedIn);
        }
        self.mic = mic;
        match mic {
            MicPermission::Granted => {
                self.phase = VoicePhase::Recording;
                self.audio_frames = 0;
                Ok(())
            }
            other => {
                self.phase = VoicePhase::Idle;
                Err(VoiceError::MicDenied { permission: other })
            }
        }
    }

    /// 结束录音并给出转写结果。零音频、转写失败都在这里被挡住。
    pub fn stop(&mut self, transcript: Result<String, String>) -> Result<(), VoiceError> {
        if self.phase != VoicePhase::Recording {
            return Err(VoiceError::WrongPhase {
                expected: vec![VoicePhase::Recording],
                actual: self.phase,
            });
        }
        if self.audio_frames == 0 {
            self.phase = VoicePhase::Cancelled;
            return Err(VoiceError::NoAudio);
        }
        match transcript {
            Ok(text) => {
                self.transcript = text;
                // 转写结果先进草稿,不提交。
                self.phase = VoicePhase::Draft;
                Ok(())
            }
            Err(detail) => {
                self.phase = VoicePhase::Cancelled;
                Err(VoiceError::TranscriptionFailed { detail })
            }
        }
    }

    pub fn cancel(&mut self) {
        self.phase = VoicePhase::Cancelled;
    }

    /// 用户显式提交草稿。这是**唯一**产生 prompt 的入口。
    pub fn submit(&mut self) -> Result<String, VoiceError> {
        if self.phase != VoicePhase::Draft {
            return Err(VoiceError::WrongPhase {
                expected: vec![VoicePhase::Draft],
                actual: self.phase,
            });
        }
        let text = self.transcript.trim().to_string();
        if text.is_empty() {
            self.phase = VoicePhase::Cancelled;
            return Err(VoiceError::EmptyDraft);
        }
        self.submitted_prompt = Some(text.clone());
        self.phase = VoicePhase::Sent;
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn acp_negotiates_the_intersection_not_the_lowest_version() {
        let negotiation = AcpNegotiation {
            supported_versions: vec![3, 2],
            peer_version: 1,
            peer_capabilities: caps(&[]),
            cwd: "/repo".to_string(),
            extra_dirs: Vec::new(),
        };
        assert_eq!(
            negotiation.negotiate(&["/repo".to_string()]).unwrap_err(),
            AcpRefusal::NoCommonVersion {
                ours: vec![3, 2],
                peer: 1,
            }
        );

        let ok = AcpNegotiation {
            peer_version: 2,
            ..negotiation
        };
        assert_eq!(ok.negotiate(&["/repo".to_string()]).unwrap().negotiated_version, 2);
    }

    #[test]
    fn acp_extra_dirs_must_stay_inside_the_allowed_roots() {
        let negotiation = AcpNegotiation {
            supported_versions: vec![1],
            peer_version: 1,
            peer_capabilities: caps(&[]),
            cwd: "/repo".to_string(),
            extra_dirs: vec!["/repo/vendor".to_string(), "/etc".to_string()],
        };
        assert_eq!(
            negotiation.negotiate(&["/repo".to_string()]).unwrap_err(),
            AcpRefusal::ExtraDirOutsideRoots {
                dir: "/etc".to_string(),
                roots: vec!["/repo".to_string()],
            }
        );
    }

    #[test]
    fn acp_options_are_not_injected_into_another_family() {
        let negotiation = AcpNegotiation {
            supported_versions: vec![1],
            peer_version: 1,
            peer_capabilities: caps(&[]),
            cwd: "/repo".to_string(),
            extra_dirs: Vec::new(),
        };
        assert!(negotiation.assert_family("acp").is_ok());
        assert_eq!(
            negotiation.assert_family("omp").unwrap_err(),
            AcpRefusal::OptionsInjectedIntoAnotherFamily {
                family: "omp".to_string()
            }
        );
    }

    #[test]
    fn an_unsupported_audio_attachment_is_refused_not_dropped() {
        let attachments = vec![
            AcpAttachment {
                kind: AttachmentKind::Image,
                name: "shot.png".to_string(),
            },
            AcpAttachment {
                kind: AttachmentKind::Audio,
                name: "note.m4a".to_string(),
            },
        ];
        let refusal = admit_attachments(&attachments, &caps(&["image"])).unwrap_err();
        assert_eq!(
            refusal,
            AttachmentRefusal::UnsupportedKind {
                kind: AttachmentKind::Audio,
                name: "note.m4a".to_string(),
                supported: vec![AttachmentKind::Image],
            }
        );
        // 支持时原样通过,不做静默替换。
        assert_eq!(
            admit_attachments(&attachments, &caps(&["image", "audio"])).unwrap(),
            attachments
        );
    }

    #[test]
    fn the_webview_can_read_oauth_status_but_never_export_the_token() {
        let status = OAuthStatus::Configured {
            owner: "keychain".to_string(),
            expires_at: Some(1_700_000_000),
            generation: 4,
        };
        let answer =
            answer_webview_credential_request(&WebviewCredentialRequest::Status, &status).unwrap();
        assert_eq!(answer, status);

        let refusal = answer_webview_credential_request(
            &WebviewCredentialRequest::Export {
                reason: "show it in the settings pane".to_string(),
            },
            &status,
        )
        .unwrap_err();
        assert_eq!(refusal, OAuthError::TokenExportRefused);
    }

    #[test]
    fn a_stale_login_does_not_overwrite_a_newer_session() {
        let current = OAuthStatus::Configured {
            owner: "keychain".to_string(),
            expires_at: None,
            generation: 5,
        };
        assert_eq!(
            apply_login(&current, 4, "keychain", None).unwrap_err(),
            OAuthError::StaleGeneration {
                current: 5,
                incoming: 4,
            }
        );
        let updated = apply_login(&current, 6, "keychain", Some(1)).unwrap();
        assert!(matches!(
            updated,
            OAuthStatus::Configured { generation: 6, .. }
        ));
    }

    #[test]
    fn an_expired_status_is_never_usable() {
        let expired = OAuthStatus::Expired {
            owner: "keychain".to_string(),
            generation: 2,
        };
        assert!(!expired.is_usable());
    }

    #[test]
    fn a_requested_but_unready_sandbox_fails_closed() {
        let report = SandboxReport {
            requested_enabled: true,
            state: SandboxState::NotReady,
            backend: "seatbelt".to_string(),
            note: Some("extension not loaded".to_string()),
            writable_roots: vec!["/tmp/work".to_string()],
        };
        assert_eq!(
            report.require_ready().unwrap_err(),
            SandboxError::EnabledButNotReady {
                backend: "seatbelt".to_string(),
                note: Some("extension not loaded".to_string()),
            }
        );
        // 未就绪时连写入判断都必须是 false,而不是「大概可以」。
        assert!(!report.allows_write("/tmp/work/file.txt"));

        let ready = SandboxReport {
            state: SandboxState::Ready,
            note: None,
            ..report
        };
        assert!(ready.require_ready().is_ok());
        assert!(ready.allows_write("/tmp/work/file.txt"));
        assert!(!ready.allows_write("/etc/passwd"));
    }

    #[test]
    fn an_unsandboxed_request_needs_no_backend() {
        let report = SandboxReport {
            requested_enabled: false,
            state: SandboxState::Unsupported,
            backend: "none".to_string(),
            note: None,
            writable_roots: Vec::new(),
        };
        assert!(report.require_ready().is_ok());
        assert!(!report.allows_write("/anything"));
    }

    #[test]
    fn tool_permissions_are_handed_off_never_granted_programmatically() {
        let notice = ToolActivityNotice {
            tool: "browser.open".to_string(),
            summary: "open a page".to_string(),
            executor: "external agent".to_string(),
            permissions: vec![
                (ToolPermissionKind::ScreenRecording, ToolPermissionGrant::Denied),
                (
                    ToolPermissionKind::Accessibility,
                    ToolPermissionGrant::UnstableSignature,
                ),
                (ToolPermissionKind::Camera, ToolPermissionGrant::Granted),
            ],
        };
        assert_eq!(
            notice.handoff(ToolPermissionKind::ScreenRecording),
            ToolPermissionHandoff::OpenSystemSettings {
                kind: ToolPermissionKind::ScreenRecording
            }
        );
        assert!(matches!(
            notice.handoff(ToolPermissionKind::Accessibility),
            ToolPermissionHandoff::Reauthorize { .. }
        ));
        assert_eq!(
            notice.handoff(ToolPermissionKind::Camera),
            ToolPermissionHandoff::Ready
        );
    }

    #[test]
    fn voice_is_off_unless_opted_in_and_a_denied_mic_produces_nothing() {
        let mut session = VoiceSession::new(false);
        assert_eq!(session.start(MicPermission::Granted), Err(VoiceError::NotOptedIn));
        assert_eq!(session.submitted_prompt, None);

        let mut session = VoiceSession::new(true);
        assert_eq!(
            session.start(MicPermission::Denied),
            Err(VoiceError::MicDenied {
                permission: MicPermission::Denied
            })
        );
        assert_eq!(session.phase, VoicePhase::Idle);
        assert_eq!(session.submitted_prompt, None);
    }

    #[test]
    fn voice_never_sends_an_empty_prompt() {
        // 零音频。
        let mut session = VoiceSession::new(true);
        session.start(MicPermission::Granted).unwrap();
        assert_eq!(session.stop(Ok("hi".to_string())), Err(VoiceError::NoAudio));
        assert_eq!(session.phase, VoicePhase::Cancelled);
        assert_eq!(session.submitted_prompt, None);

        // 转写失败。
        let mut session = VoiceSession::new(true);
        session.start(MicPermission::Granted).unwrap();
        session.audio_frames = 120;
        assert_eq!(
            session.stop(Err("network unreachable".to_string())),
            Err(VoiceError::TranscriptionFailed {
                detail: "network unreachable".to_string()
            })
        );
        assert_eq!(session.submitted_prompt, None);

        // 空白草稿。
        let mut session = VoiceSession::new(true);
        session.start(MicPermission::Granted).unwrap();
        session.audio_frames = 120;
        session.stop(Ok("   ".to_string())).unwrap();
        assert_eq!(session.phase, VoicePhase::Draft);
        assert_eq!(session.submit(), Err(VoiceError::EmptyDraft));
        assert_eq!(session.submitted_prompt, None);
    }

    #[test]
    fn voice_transcript_is_a_draft_until_the_user_submits_it() {
        let mut session = VoiceSession::new(true);
        session.start(MicPermission::Granted).unwrap();
        session.audio_frames = 240;
        session.stop(Ok("run the tests".to_string())).unwrap();
        assert_eq!(session.phase, VoicePhase::Draft);
        assert_eq!(session.submitted_prompt, None, "draft is not a sent prompt");

        // 取消:草稿不落地。
        let mut cancelled = session.clone();
        cancelled.cancel();
        assert_eq!(cancelled.phase, VoicePhase::Cancelled);
        assert_eq!(cancelled.submitted_prompt, None);
        assert_eq!(
            cancelled.submit(),
            Err(VoiceError::WrongPhase {
                expected: vec![VoicePhase::Draft],
                actual: VoicePhase::Cancelled
            })
        );

        assert_eq!(session.submit().unwrap(), "run the tests");
        assert_eq!(session.phase, VoicePhase::Sent);
        assert_eq!(session.submitted_prompt.as_deref(), Some("run the tests"));
    }
}
