//! Windows 远端的 SFTP 命令构建器,与 `sftp.rs` 里的 POSIX 构建器一一对应。
//!
//! 关键约束:**stdout 必须与 POSIX 侧逐字节同构**。`parse_remote_entries` 和
//! `parse_remote_directory_summary` 是单份实现,两个 OS 共用;一旦这里的字段顺序、
//! 分隔符或换行与 `sh` 那套不一致,解析器就得分叉成两份。所以每个构建器上方都标注了
//! 它要复刻的 POSIX 行为。
//!
//! **未在真实 Windows 主机上跑过。** 下面的实现决定基于 PowerShell / Win32 的文档化
//! 行为,但整条链路(探测 → 建命令 → ssh 往返 → 解析)没有端到端验证过;单元测试只断言
//! 生成的脚本文本,不能证明远端接受这些 cmdlet。第一次接真机时优先复核标了 (?) 的几条。
//!
//! 1. 脚本一律经 `remote_os::build_powershell_command` 走 `-EncodedCommand`,命令行里
//!    只剩 base64,cmd.exe / PowerShell / sh 谁做默认 shell 都不改变语义。
//! 2. stdout 走 `[Console]::OpenStandardOutput()` 写裸 UTF-8 字节,不用 `Write-Output`。
//!    默认输出管线会按 host 编码转码、给行尾加 CRLF、还可能插 BOM —— 三件事都会破坏
//!    上面那条同构约束。(?) 需在真机确认 stdout 无 CR、无 BOM。
//! 3. stderr 同样走裸 UTF-8 字节。`[Console]::Error.WriteLine` 用的是
//!    `[Console]::OutputEncoding`,在非英文 Windows 上通常是 OEM 代码页(简中为 GBK),
//!    而 `run_ssh_output` 用 `String::from_utf8_lossy` 读 stderr,会出乱码。
//! 4. `$ProgressPreference = 'SilentlyContinue'`。PowerShell 在 stderr 被重定向时会把
//!    进度记录序列化成 CLIXML(`#< CLIXML` + `<Objs>`),混进真正的错误文本里。
//! 5. `$ErrorActionPreference = 'Stop'` + `try/catch { stderr; exit 1 }`。默认情况下
//!    PowerShell 的非终止错误只打印不改退出码,进程仍然 exit 0,于是 `run_ssh_output`
//!    会把失败当成功、把错误文本当数据喂给解析器。
//! 6. 路径的取名/拼接用 `[IO.Path]::GetFileName` / `[IO.Path]::Combine`,不用
//!    `Split-Path` / `Join-Path`:`Split-Path` 的 `LeafSet` 参数集只接受 `-Path`,
//!    `-LiteralPath -Leaf` 是无法解析的组合;而 `-Path` 会做通配符展开,`a[1]` 这种
//!    合法文件名会被当成字符类。.NET 那两个是纯字符串运算,两个问题一起没了。
//! 7. `[IO.Directory]::CreateDirectory` 而非 `New-Item`:PowerShell 5.1 的 `New-Item`
//!    没有 `-LiteralPath` 参数(只有会做通配符展开的 `-Path`)。

use crate::remote_os::{
    build_powershell_command, join_remote_path, powershell_quote, remote_parent_of,
    to_windows_native_path,
};
use crate::sftp::{
    validate_entry_name, SftpConflictStrategy, MAX_SFTP_IMAGE_PREVIEW_BYTES,
    MAX_SFTP_TEXT_FILE_BYTES,
};

