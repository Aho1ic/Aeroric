//! 日志与错误文案的脱敏（P3-06 Action 5「stdout/stderr 日志脱敏」）。
//!
//! 形状与宿主工具链的 `check.mjs --redact-selftest` 同族（那里管 JS 侧日志，
//! 这里管 host 进程的 stderr）：**写入前洗，不靠调用方自觉**。
//! 秘密的登记处只有一个——`Redactor` 构造时收下的值集合；运行期新出现的
//! 秘密（如首建的 token）必须经 `add_secret` 登记，否则它出现在日志里就是
//! 泄漏，而不是「没匹配到」。
//!
//! 错误类型侧的防线是设计本身：`AuthError::TokenMismatch` 与
//! `HostToken` 的 `Debug` 都不携带明文（277 片）；本模块兜住的是
//! **第三方文本**（handler 转发的上游错误、serde 报错等）里混进来的秘密。

use std::sync::Mutex;

pub const MASK: &str = "****";

#[derive(Default)]
pub struct Redactor {
    secrets: Mutex<Vec<String>>,
}

impl Redactor {
    pub fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        Self {
            secrets: Mutex::new(secrets.into_iter().filter(|s| !s.is_empty()).collect()),
        }
    }

    /// 运行期新增的秘密（首建 token、读到的凭据值）。空值忽略——
    /// 空串匹配一切，登记它等于把所有文本洗成 MASK。
    pub fn add_secret(&self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        self.secrets.lock().expect("lock").push(secret.to_string());
    }

    /// 把文本里出现的每个已登记秘密替换为 MASK（重叠出现全部洗掉）。
    pub fn redact(&self, text: &str) -> String {
        let secrets = self.secrets.lock().expect("lock");
        let mut out = text.to_string();
        for secret in secrets.iter() {
            out = out.replace(secret.as_str(), MASK);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_secrets_are_washed_everywhere_they_appear() {
        let redactor = Redactor::new(["tok-abc123".to_string(), "sk-live-42".to_string()]);
        let line =
            "upstream rejected: token=tok-abc123 while header was sk-live-42; retry tok-abc123";
        let washed = redactor.redact(line);
        assert!(!washed.contains("tok-abc123"));
        assert!(!washed.contains("sk-live-42"));
        assert_eq!(
            washed,
            "upstream rejected: token=**** while header was ****; retry ****"
        );
    }

    #[test]
    fn empty_and_unregistered_texts_pass_through_unchanged() {
        let redactor = Redactor::new(["secret".to_string()]);
        assert_eq!(
            redactor.redact("nothing sensitive here"),
            "nothing sensitive here"
        );
        assert_eq!(redactor.redact(""), "");
        // 空值登记被忽略（空串匹配一切，接受它等于自毁日志）。
        redactor.add_secret("");
        assert_eq!(redactor.redact("still readable"), "still readable");
    }

    #[test]
    fn runtime_secrets_join_the_wash_list() {
        let redactor = Redactor::new([]);
        let token = "fresh-token-value";
        redactor.add_secret(token);
        assert_eq!(
            redactor.redact(&format!("handshake with {token}")),
            "handshake with ****"
        );
    }
}
