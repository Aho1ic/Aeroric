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
//!
//! 三条不变量本身不需要碰钥匙串就能测:空串分派([`set_with`])与 `NoEntry` 归一化
//! ([`normalize_missing`])都是纯判断,不变量 1 由本模块测试里的源码级守卫盯住。

use keyring::Entry;

/// 钥匙串里的服务名。与 `tauri.conf.json` 的 identifier 一致,这样用户在钥匙串里看到
/// 的归属和应用对得上。
const SERVICE: &str = "com.aeroric.desktop";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|error| error.to_string())
}

/// [`set`] 的分派:空串走 clear,其余走 write。见不变量 3。
///
/// 两个副作用做成参数,与 `notebook::rag::commands::with_stored_key` 同一手法 —— 真的
/// 写钥匙串测不了(见模块注释末节),而这条分派一旦反了,症状(下游补出一个空 key,
/// provider 回 401)离原因很远。
fn set_with(
    value: &str,
    write: impl FnOnce(&str) -> Result<(), String>,
    clear: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if value.is_empty() {
        return clear();
    }
    write(value)
}

/// 把 keyring 的结果归一化:`NoEntry` 不是错误。见不变量 2。
///
/// `get` 与 `delete` 共用同一条判断 —— 「没设过」在这两条路上都是正常状态。
fn normalize_missing<T>(result: Result<T, keyring::Error>, missing: T) -> Result<T, String> {
    match result {
        Ok(value) => Ok(value),
        Err(keyring::Error::NoEntry) => Ok(missing),
        Err(error) => Err(error.to_string()),
    }
}

/// 写入。空串视为清除,见不变量 3。
pub(crate) fn set(account: &str, value: &str) -> Result<(), String> {
    set_with(
        value,
        |value| {
            entry(account)?
                .set_password(value)
                .map_err(|error| error.to_string())
        },
        || delete(account),
    )
}

