//! vault 内的回收站(软删)。
//!
//! 删除笔记不进系统回收站,而是 `fs::rename` 到 `<vault>/.notebook/trash/`。
//! 理由是系统回收站给不了随手记需要的两件事:
//! - **可审计**:系统回收站里只剩一个文件名,原来在 vault 的哪个子目录、什么时候
//!   删的都没了。恢复一个 `untitled.md` 时用户根本不知道该往哪放。
//! - **同 vault**:rename 在同一分区内是原子的,不搬字节;跨到系统回收站可能是
//!   跨设备复制,大 vault 上会卡住 IPC。
//!
//! 系统回收站仍然用得上 —— 它是**彻底删除**的落点(见 [`purge`])。两层的分工是
//! "vault 内可恢复 + 可审计" → "退出 vault 但仍可从 Finder 捞回来"。
//!
//! 布局:
//! ```text
//! <vault>/.notebook/trash/
//!   1712345678901.meta.json     清单:原相对路径、原名、删除时间、尺寸
//!   1712345678901.bin           载荷(文件)
//!   1712345678901.dir/          载荷(目录,整棵搬过来)
//! ```
//!
//! 载荷**不保留原名**。原名可能重复、超长、或含当前平台非法的字符(vault 可以是
//! 用户从别处挂进来的目录),用它命名会把这些问题从删除时推迟到恢复时。恢复要
//! 用的信息全在清单里。
//!
//! ID 是毫秒时间戳,同毫秒的第二条起带 `-N` 后缀。抢名字靠 `create_new` 原子地
//! 占住清单文件 —— 不是"先 exists 再写"。后者在同毫秒并发删两个同名笔记时会让
//! 两边都看到"没人占",后写的那个覆盖先写的那个,于是有一条笔记人间蒸发。

use std::io::Write;
use std::path::{Path, PathBuf};

use super::fs_ops::{self, VAULT_PRIVATE_DIR};
use super::snapshots;

/// 相对 vault 根的回收站目录。
const TRASH_DIR: &str = ".notebook/trash";

/// 编译期确认回收站还在 vault 私有目录里面 —— 否则删掉的文件会作为笔记重新
/// 出现在列表里。
const _: () = fs_ops::assert_inside_private_dir(TRASH_DIR);

/// 同毫秒内最多能塞多少条。抢到这个数还没空位就报错,而不是无限循环。
const MAX_SAME_MS_ENTRIES: u32 = 1000;

/// 目录尺寸统计的递归深度上限。只是给 UI 显示一个数,不值得为它走到天荒地老。
const MAX_SIZE_DEPTH: usize = 12;

/// 回收站里的一条。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    /// 时间戳 ID,同时是载荷和清单的文件名前缀。恢复 / 彻底删除都只认它 ——
    /// 前端不回传路径,这个入口就没法被拿去动 vault 外的东西。
    pub id: String,
    /// 删除前的文件名(含扩展名)。
    pub name: String,
    /// 删除前相对 vault 根的路径,`/` 分隔。UI 用它告诉用户"这条原来在哪"。
    pub relative_path: String,
    pub deleted_at_ms: u64,
    pub size: u64,
    pub is_dir: bool,
}

/// 清单文件的内容。
///
/// 存**相对** vault 根的路径而不是绝对路径:vault 会跟着项目移动(项目级 vault
/// 是 `<project>/.aeroric/notes`),存绝对路径的话项目一改名,整个回收站就全恢复
/// 不回去了。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TrashManifest {
    /// 相对 vault 根,`/` 分隔。
    relative_path: String,
    name: String,
    deleted_at_ms: u64,
    size: u64,
    is_dir: bool,
}

/// 回收站目录的绝对路径。
pub fn trash_dir(vault: &Path) -> PathBuf {
    vault.join(TRASH_DIR)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_millis() as u64)
        .unwrap_or(0)
}

/// ID 只能是数字和连字符。前端拿到的 ID 会原样回传,不校验的话
/// `../../../etc/passwd` 就能当 ID 用。
fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        return Err("Invalid trash entry id".to_string());
    }
    Ok(())
}

fn manifest_path(vault: &Path, id: &str) -> PathBuf {
    trash_dir(vault).join(format!("{id}.meta.json"))
}

/// 载荷路径。目录用 `.dir`、文件用 `.bin`,这样光看名字就知道该 `remove_file`
/// 还是 `remove_dir_all`,不必依赖磁盘上的实际类型(它可能已经被外部动过)。
fn payload_path(vault: &Path, id: &str, is_dir: bool) -> PathBuf {
    let ext = if is_dir { "dir" } else { "bin" };
    trash_dir(vault).join(format!("{id}.{ext}"))
}

