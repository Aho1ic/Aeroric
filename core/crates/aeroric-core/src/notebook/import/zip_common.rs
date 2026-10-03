//! zip 型 provider 共用的落盘动作。
//!
//! Bear 和 Roam 的 markdown 那一路都是「按归档里的相对路径原样落下来」,Notion 只多一步
//! 名字清洗与链接重写。把落盘这一段单独拎出来,是为了让**指纹只在写成功之后记**这条规则
//! 只有一处实现 —— 写失败还记指纹的话,重试那一轮会把这条当成「导过了」跳掉,那篇笔记就
//! 永远进不来。

use std::path::Path;

use super::landing;
use super::manifest::{self, Session};
use super::report::SkipReason;
use super::zip_src::Handled;

/// 原样落盘:保留归档里的目录结构,不改内容。
pub fn land_verbatim(
    provider: &str,
    session: &mut Session,
    dest_dir: &Path,
    relative: &str,
    bytes: Vec<u8>,
) -> Handled {
    let key = manifest::fingerprint(&format!("{provider}::{relative}"));
    if session.is_known(&key) {
        return Handled::Skipped(SkipReason::AlreadyImported);
    }
    match land_bytes(dest_dir, relative, &bytes) {
        Ok(dest) => {
            session.record(key);
            Handled::Landed {
                dest: dest_relative(provider, dest_dir, &dest),
                issues: Vec::new(),
            }
        }
        Err(detail) => Handled::Failed(detail),
    }
}

/// 把字节写到 `dest_dir/relative`,重名不覆盖。返回实际落点的绝对路径。
pub fn land_bytes(
    dest_dir: &Path,
    relative: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, String> {
    let path = Path::new(relative);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".to_string());
    let mut dir = dest_dir.to_path_buf();
    if let Some(parent) = path.parent() {
        for component in parent.components() {
            dir.push(landing::sanitize_name(
                &component.as_os_str().to_string_lossy(),
            ));
        }
    }
    std::fs::create_dir_all(&dir).map_err(|error| format!("建目录失败:{error}"))?;
    let target = landing::unique_path(&dir, &name);
    std::fs::write(&target, bytes).map_err(|error| format!("写入失败:{error}"))?;
    Ok(target)
}

/// 落点的 vault 相对路径。
///
/// 不能直接拿归档里的相对路径拼:`unique_path` 可能把名字改成了 `a-2.md`,而报告里的
/// `dest` 是用户要拿去在 vault 里找文件的。
pub fn dest_relative(provider: &str, dest_dir: &Path, target: &Path) -> String {
    let prefix = landing::provider_dir(provider);
    match target.strip_prefix(dest_dir) {
        Ok(rest) => format!("{prefix}/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => prefix,
    }
}