/// 读取。**返回值不许送去前端**,见不变量 1。
pub(crate) fn get(account: &str) -> Result<Option<String>, String> {
    normalize_missing(entry(account)?.get_password().map(Some), None)
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
    normalize_missing(entry(account)?.delete_credential(), ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;

    #[test]
    fn an_empty_set_is_a_delete() {
        let mut wrote = None;
        let mut cleared = false;
        set_with(
            "",
            |value| {
                wrote = Some(value.to_string());
                Ok(())
            },
            || {
                cleared = true;
                Ok(())
            },
        )
        .expect("空串清除不是错误");
        assert!(cleared, "空串必须走 clear,等价于 delete");
        assert!(wrote.is_none(), "空串不得写入钥匙串");
    }

    #[test]
    fn a_non_empty_set_is_a_write() {
        let mut wrote = None;
        let mut cleared = false;
        set_with(
            "sk-test",
            |value| {
                wrote = Some(value.to_string());
                Ok(())
            },
            || {
                cleared = true;
                Ok(())
            },
        )
        .expect("写入不是错误");
        assert_eq!(wrote.as_deref(), Some("sk-test"));
        assert!(!cleared, "非空串不得走 clear");
    }

    /// 不变量 2:「没设过」不是错误。`delete` 整个函数体就是这条归一化。
    #[test]
    fn deleting_a_missing_entry_succeeds() {
        assert_eq!(normalize_missing(Err(keyring::Error::NoEntry), ()), Ok(()));
        assert_eq!(
            normalize_missing::<Option<String>>(Err(keyring::Error::NoEntry), None),
            Ok(None)
        );
        assert_eq!(normalize_missing(Ok(()), ()), Ok(()));
    }

    /// 只有 `NoEntry` 算「正常缺失」。钥匙串没解锁 / 平台没后端必须上抛,否则删除失败
    /// 会被报成删除成功。
    #[test]
    fn a_real_keyring_error_is_not_treated_as_missing() {
        let error = keyring::Error::Invalid("account".into(), "empty".into());
        let result: Result<(), String> = normalize_missing(Err(error), ());
        assert!(result.is_err());
    }

    /// 不变量 1 的源码级守卫。
    ///
    /// 能抓到:
    /// - 任何文件里 `secrets::get` / `crate::secrets::get` 的出现位置,必须与允许清单
    ///   (现在只有 `resolve_key`)完全一致。新增读者 = 强制 review。
    /// - `use crate::secrets::get`(含 `{get, ...}` 分组与 `as` 重命名)、
    ///   `use crate::secrets as ...`、`use crate::secrets::*`:这些能让后续裸 `get(...)`
    ///   或 `别名::get` 逃过字面量扫描,直接禁掉。
    /// - `keyring::` 出现在本文件以外:绕开本模块直接碰钥匙串。
    /// - 本文件自己定义 `#[tauri::command]`:同文件裸 `get(...)` 的唯一入口。
    ///
    /// 抓不到(照实说,别给虚假的安心):
    /// - **数据流**。`resolve_key` 返回的 `EmbedConfig` 带着明文 `api_key`,某个 command
    ///   若把它原样 serialize 回前端,这条守卫看不见。当前没有 command 返回
    ///   `EmbedConfig`(全都只回 `usize` / `IndexStats` / 命中列表),那是返回类型在挡,
    ///   不是这条测试在挡。
    /// - 宏展开后才出现的调用、`include!` 进来的代码。
    /// - 字符串字面量里的 `//` 会让该行后半段被当成注释剥掉。
    #[test]
    fn no_tauri_command_calls_secrets_get() {
        let root = crate::command_registration_tests::source_root();
        let mut files = Vec::new();
        crate::command_registration_tests::rust_sources(&root, &mut files);
        files.sort();
        // 解析器静默扫到空集合会让下面的断言变成永真。
        assert!(
            files.len() > 50,
            "只扫到 {} 个 .rs,源码遍历大概率坏了",
            files.len()
        );

        let allowlist: BTreeSet<(String, String)> = [("notebook/rag/commands.rs", "resolve_key")]
            .into_iter()
            .map(|(file, name)| (file.to_owned(), name.to_owned()))
            .collect();

        let mut found = BTreeSet::new();
        for file in &files {
            let relative = file
                .strip_prefix(&root)
                .unwrap_or(file.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("read {file:?}: {e}"));
            let lines: Vec<&str> = text.lines().collect();

            // 本文件:明文读取就在这里,路径扫描对它没有意义(裸 `get(...)` 就够)。
            // 改成禁止在这里定义 command —— 那是同文件裸调用能被前端摸到的唯一入口。
            if relative == "secrets.rs" {
                for (index, line) in lines.iter().enumerate() {
                    assert!(
                        !line.trim_start().starts_with("#[tauri::command"),
                        "secrets.rs:{} 定义了 #[tauri::command]。明文读取就在本文件,\
                         同文件 command 用裸 get() 即可绕过路径扫描",
                        index + 1
                    );
                }
                continue;
            }

            for (index, line) in lines.iter().enumerate() {
                let code = strip_line_comment(line);
                assert!(
                    !hides_secrets_get(code),
                    "{relative}:{} 把 secrets 模块或 get 重命名/glob 导入了,\
                     后续调用会逃过字面量扫描:\n  {line}",
                    index + 1
                );
                assert!(
                    !code.contains("keyring::"),
                    "{relative}:{} 直接提到 keyring::。钥匙串只许经 crate::secrets 进出:\n  {line}",
                    index + 1
                );
                if code.contains("secrets::get") {
                    let name = enclosing_fn(&lines, index).unwrap_or_else(|| "<module>".to_owned());
                    found.insert((relative.clone(), name));
                }
            }
        }

        assert_eq!(
            found, allowlist,
            "secrets::get 的出现位置必须与允许清单一致。新增读者请先确认它的返回值\
             不会送到前端,再把 (文件, 函数名) 加进清单"
        );
    }

    /// 去掉行注释。`https://` 这类「冒号紧跟 //」不算注释起点。
    fn strip_line_comment(line: &str) -> &str {
        let mut from = 0;
        while let Some(offset) = line[from..].find("//") {
            let at = from + offset;
            if !line[..at].ends_with(':') {
                return &line[..at];
            }
            from = at + 2;
        }
        line
    }

    /// 这行 `use` 是否让 `secrets::get` 能以别的写法出现。
    fn hides_secrets_get(code: &str) -> bool {
        let text = code.trim();
        if !text.starts_with("use ") || !text.contains("secrets") {
            return false;
        }
        if text.contains("secrets as ") || text.contains("secrets::*") {
            return true;
        }
        text.split("secrets::").skip(1).any(|after| {
            after
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|token| token == "get")
        })
    }

    /// 往上找最近的 fn 声明,用来把一处命中归给某个函数。
    fn enclosing_fn(lines: &[&str], index: usize) -> Option<String> {
        lines[..index].iter().rev().find_map(|line| {
            let text = strip_line_comment(line).trim();
            if text.starts_with('#') || text.starts_with("use ") {
                return None;
            }
            function_name(text)
        })
    }

    fn function_name(line: &str) -> Option<String> {
        let after_fn = line.split_once("fn ")?.1;
        let name: String = after_fn
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// 守卫的三个解析器各自都会静默失效,单独钉住。
    #[test]
    fn the_guard_parsers_read_what_they_claim() {
        assert_eq!(strip_line_comment("let a = 1; // 注释"), "let a = 1; ");
        assert_eq!(
            strip_line_comment("let url = \"https://example.com\";"),
            "let url = \"https://example.com\";"
        );

        assert!(hides_secrets_get("use crate::secrets::get;"));
        assert!(hides_secrets_get("use crate::secrets::get as read_key;"));
        assert!(hides_secrets_get("use crate::secrets::{get, set};"));
        assert!(hides_secrets_get("use crate::secrets as vault;"));
        assert!(hides_secrets_get("use crate::secrets::*;"));
        assert!(!hides_secrets_get("use crate::secrets::set;"));
        assert!(!hides_secrets_get("use crate::secrets::get_status;"));
        assert!(!hides_secrets_get("crate::secrets::get(ACCOUNT)"));

        let lines = vec![
            "async fn resolve_key(config: EmbedConfig) {",
            "    spawn_blocking(move || {",
            "        crate::secrets::get(ACCOUNT)",
        ];
        assert_eq!(enclosing_fn(&lines, 2).as_deref(), Some("resolve_key"));
        assert_eq!(enclosing_fn(&lines, 0), None);
    }
}