/// 同毫秒序号。列表排序时用作时间戳的次级键 —— 只按时间戳排的话同毫秒的几条
/// 顺序由 `read_dir` 决定,每次刷新都可能不一样。
fn id_sequence(id: &str) -> u32 {
    id.split_once('-')
        .and_then(|(_, suffix)| suffix.parse().ok())
        .unwrap_or(0)
}

fn read_manifest(vault: &Path, id: &str) -> Result<TrashManifest, String> {
    let path = manifest_path(vault, id);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read trash manifest {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("Trash manifest {} is corrupt: {e}", path.display()))
}

/// 把 vault 内的绝对路径转成相对路径(`/` 分隔)。
///
/// 拒掉含 `..` / 前缀盘符之类非 `Normal` 组件的路径:调用方给的都是
/// `resolve_in_vaults` 出来的 canonical 路径,出现别的组件说明有人绕过了那道闸门。
fn relative_within(vault: &Path, target: &Path) -> Result<String, String> {
    let relative = target
        .strip_prefix(vault)
        .map_err(|_| format!("{} is outside the vault", target.display()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            _ => return Err(format!("Unsupported notebook path {}", target.display())),
        }
    }
    if parts.is_empty() {
        return Err("Cannot trash the vault root".to_string());
    }
    Ok(parts.join("/"))
}

/// 把清单里的相对路径还原成 vault 内的绝对路径。
///
/// 这是回收站唯一一处"外部数据决定写入位置"的地方 —— 清单是磁盘上的 JSON,
/// 用户手改过、或者别的工具写过都有可能。所以逐段校验,而不是 `vault.join(s)`:
/// `join` 遇到绝对路径会把 vault 整个丢掉,遇到 `..` 又能一路爬出去。
fn resolve_relative(vault: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut dest = vault.to_path_buf();
    let mut segments = 0;
    for segment in relative.split('/') {
        let mut components = Path::new(segment).components();
        let Some(std::path::Component::Normal(part)) = components.next() else {
            return Err("Trash manifest has an unsafe original path".to_string());
        };
        if components.next().is_some() {
            return Err("Trash manifest has an unsafe original path".to_string());
        }
        dest.push(part);
        segments += 1;
    }
    if segments == 0 {
        return Err("Trash manifest has an empty original path".to_string());
    }
    // 恢复目标不能落进 vault 私有目录:那里放的是历史 / 索引 / 回收站自己,
    // 往里塞一个用户文件轻则被树扫描忽略,重则把回收站自己的清单覆盖掉。
    if dest.starts_with(vault.join(VAULT_PRIVATE_DIR)) {
        return Err("Cannot restore into the vault private directory".to_string());
    }
    Ok(dest)
}

/// 目录的总字节数。跳过 symlink(不跟随,免得指向父目录的链接把统计拖进循环),
/// 触到深度上限就停 —— 这个数只用来显示。
fn dir_size(dir: &Path, depth: usize) -> u64 {
    if depth >= MAX_SIZE_DEPTH {
        return 0;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        total += if meta.is_dir() {
            dir_size(&path, depth + 1)
        } else {
            meta.len()
        };
    }
    total
}

/// 抢一个没被占用的 ID 并把清单写下去。
///
/// 抢名字的原子性完全靠 `create_new`:两个进程 / 线程同毫秒删同名笔记时,只有
/// 一个能建出 `<ts>.meta.json`,另一个拿到 `AlreadyExists` 后往下试 `<ts>-1`。
/// 换成"先 exists 再写"就会两边都看到空位,后写的覆盖先写的 —— 那条笔记没了。
///
/// 清单在载荷落地**之前**写完:反过来的话崩在中间会留下一个没有清单的载荷,
/// 那是一份用户永远看不到、也不知道怎么删的数据。反之(有清单没载荷)是可见的
/// 半条,[`list`] 会把它跳过。
fn claim_entry(vault: &Path, manifest: &TrashManifest) -> Result<String, String> {
    let dir = trash_dir(vault);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let text = serde_json::to_string(manifest).map_err(|e| e.to_string())?;

    for suffix in 0..MAX_SAME_MS_ENTRIES {
        let id = if suffix == 0 {
            manifest.deleted_at_ms.to_string()
        } else {
            format!("{}-{suffix}", manifest.deleted_at_ms)
        };
        let path = manifest_path(vault, &id);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot write {}: {error}", path.display())),
        };
        // 清单是恢复的唯一依据,fsync 一次:没有它,断电后可能留下一个大小
        // 正确但内容是零的清单,而载荷已经改名了 —— 那条笔记就找不回来了。
        let written = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all());
        if let Err(error) = written {
            let _ = std::fs::remove_file(&path);
            return Err(format!("Cannot write {}: {error}", path.display()));
        }
        return Ok(id);
    }

    Err("Too many notebook items were deleted in the same millisecond".to_string())
}