/// 每个脚本共用的前言。`Get-AeroricUnixSeconds` 刻意用 `[datetime]::new(1970,1,1)` 而不是
/// `[datetime]::UnixEpoch`:后者是 .NET Core 才有的静态属性,而 Windows PowerShell 5.1
/// 跑在 .NET Framework 上,没有它。
const PREAMBLE: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$AeroricOut = [Console]::OpenStandardOutput()
$AeroricErr = [Console]::OpenStandardError()
function Write-AeroricBytes($stream, [string]$text) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($text)
  $stream.Write($bytes, 0, $bytes.Length)
}
function Write-AeroricText([string]$text) { Write-AeroricBytes $AeroricOut $text }
function Get-AeroricUnixSeconds($time) {
  [int64][math]::Floor(($time - [datetime]::new(1970, 1, 1)).TotalSeconds)
}
try {
"#;

const EPILOGUE: &str = r#"
  $AeroricOut.Flush()
} catch {
  Write-AeroricBytes $AeroricErr ($_.Exception.Message + "`n")
  $AeroricErr.Flush()
  exit 1
}
"#;

fn script(body: &str) -> String {
    build_powershell_command(&format!("{PREAMBLE}{body}{EPILOGUE}"))
}

/// POSIX 侧对应 `:`(什么都不做且退出 0)。
fn noop() -> String {
    build_powershell_command("exit 0")
}

