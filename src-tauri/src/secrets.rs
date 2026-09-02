//! OS 钥匙串的薄包装。
//!
//! 只服务一个用途:随手记 RAG 的 embedding key(计划 §3.3 —— 那一节原先列的「图床
//! token」「剪藏凭据」核对后都不成立)。放在 crate 根而不是 `notebook/` 下面,是因为
//! 这里没有任何笔记概念,将来第二个秘密要落盘时不必再挪一次。
//!
//! 不变量:
//!
//! 1. **明文不出后端。** 没有任何 `#[tauri::command]` 返回 [`get`] 的结果 —— 前端只见
//!    has / set / delete。[`get`] 是 `pub(crate)`,给真的要去调 provider 的那一层用
//!    (`notebook::rag::commands::resolve_key`)。
//!
//! 2. **「没设过」不是错误。** [`get`] 回 `Ok(None)`、[`delete`] 回 `Ok(())`。keyring 的
//!    `NoEntry` 在这里是正常状态(用户还没填),当成错误会让设置页第一次打开就弹一条
//!    报错。
//!
//! 3. **空串等于清除。** 设置页把输入框清空再保存的意思是「不要这个 key 了」,而存一个
//!    空串会让下游那条「key 为空时从钥匙串补」永远补出一个空值 —— provider 拿到空
//!    Authorization 头回 401,而用户看到的是自己刚清空过的输入框。
//!
//! ## 两个平台差异
//!
//! - **Windows** 的凭据管理器会明文显示条目**名**(`com.aeroric.desktop
//!   notebook:embedding:key`)。那是 OS 的行为不是泄露,条目的**值**仍然受 DPAPI 保护。
//! - **Linux** 用的是内核 keyutils(见 `Cargo.toml` 的 feature 说明),key 存在会话
//!   keyring 里,**重启或注销后要重填**。要改成持久的得启 `sync-secret-service`,那会
//!   给 Linux 构建加一个 libdbus-1-dev 的系统依赖 —— 两个 workflow 现在都没装。
//!
//! ## 为什么没有往返的单元测试
//!
//! 真的读写会碰用户自己的钥匙串(macOS 上还会弹授权框),而 keyring 的 mock 后端每次
//! `Entry::new` 建一份新的凭据、不共享状态,`set` 之后 `get` 不回来 —— 往返测不了。
//! 真正容易错的那一步(什么时候该去补 key)抽成了纯函数,测在
//! `notebook::rag::commands` 里。

use keyring::Entry;

/// 钥匙串里的服务名。与 `tauri.conf.json` 的 identifier 一致,这样用户在钥匙串里看到
/// 的归属和应用对得上。
const SERVICE: &str = "com.aeroric.desktop";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|error| error.to_string())
}

/// 写入。空串视为清除,见不变量 3。
pub(crate) fn set(account: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return delete(account);
    }
    entry(account)?
        .set_password(value)
        .map_err(|error| error.to_string())
}

/// 读取。**返回值不许送去前端**,见不变量 1。
pub(crate) fn get(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// 设过没有。
///
/// 读失败(钥匙串没解锁、平台上没有可用后端)一律当成「没有」:这个答案只用来决定设置
/// 页显示「已保存」还是一个空输入框,而在那两种情况下让用户重填一次正是对的反应。
pub(crate) fn has(account: &str) -> bool {
    matches!(get(account), Ok(Some(value)) if !value.is_empty())
}

/// 删除。没设过也算成功,见不变量 2。
pub(crate) fn delete(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