/// 测试用:软删,但删除时间由调用方指定。
///
/// "同毫秒删两条同名笔记不互相覆盖"是这个模块唯一一条无法用真实时钟稳定复现的
/// 不变式 —— 两次 `trash()` 落在同一毫秒里靠的是机器够快,在慢机器上就静默变成
/// 一条什么都没验的测试。所以把时间戳做成参数,让那条守卫每次都真的被验到。
#[cfg(test)]
pub(super) fn trash_at(
    vault: &Path,
    target: &Path,
    deleted_at_ms: u64,
) -> Result<TrashItem, String> {
    trash_inner(vault, target, Some(deleted_at_ms))
}

/// 软删:把 `target` 搬进 vault 回收站。文件和目录都收。
///
/// `target` 必须是 `resolve_in_vaults` 出来的路径,`vault` 必须是它的
/// `owning_vault`。这里再验一次 `starts_with` —— 这个函数直接动用户的文件,
/// 不该假设调用方一定按顺序做过那两步。
pub fn trash(vault: &Path, target: &Path) -> Result<TrashItem, String> {
    trash_inner(vault, target, None)
}

fn trash_inner(vault: &Path, target: &Path, at_ms: Option<u64>) -> Result<TrashItem, String> {
    if !target.starts_with(vault) {
        return Err(format!("{} is outside the vault", target.display()));
    }
    // 回收站自己和历史都在私有目录里。允许软删它们就意味着"删除回收站"会把
    // 回收站搬进回收站,而恢复时又落回私有目录 —— 一个没有出口的循环。
    if target.starts_with(vault.join(VAULT_PRIVATE_DIR)) {
        return Err("Cannot trash the vault private directory".to_string());
    }
    let relative_path = relative_within(vault, target)?;
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "Invalid notebook file name".to_string())?;

    let meta = std::fs::symlink_metadata(target)
        .map_err(|e| format!("Cannot read {}: {e}", target.display()))?;
    let is_dir = meta.is_dir();
    let manifest = TrashManifest {
        relative_path,
        name,
        deleted_at_ms: at_ms.unwrap_or_else(now_ms),
        size: if is_dir {
            dir_size(target, 0)
        } else {
            meta.len()
        },
        is_dir,
    };

    let id = claim_entry(vault, &manifest)?;
    let payload = payload_path(vault, &id, is_dir);
    if let Err(error) = fs_ops::with_fs_retry(|| std::fs::rename(target, &payload)) {
        // 搬不动就把刚占的 ID 让出来,否则回收站里会攒一堆指向不存在载荷的清单。
        let _ = std::fs::remove_file(manifest_path(vault, &id));
        return Err(format!(
            "Cannot move {} to trash: {error}",
            target.display()
        ));
    }

    Ok(TrashItem {
        id,
        name: manifest.name,
        relative_path: manifest.relative_path,
        deleted_at_ms: manifest.deleted_at_ms,
        size: manifest.size,
        is_dir: manifest.is_dir,
    })
}