/// 把路径列表拼成 PowerShell 数组字面量。
fn native_array(paths: &[String]) -> String {
    let items = paths
        .iter()
        .map(|path| powershell_quote(&to_windows_native_path(path)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("@({items})")
}

fn name_array(names: &[String]) -> Result<String, String> {
    let items = names
        .iter()
        .map(|name| {
            validate_entry_name(name)?;
            Ok(powershell_quote(name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!("@({})", items.join(", ")))
}

/// 复刻 `build_remote_read_dir_command`:每行 `name\ttype\tsize\tmtime`,
/// type 为 `d`/`f`,目录的 size 为空,mtime 是 Unix 秒。
///
/// `-Force` 对应 POSIX 那串 glob 里的 `./.[!.]*`(列出隐藏项)。用 StringBuilder 一次性
/// 拼完再写,避免每个条目一次系统调用 —— 大目录下这是可观测的差别。
pub(crate) fn build_windows_read_dir_command(remote_path: &str) -> String {
    let root = powershell_quote(&to_windows_native_path(remote_path));
    script(&format!(
        r#"  $lines = New-Object Text.StringBuilder
  foreach ($item in (Get-ChildItem -LiteralPath {root} -Force)) {{
    $kind = if ($item.PSIsContainer) {{ 'd' }} else {{ 'f' }}
    $size = if ($item.PSIsContainer) {{ '' }} else {{ [string]$item.Length }}
    $mtime = Get-AeroricUnixSeconds $item.LastWriteTimeUtc
    [void]$lines.Append($item.Name).Append("`t").Append($kind).Append("`t").Append($size).Append("`t").Append($mtime).Append("`n")
  }}
  Write-AeroricText $lines.ToString()"#
    ))
}

/// 复刻 `build_remote_read_text_command`:超限即失败,否则原样吐字节。
///
/// POSIX 那条在超限时靠 `[ "$size" -le N ]` 失败,stderr 是空的,用户只看到一个泛化
/// 错误。这里给出与本地/存储分支同一套措辞(`sftp.rs` 的 "File too large"),用
/// InvariantCulture 定死小数点,免得远端区域设置把 `2.1` 写成 `2,1`。
pub(crate) fn build_windows_read_text_command(remote_path: &str) -> String {
    let path = powershell_quote(&to_windows_native_path(remote_path));
    script(&format!(
        r#"  $item = Get-Item -LiteralPath {path} -Force
  if ($item.PSIsContainer) {{ throw 'Path must be a file' }}
  if ($item.Length -gt {MAX_SFTP_TEXT_FILE_BYTES}) {{
    throw [string]::Format([Globalization.CultureInfo]::InvariantCulture, 'File too large ({{0:F1}} MB)', $item.Length / 1048576)
  }}
  $stream = [IO.File]::OpenRead($item.FullName)
  try {{ $stream.CopyTo($AeroricOut) }} finally {{ $stream.Dispose() }}"#
    ))
}

/// 复刻 `build_remote_image_preview_command`:超限即失败,否则输出 base64。
///
/// POSIX `base64` 会按 76 列折行,`[Convert]::ToBase64String` 输出单行 —— 两者都能被
/// `sftp_read_image_preview` 里那道 `filter(|ch| !ch.is_whitespace())` 吃掉,
/// 所以这个差异是允许的。
pub(crate) fn build_windows_image_preview_command(remote_path: &str) -> String {
    let path = powershell_quote(&to_windows_native_path(remote_path));
    script(&format!(
        r#"  $item = Get-Item -LiteralPath {path} -Force
  if ($item.PSIsContainer) {{ throw 'Path must be a file' }}
  if ($item.Length -gt {MAX_SFTP_IMAGE_PREVIEW_BYTES}) {{
    throw [string]::Format([Globalization.CultureInfo]::InvariantCulture, 'Image too large ({{0:F1}} MB)', $item.Length / 1048576)
  }}
  Write-AeroricText ([Convert]::ToBase64String([IO.File]::ReadAllBytes($item.FullName)))"#
    ))
}

/// 复刻 `build_remote_directory_summary_command`:`files\tdirs\tbytes\tmtime\n`。
///
/// `dirs` 不含根自身(POSIX 那边是 `find -type d | sed 1d`),`-Recurse` 天然不含根。
/// `-ErrorAction SilentlyContinue` 对应 POSIX 的 `2>/dev/null`:遍历里碰到无权限的子树
/// 时跳过而不是整体失败。
pub(crate) fn build_windows_directory_summary_command(remote_path: &str) -> String {
    let root = powershell_quote(&to_windows_native_path(remote_path));
    script(&format!(
        r#"  $rootItem = Get-Item -LiteralPath {root} -Force
  if (-not $rootItem.PSIsContainer) {{ throw 'Path must be a directory' }}
  $files = [int64]0
  $dirs = [int64]0
  $bytes = [int64]0
  foreach ($item in (Get-ChildItem -LiteralPath {root} -Recurse -Force -ErrorAction SilentlyContinue)) {{
    if ($item.PSIsContainer) {{ $dirs++ }} else {{ $files++; $bytes += $item.Length }}
  }}
  Write-AeroricText ("$files`t$dirs`t$bytes`t" + (Get-AeroricUnixSeconds $rootItem.LastWriteTimeUtc) + "`n")"#
    ))
}

/// 复刻 `build_remote_create_dir_command`(`mkdir` 不带 `-p`):父目录必须已存在,
/// 目标必须不存在。
///
/// 用 `[IO.Directory]::CreateDirectory` 而不是 `New-Item`:PowerShell 5.1 的 `New-Item`
/// 没有 `-LiteralPath` 参数(只有 `-Path`,且会做通配符展开)。
/// `CreateDirectory` 本身是 `mkdir -p` 语义,所以上面那两个前置检查不是防御性代码,
/// 而是维持"不带 -p"这一既有语义的必要条件。
pub(crate) fn build_windows_create_dir_command(remote_path: &str) -> String {
    let path = powershell_quote(&to_windows_native_path(remote_path));
    let parent = powershell_quote(&to_windows_native_path(&remote_parent_of(remote_path)));
    script(&format!(
        r#"  if (Test-Path -LiteralPath {path}) {{ throw 'A file or folder with that name already exists' }}
  if (-not (Test-Path -LiteralPath {parent} -PathType Container)) {{ throw 'Cannot resolve parent directory' }}
  [void][IO.Directory]::CreateDirectory({path})"#
    ))
}

/// 复刻 `build_remote_delete_command`(`rm -rf`)。
///
/// `rm -rf` 对不存在的路径静默成功,但对权限等真实错误返回非零。所以这里先 `Test-Path`
/// 跳过不存在的项,剩下的交给 `Remove-Item` 抛错 —— 而不是一刀切
/// `-ErrorAction SilentlyContinue`,那会把"删不掉"也伪装成成功。
pub(crate) fn build_windows_delete_command(paths: &[String]) -> String {
    if paths.is_empty() {
        return noop();
    }
    let targets = native_array(paths);
    script(&format!(
        r#"  foreach ($path in {targets}) {{
    if (Test-Path -LiteralPath $path) {{ Remove-Item -LiteralPath $path -Recurse -Force }}
  }}"#
    ))
}

/// 复刻 `build_remote_conflict_check_command`。错误文本与 POSIX 侧逐字一致 ——
/// 它会经 stderr 原样冒到前端提示里。
pub(crate) fn build_windows_conflict_check_command(
    names: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if names.is_empty() {
        return Ok(noop());
    }
    let target = powershell_quote(&to_windows_native_path(target_directory));
    let names = name_array(names)?;
    Ok(script(&format!(
        r#"  if (-not (Test-Path -LiteralPath {target} -PathType Container)) {{ throw 'Target directory does not exist' }}
  foreach ($name in {names}) {{
    if (Test-Path -LiteralPath ([IO.Path]::Combine({target}, $name))) {{ throw 'A file or folder with that name already exists' }}
  }}"#
    )))
}

/// 复刻 `build_remote_delete_target_names_command`(Replace 策略的前置清理)。
pub(crate) fn build_windows_delete_target_names_command(
    names: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if names.is_empty() {
        return Ok(noop());
    }
    let target = powershell_quote(&to_windows_native_path(target_directory));
    let names = name_array(names)?;
    Ok(script(&format!(
        r#"  if (-not (Test-Path -LiteralPath {target} -PathType Container)) {{ throw 'Target directory does not exist' }}
  foreach ($name in {names}) {{
    $victim = [IO.Path]::Combine({target}, $name)
    if (Test-Path -LiteralPath $victim) {{ Remove-Item -LiteralPath $victim -Recurse -Force }}
  }}"#
    )))
}

/// 复刻 POSIX 递归 Merge 预检:目标不存在放行;文件↔文件 / 目录→文件 /
/// 文件→已存在目录 拒绝;目录+目录逐子项递归(嵌套同名文件同样拒绝)。
pub(crate) fn build_windows_merge_conflict_check_command(
    names: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if names.is_empty() {
        return Ok(noop());
    }
    let target = powershell_quote(&to_windows_native_path(target_directory));
    let names = name_array(names)?;
    // 仅名字时无法拿到远端源树,退化成顶层检查;完整源路径的递归预检内联在
    // `build_windows_copy_or_move_command` 的 `Test-AeroricMerge` 里。
    Ok(script(&format!(
        r#"  if (-not (Test-Path -LiteralPath {target} -PathType Container)) {{ throw 'Target directory does not exist' }}
  foreach ($name in {names}) {{
    $candidate = [IO.Path]::Combine({target}, $name)
    if ((Test-Path -LiteralPath $candidate) -and -not (Test-Path -LiteralPath $candidate -PathType Container)) {{
      throw 'Cannot merge a file into an existing file'
    }}
  }}"#
    )))
}

const WINDOWS_MERGE_CHECK_FN: &str = r#"  function Test-AeroricMerge([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Destination)) { return }
    $srcItem = Get-Item -LiteralPath $Source -Force
    $destItem = Get-Item -LiteralPath $Destination -Force
    if (-not $destItem.PSIsContainer) {
      if ($srcItem.PSIsContainer) { throw 'Cannot merge a directory into a file' }
      else { throw 'Cannot merge a file into an existing file' }
    }
    if (-not $srcItem.PSIsContainer) { throw 'Cannot merge a file into an existing directory' }
    foreach ($child in (Get-ChildItem -LiteralPath $Source -Force)) {
      $childDest = [IO.Path]::Combine($Destination, $child.Name)
      Test-AeroricMerge $child.FullName $childDest
    }
  }
"#;

/// 复刻 `build_remote_rename_command`。目标路径在 Rust 侧算好,用的是
/// `remote_os::remote_parent_of` 而不是 `sftp.rs::remote_parent` —— 后者按 `/` 切分,
/// 在盘根 `C:/x` 上会给出 `C:`。
pub(crate) fn build_windows_rename_command(
    remote_path: &str,
    new_name: &str,
) -> Result<String, String> {
    validate_entry_name(new_name)?;
    let destination = join_remote_path(&remote_parent_of(remote_path), new_name);
    let source = powershell_quote(&to_windows_native_path(remote_path));
    let destination = powershell_quote(&to_windows_native_path(&destination));
    Ok(script(&format!(
        r#"  if (Test-Path -LiteralPath {destination}) {{ throw 'A file or folder with that name already exists' }}
  Move-Item -LiteralPath {source} -Destination {destination}"#
    )))
}

/// 复刻 `build_remote_copy_or_move_command`。
///
/// 递归合并那段不是多余的复杂度,而是 `Copy-Item` 与 `cp -R` 的语义差:
/// `cp -R src target/` 在 `target/src` 已存在且同为目录时**递归合并**,而
/// `Copy-Item src -Destination target -Recurse` 会把 src 塞进去变成 `target/src/src`。
/// 所以目录+目录这一种情况必须自己按子项递归下去,其余情况才交给 `Copy-Item`/`Move-Item`。
///
/// Merge 预检与 POSIX 递归检查对齐:文件→已存在文件、文件→已存在目录、
/// 目录→文件、以及**任意深度**的嵌套同名文件都会在任何删除/复制之前失败。
pub(crate) fn build_windows_copy_or_move_command(
    source_paths: &[String],
    target_directory: &str,
    move_paths: bool,
    conflict_strategy: SftpConflictStrategy,
) -> Result<String, String> {
    if source_paths.is_empty() {
        return Ok(noop());
    }
    for source in source_paths {
        let name = source
            .rsplit_once('/')
            .map(|(_, name)| name)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| "Invalid file name".to_string())?;
        validate_entry_name(name)?;
        if remote_parent_of(source) == target_directory {
            return Err("Cannot replace a file or folder with itself".to_string());
        }
    }
    let target = powershell_quote(&to_windows_native_path(target_directory));
    let sources = native_array(source_paths);
    let merge_preflight = match conflict_strategy {
        SftpConflictStrategy::Merge => {
            format!(
                "{WINDOWS_MERGE_CHECK_FN}  foreach ($source in {sources}) {{\n    $destination = [IO.Path]::Combine({target}, [IO.Path]::GetFileName($source))\n    Test-AeroricMerge $source $destination\n  }}\n"
            )
        }
        _ => {
            let preflight = match conflict_strategy {
                SftpConflictStrategy::Fail => {
                    "    if (Test-Path -LiteralPath $destination) { throw 'A file or folder with that name already exists' }"
                }
                SftpConflictStrategy::Merge => unreachable!(),
                SftpConflictStrategy::Replace => {
                    "    if (Test-Path -LiteralPath $destination) { Remove-Item -LiteralPath $destination -Recurse -Force }"
                }
            };
            format!(
                "  foreach ($source in {sources}) {{\n    $destination = [IO.Path]::Combine({target}, [IO.Path]::GetFileName($source))\n{preflight}\n  }}\n"
            )
        }
    };
    let drain = if move_paths {
        "\n      Remove-Item -LiteralPath $source -Recurse -Force"
    } else {
        ""
    };
    let leaf = if move_paths {
        "Move-Item -LiteralPath $source -Destination $destination -Force"
    } else {
        "Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force"
    };
    // Merge 预检通过后:文件↔文件 / 文件→目录 / 目录→文件都不可能再出现,
    // 因此目录+目录走子项递归,目标缺失走 Copy/Move-Item 即可。
    Ok(script(&format!(
        r#"  if (-not (Test-Path -LiteralPath {target} -PathType Container)) {{ throw 'Target directory does not exist' }}
  function Copy-AeroricInto([string]$source, [string]$targetDir) {{
    $destination = [IO.Path]::Combine($targetDir, [IO.Path]::GetFileName($source))
    if ((Get-Item -LiteralPath $source -Force).PSIsContainer -and (Test-Path -LiteralPath $destination -PathType Container)) {{
      foreach ($child in (Get-ChildItem -LiteralPath $source -Force)) {{ Copy-AeroricInto $child.FullName $destination }}{drain}
    }} elseif ((Get-Item -LiteralPath $source -Force).PSIsContainer -and (Test-Path -LiteralPath $destination)) {{
      throw 'Cannot merge a directory into a file'
    }} elseif (-not (Get-Item -LiteralPath $source -Force).PSIsContainer -and (Test-Path -LiteralPath $destination -PathType Container)) {{
      throw 'Cannot merge a file into an existing directory'
    }} elseif (-not (Get-Item -LiteralPath $source -Force).PSIsContainer -and (Test-Path -LiteralPath $destination)) {{
      throw 'Cannot merge a file into an existing file'
    }} else {{
      {leaf}
    }}
  }}
{merge_preflight}  foreach ($source in {sources}) {{ Copy-AeroricInto $source {target} }}"#
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_os::RemoteOs;

    /// 测试断言的是解码后的脚本正文。只对 base64 串做断言等于什么都没验 ——
    /// 这个 helper 把 `-EncodedCommand` 还原回脚本,让下面的断言落在真实内容上。
    fn decode(command: &str) -> String {
        let encoded = command
            .rsplit_once(' ')
            .expect("command carries an -EncodedCommand payload")
            .1;
        let bytes = crate::remote_os::tests_decode_base64(encoded);
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).expect("script decodes as UTF-16LE")
    }

    /// 这四行各自封住一个失败模式,少一行就会退回那个失败:
    /// 没有 `Stop` → 错误被当数据;没有 `exit 1` → 失败被当成功;
    /// 没有 `SilentlyContinue` → stderr 里混进 CLIXML;
    /// stderr 不走 UTF-8 字节 → 非 ASCII 错误文本变乱码。
    #[test]
    fn every_builder_carries_the_four_guards() {
        let commands = vec![
            build_windows_read_dir_command("C:/Users"),
            build_windows_read_text_command("C:/Users/a.txt"),
            build_windows_image_preview_command("C:/Users/a.png"),
            build_windows_directory_summary_command("C:/Users"),
            build_windows_create_dir_command("C:/Users/new"),
            build_windows_delete_command(&["C:/Users/a".to_string()]),
            build_windows_rename_command("C:/a.txt", "b.txt").unwrap(),
            build_windows_conflict_check_command(&["a.txt".to_string()], "C:/t").unwrap(),
            build_windows_merge_conflict_check_command(&["a.txt".to_string()], "C:/t").unwrap(),
            build_windows_delete_target_names_command(&["a.txt".to_string()], "C:/t").unwrap(),
            build_windows_copy_or_move_command(
                &["C:/s/a".to_string()],
                "C:/t",
                false,
                SftpConflictStrategy::Fail,
            )
            .unwrap(),
        ];
        for command in commands {
            assert!(command.starts_with("powershell -NoProfile -NonInteractive -EncodedCommand "));
            let script = decode(&command);
            assert!(script.contains("$ErrorActionPreference = 'Stop'"));
            assert!(script.contains("$ProgressPreference = 'SilentlyContinue'"));
            assert!(script.contains("exit 1"));
            assert!(
                script.contains(r#"Write-AeroricBytes $AeroricErr ($_.Exception.Message + "`n")"#)
            );
        }
    }

    #[test]
    fn read_dir_emits_the_posix_tab_layout() {
        let script = decode(&build_windows_read_dir_command("C:/Users"));
        assert!(script.contains("Get-ChildItem -LiteralPath 'C:\\Users' -Force"));
        // 字段顺序与 parse_remote_entries 的 split('\t') 顺序绑死。
        assert!(script.contains(
            r#"Append($item.Name).Append("`t").Append($kind).Append("`t").Append($size).Append("`t").Append($mtime).Append("`n")"#
        ));
        assert!(script.contains("if ($item.PSIsContainer) { 'd' } else { 'f' }"));
    }

    #[test]
    fn read_text_and_preview_carry_the_shared_size_caps() {
        let text = decode(&build_windows_read_text_command("C:/a.txt"));
        assert!(text.contains(&format!("-gt {MAX_SFTP_TEXT_FILE_BYTES}")));
        let preview = decode(&build_windows_image_preview_command("C:/a.png"));
        assert!(preview.contains(&format!("-gt {MAX_SFTP_IMAGE_PREVIEW_BYTES}")));
    }

    #[test]
    fn directory_summary_emits_four_tab_separated_counters() {
        let script = decode(&build_windows_directory_summary_command("C:/Users"));
        assert!(script.contains(r#"Write-AeroricText ("$files`t$dirs`t$bytes`t""#));
        assert!(script.contains("-Recurse -Force -ErrorAction SilentlyContinue"));
    }

    /// PowerShell 5.1 的 `New-Item` 没有 `-LiteralPath`。这条钉住替代方案,
    /// 同时钉住"不是 mkdir -p"的两道前置检查。
    #[test]
    fn create_dir_avoids_new_item_and_refuses_to_act_like_mkdir_p() {
        let script = decode(&build_windows_create_dir_command("C:/Users/new"));
        assert!(!script.contains("New-Item"));
        assert!(script.contains("[IO.Directory]::CreateDirectory('C:\\Users\\new')"));
        assert!(script.contains("if (Test-Path -LiteralPath 'C:\\Users\\new') { throw"));
        assert!(script.contains("-LiteralPath 'C:\\Users' -PathType Container"));
    }

    /// `Split-Path -LiteralPath X -Leaf` 是无法解析的参数组合(LeafSet 只接受 `-Path`),
    /// 而 `-Path` 会把 `a[1]` 当字符类展开。两个都不能用。
    #[test]
    fn path_math_uses_dotnet_not_cmdlets() {
        let script = decode(
            &build_windows_copy_or_move_command(
                &["C:/s/a".to_string()],
                "C:/t",
                false,
                SftpConflictStrategy::Fail,
            )
            .unwrap(),
        );
        assert!(!script.contains("Split-Path"));
        assert!(!script.contains("Join-Path"));
        assert!(script.contains("[IO.Path]::Combine($targetDir, [IO.Path]::GetFileName($source))"));

        let conflict =
            decode(&build_windows_conflict_check_command(&["a.txt".to_string()], "C:/t").unwrap());
        assert!(!conflict.contains("Join-Path"));
        assert!(conflict.contains("[IO.Path]::Combine('C:\\t', $name)"));
    }

    #[test]
    fn delete_skips_missing_paths_but_still_reports_real_failures() {
        let script = decode(&build_windows_delete_command(&[
            "C:/a".to_string(),
            "C:/b".to_string(),
        ]));
        assert!(script.contains("@('C:\\a', 'C:\\b')"));
        assert!(script.contains("if (Test-Path -LiteralPath $path) { Remove-Item"));
        assert!(!script.contains("Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction"));
    }

    #[test]
    fn conflict_messages_match_the_posix_wording_byte_for_byte() {
        let fail =
            build_windows_conflict_check_command(&["a.txt".to_string()], "C:/target").unwrap();
        assert!(decode(&fail).contains("throw 'A file or folder with that name already exists'"));
        let merge = build_windows_merge_conflict_check_command(&["a.txt".to_string()], "C:/target")
            .unwrap();
        assert!(decode(&merge).contains("throw 'Cannot merge a file into an existing file'"));
    }

    #[test]
    fn empty_input_becomes_a_no_op_like_the_posix_colon() {
        assert_eq!(
            build_windows_conflict_check_command(&[], "C:/target").unwrap(),
            build_powershell_command("exit 0")
        );
        assert_eq!(
            build_windows_copy_or_move_command(&[], "C:/target", false, SftpConflictStrategy::Fail)
                .unwrap(),
            build_powershell_command("exit 0")
        );
        assert_eq!(
            build_windows_delete_command(&[]),
            build_powershell_command("exit 0")
        );
    }

    #[test]
    fn rename_resolves_the_destination_at_the_drive_root() {
        let script = decode(&build_windows_rename_command("C:/a.txt", "b.txt").unwrap());
        assert!(script.contains("-LiteralPath 'C:\\a.txt' -Destination 'C:\\b.txt'"));
    }

    #[test]
    fn rename_rejects_path_separators_in_the_new_name() {
        assert!(build_windows_rename_command("C:/a.txt", "sub/b.txt").is_err());
        assert!(build_windows_rename_command("C:/a.txt", "sub\\b.txt").is_err());
    }

    #[test]
    fn copy_merges_directories_instead_of_nesting_them() {
        let script = decode(
            &build_windows_copy_or_move_command(
                &["C:/src/dir".to_string()],
                "C:/target",
                false,
                SftpConflictStrategy::Merge,
            )
            .unwrap(),
        );
        // 目录+目录走子项递归,这正是 cp -R 的合并语义;其余情况才落到 Copy-Item。
        assert!(script.contains("foreach ($child in (Get-ChildItem -LiteralPath $source -Force)) { Copy-AeroricInto $child.FullName $destination }"));
        assert!(script
            .contains("Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force"));
        assert!(script.contains("function Test-AeroricMerge"));
        assert!(script.contains("throw 'Cannot merge a file into an existing file'"));
        assert!(script.contains("throw 'Cannot merge a directory into a file'"));
        assert!(script.contains("throw 'Cannot merge a file into an existing directory'"));
        // 复制不该删源。
        assert!(!script.contains("Remove-Item -LiteralPath $source"));
    }

    #[test]
    fn move_removes_the_drained_source_directory() {
        let script = decode(
            &build_windows_copy_or_move_command(
                &["C:/src/dir".to_string()],
                "C:/target",
                true,
                SftpConflictStrategy::Replace,
            )
            .unwrap(),
        );
        assert!(script.contains("Move-Item -LiteralPath $source -Destination $destination -Force"));
        assert!(script.contains("Remove-Item -LiteralPath $source -Recurse -Force"));
        assert!(script.contains("if (Test-Path -LiteralPath $destination) { Remove-Item -LiteralPath $destination -Recurse -Force }"));
    }

    #[test]
    fn copy_into_its_own_parent_is_rejected() {
        let result = build_windows_copy_or_move_command(
            &["C:/target/a.txt".to_string()],
            "C:/target",
            false,
            SftpConflictStrategy::Fail,
        );

        assert_eq!(
            result,
            Err("Cannot replace a file or folder with itself".to_string())
        );
    }

    #[test]
    fn single_quotes_in_names_cannot_break_out_of_the_literal() {
        let script = decode(
            &build_windows_conflict_check_command(&["it's.txt".to_string()], "C:/t").unwrap(),
        );
        assert!(script.contains("@('it''s.txt')"));
    }

    /// 路径模型是共享的,这里只钉住"构建器确实用的是原生反斜杠形式"这一点。
    #[test]
    fn scripts_receive_native_backslash_paths() {
        assert_eq!(
            to_windows_native_path("C:/Users/Administrator/Documents"),
            "C:\\Users\\Administrator\\Documents"
        );
        let script = decode(&build_windows_read_dir_command(
            "C:\\Users\\Administrator\\Documents",
        ));
        assert!(script.contains("'C:\\Users\\Administrator\\Documents'"));
        assert_eq!(
            crate::remote_os::validate_remote_path("C:/x", RemoteOs::Windows),
            Ok("C:/x".to_string())
        );
    }
}