/// 列出回收站,新删的在前。
///
/// 坏条目(清单读不出、解析不了、载荷不在了)跳过而不是整个失败:回收站是恢复
/// 数据的地方,一条烂清单不该让另外二十条也捞不回来。
pub fn list(vault: &Path) -> Result<Vec<TrashItem>, String> {
    let dir = trash_dir(vault);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let read =
        std::fs::read_dir(&dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;
    let mut items = Vec::new();
    for entry in read.flatten() {
        let Some(id) = manifest_id(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let Ok(manifest) = read_manifest(vault, &id) else {
            continue;
        };
        // 载荷不在说明清单写完之后 rename 没成 —— 文件还在原位,列出来会让
        // 用户以为它被删了,点恢复又什么都没发生。
        if !payload_path(vault, &id, manifest.is_dir).exists() {
            continue;
        }
        items.push(TrashItem {
            id,
            name: manifest.name,
            relative_path: manifest.relative_path,
            deleted_at_ms: manifest.deleted_at_ms,
            size: manifest.size,
            is_dir: manifest.is_dir,
        });
    }
    items.sort_by(|left, right| {
        right
            .deleted_at_ms
            .cmp(&left.deleted_at_ms)
            .then_with(|| id_sequence(&right.id).cmp(&id_sequence(&left.id)))
    });
    Ok(items)
}

/// 从清单文件名里取 ID。不是清单就返回 None。
fn manifest_id(file_name: &str) -> Option<String> {
    let id = file_name.strip_suffix(".meta.json")?;
    validate_id(id).ok()?;
    Some(id.to_string())
}

/// 恢复的结果。返回绝对路径,前端据此重新加载那条笔记 / 刷新那棵子树。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredItem {
    pub path: String,
    pub is_dir: bool,
}

/// 恢复一条。原路径已被占用时报 `ALREADY_EXISTS:` —— 和新建 / 改名一个前缀,
/// 前端可以复用同一套"要不要换个名字"的处理。
///
/// 目录会整棵搬回去(rename 一次,不是逐个文件复制),所以"目录软删可完整恢复"
/// 这件事不依赖遍历的正确性。
pub fn restore(vault: &Path, id: &str) -> Result<RestoredItem, String> {
    validate_id(id)?;
    let manifest = read_manifest(vault, id)?;
    let payload = payload_path(vault, id, manifest.is_dir);
    if !payload.exists() {
        return Err("This trash item is no longer on disk".to_string());
    }
    let dest = resolve_relative(vault, &manifest.relative_path)?;
    if dest.exists() {
        return Err(format!("ALREADY_EXISTS:{}", dest.display()));
    }
    // 父目录可能也被删过(先删文件夹再单独恢复里面某条笔记)。补出来而不是报错
    // —— 它必然在 vault 内(`resolve_relative` 已经验过),建它是安全的。
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    }
    fs_ops::with_fs_retry(|| std::fs::rename(&payload, &dest))
        .map_err(|e| format!("Cannot restore {}: {e}", dest.display()))?;
    // 清单在载荷之后删:反过来的话崩在中间就留下一个没有清单的载荷 —— 用户
    // 看不到也删不掉。这个顺序下最坏是留一条清单,`list` 会跳过它。
    let _ = std::fs::remove_file(manifest_path(vault, id));
    Ok(RestoredItem {
        path: dest.to_string_lossy().into_owned(),
        is_dir: manifest.is_dir,
    })
}

/// 彻底删除一条:载荷交给**系统回收站**,清单和历史快照直接删掉。
///
/// 为什么载荷还要再进一层回收站:这是随手记里唯一一个"用户以为东西没了"的操作,
/// 而 vault 回收站里的笔记往往已经躺了很久 —— 点错的人不会记得内容。系统回收站
/// 是最后一道网,而且 Finder / 资源管理器里就能捞,不需要我们再做一套 UI。
///
/// 历史快照一起清:它按相对路径归档,不跟着文件走。留着的话用户以为内容已经没了
/// (实际还在 `.notebook/history/`),而同路径的新笔记一出生就继承上一条的历史。
pub fn purge(vault: &Path, id: &str) -> Result<(), String> {
    validate_id(id)?;
    let manifest = read_manifest(vault, id)?;
    let payload = payload_path(vault, id, manifest.is_dir);
    if payload.exists() {
        discard_history_for(vault, &manifest, &payload);
        remove_payload(&payload, &manifest.name)?;
    }
    let meta = manifest_path(vault, id);
    std::fs::remove_file(&meta).map_err(|e| format!("Cannot delete {}: {e}", meta.display()))
}

/// 清空回收站。返回真正清掉的载荷条数(给"已清空 N 项"用)。
///
/// 不是 `remove_dir_all(trash)` —— 那样载荷就直接消失了,而彻底删除的语义是
/// "退出 vault,但还能从系统回收站捞回来"。所以逐条走 [`purge`]。
///
/// 单条失败不中断:回收站里可能混着权限异常的条目,让它挡住其余几十条的清理
/// 只会逼用户去 Finder 里手动删。全部失败信息汇总后一起报。
pub fn purge_all(vault: &Path) -> Result<u32, String> {
    let dir = trash_dir(vault);
    if !dir.exists() {
        return Ok(0);
    }
    // 返回的条数按**用户在列表里看见的**算,不按清单文件数:清单还在而载荷已经
    // 被外部删掉的半条不会出现在列表里,把它也计进去会让提示说出一个比用户刚才
    // 看到的更大的数字。
    let visible = list(vault)?.len() as u32;

    let mut failures = Vec::new();
    for id in manifest_ids(&dir)? {
        if let Err(error) = purge(vault, &id) {
            failures.push(error);
        }
    }

    // 清单清完再扫一遍收孤儿载荷。**必须是第二遍**:第一遍时每条清单都还配着
    // 自己的载荷,分不清哪个是孤儿 —— 照名字判会把正常载荷也算进去,于是
    // `purge` 删过一次之后这里再删一次,清空整个失败。
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| format!("Cannot read {}: {e}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if manifest_id(&name).is_some() {
            // 清单还在说明它那条 purge 失败了(失败已经记在 failures 里)。
            // 连带把载荷删掉会让这条彻底恢复不了,所以整对留着。
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            if manifest_path(vault, stem).exists() {
                continue;
            }
        }
        // 没有清单的载荷不知道原名,按现名(`<id>.bin`)送走。
        if let Err(error) = remove_payload(&path, "") {
            failures.push(error);
        }
    }

    if failures.is_empty() {
        Ok(visible)
    } else {
        Err(failures.join("; "))
    }
}

/// 回收站里所有清单的 ID。
fn manifest_ids(dir: &Path) -> Result<Vec<String>, String> {
    Ok(std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {e}", dir.display()))?
        .flatten()
        .filter_map(|entry| manifest_id(&entry.file_name().to_string_lossy()))
        .collect())
}

/// 彻底删除时要不要再经过系统回收站。测试里恒为 false。
///
/// 不是"测试走一条不同的逻辑" —— 落点之外的每一步(改回原名、清单、历史、错误
/// 汇总)都一样。关掉它只是因为让 `cargo test` 往开发者的 `~/.Trash` 里攒一批
/// `purged.md` 是不能接受的副作用,而"彻底删除后文件不在 vault 里了"这个断言
/// 并不需要它。关掉后走的正是下面那条真实存在的退路(headless Linux 上就是它)。
const USE_SYSTEM_TRASH: bool = !cfg!(test);

/// 把载荷交给系统回收站。
///
/// 先尽量把它改回原名:系统回收站里显示的是文件名,一堆 `1712345678901.bin`
/// 等于捞不回来。改不动就按现名送走 —— 名字不好看比"删不掉"好。
///
/// 系统回收站不可用(headless Linux、没有 `$HOME`、跨设备)时退回硬删。用户点的
/// 是"彻底删除",这时候拒绝删除才是更糟的结果;但要走 stderr,免得"文件明明彻底
/// 删了却不在系统回收站里"这件事查不出原因。
fn remove_payload(payload: &Path, original_name: &str) -> Result<(), String> {
    let mut target = payload.to_path_buf();
    if !original_name.is_empty() {
        let renamed = payload.with_file_name(original_name);
        if !renamed.exists() && std::fs::rename(payload, &renamed).is_ok() {
            target = renamed;
        }
    }

    if USE_SYSTEM_TRASH {
        // `trash` 是外部 crate;这个模块自己也叫 trash,不写 `::` 会解析到自己身上。
        match ::trash::delete(&target) {
            Ok(()) => return Ok(()),
            Err(error) => eprintln!(
                "notebook: cannot move {} to the system trash ({error}); deleting it outright",
                target.display()
            ),
        }
    }

    let hard = if target.is_dir() {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_file(&target)
    };
    hard.map_err(|e| format!("Cannot delete {}: {e}", target.display()))
}

/// 清掉这一条对应的历史快照。
///
/// 目录要逐个笔记清:快照按**笔记的**相对路径归档,目录自己没有历史。所以把载荷
/// 里每个文件的路径拼回"它原来在 vault 里的相对路径",一条条清。
///
/// 失败不阻断彻底删除 —— 主要目标是把文件送走,历史清不掉只是残留。
fn discard_history_for(vault: &Path, manifest: &TrashManifest, payload: &Path) {
    let mut relatives = Vec::new();
    if manifest.is_dir {
        collect_payload_files(payload, &manifest.relative_path, 0, &mut relatives);
    } else {
        relatives.push(manifest.relative_path.clone());
    }
    for relative in relatives {
        // `discard` 收的是"这条笔记曾经在的绝对路径"。文件已经不在那了,而算
        // 快照目录名只用到相对路径,不读盘。
        let Ok(file) = resolve_relative(vault, &relative) else {
            continue;
        };
        if let Err(error) = snapshots::discard(vault, &file) {
            eprintln!("notebook: cannot discard history for {relative}: {error}");
        }
    }
}

/// 收集载荷目录里所有文件的**原相对路径**(相对 vault 根)。
fn collect_payload_files(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
    if depth >= MAX_SIZE_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}/{name}");
        if meta.is_dir() {
            collect_payload_files(&path, &relative, depth + 1, out);
        } else {
            out.push(relative);
        }
    }
}
