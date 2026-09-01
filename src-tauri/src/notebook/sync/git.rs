//! Git 同步:把 vault 当一个 git 工作区,一轮 = `commit → fetch → pull → push`。
//!
//! ## 为什么这一层不是 [`RemoteFs`](super::engine::RemoteFs)
//!
//! 看着很像:两边都是「本地一份、远端一份、要对齐」。但接成 `RemoteFs` 是错的,三条硬
//! 理由:
//!
//! - **vault 就是工作区。** `RemoteFs::put` 会写进本地扫描正在读的同一个目录 —— 本地与
//!   远端互为别名,而三方 diff 的前提是两侧是独立的存储。
//! - **git 自己就是一个三方合并引擎**,有自己的 merge base。在它上面再套一层我们的基线,
//!   等于两套互不知情的对账逻辑,谁说了算取决于执行顺序。
//! - **冲突标记会被当成正文。** `git pull` 冲突时把 `<<<<<<<` 写进工作区文件;我们的扫描器
//!   会把它 hash 成合法内容,然后传到别的远端去。
//!
//! 所以 git 是一个**并列的同步模式**,不是一个后端。Markio 的做法可以印证:它的 git 同步
//! 是独立命令面(`commands/git.rs`),完全不经过 `diff.ts`。
//!
//! ## 这一层最危险的一件事
//!
//! **冲突未解时绝不能 `add -A && commit`。** 那会把 `<<<<<<<` 标记当成用户已经解决的正文
//! 提交上去,而且是**沿着两条历史都合法**的一次提交 —— 别的设备拉下来看不出异常,只会
//! 看到一篇正文里带着标记的笔记。之后再想还原,得手工翻历史。
//!
//! 所以一轮的第一件事是查未合并路径,有就立刻停,什么都不做。
//!
//! ## pull 用 merge 不用 rebase
//!
//! rebase 冲突会把仓库留在 rebase 中途,那对不懂 git 的用户是个很难走出来的状态(要
//! `--continue` / `--skip` / `--abort` 三选一,选错会丢提交)。merge 冲突只是一个状态,
//! 解了 commit、不想解就 `merge --abort`,两条路都直白。历史上多几个 merge 提交,对一个
//! 笔记库来说不值得为它换掉可恢复性。

use std::path::Path;

use crate::git::{git_dir, run_git_for, run_git_network_for};

/// 一轮 git 同步的结果。字段直接给前端显示用。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncReport {
    /// 这一轮做成了什么。
    pub stage: GitSyncStage,
    /// 本地新提交的文件数(0 表示没有本地改动)。
    pub committed: usize,
    /// 从远端合进来的提交数。
    pub pulled: usize,
    /// 推上去的提交数。
    pub pushed: usize,
    /// 未解决的冲突文件(vault 相对路径)。非空时 `stage` 必为 `Conflicted`。
    pub conflicts: Vec<String>,
    /// 给用户看的一句话。git 的原始输出留在这里而不是丢掉 —— 排障时它是唯一线索。
    pub detail: String,
}

/// 一轮走到哪一步为止。
///
/// 不用 `bool success`:「没有远端所以只提交了本地」和「推送成功」都不是失败,但也不是
/// 同一件事,而用户需要能分清。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitSyncStage {
    /// 一轮走完,本地与远端一致。
    Synced,
    /// 提交了本地改动,但没有配置远端 —— 「有版本管理但不同步」是正当配置。
    CommittedLocally,
    /// 有未解决的冲突。**没有做任何推送**,等用户处理。
    Conflicted,
    /// 两侧都没有变化,什么都没做。
    UpToDate,
}

/// 走一轮。
///
/// `vault` 必须已经是 git 工作区 —— init / clone 属于绑定阶段,不在同步一轮里做。理由是
/// 那两个动作各有自己的失败模式和用户确认(clone 会写一整个目录),混进定时任务里等于
/// 让一次网络抖动触发一次仓库创建。
pub fn run(vault: &Path, message: &str) -> Result<GitSyncReport, String> {
    let repo = vault
        .to_str()
        .ok_or_else(|| "Vault path is not valid UTF-8".to_string())?;
    git_dir(repo).map_err(|_| format!("{repo} is not a git repository"))?;

    // 第一件事:冲突未解就立刻停。见模块文档 —— 在冲突态上 `add -A` 会把标记提交上去。
    let conflicts = unmerged_paths(repo)?;
    if !conflicts.is_empty() {
        return Ok(GitSyncReport {
            stage: GitSyncStage::Conflicted,
            committed: 0,
            pulled: 0,
            pushed: 0,
            conflicts,
            detail: "Resolve the existing conflicts before syncing again".to_string(),
        });
    }

    // 上一轮留下的合并有没有收尾。**必须在这里问**:冲突已经解完(没有未合并路径)但
    // 合并还没提交时,`MERGE_HEAD` 仍在,而那种状态下 `git pull` 会直接报「尚未结束您的
    // 合并」—— 每一轮都报,同步彻底卡死。
    let merging = merge_in_progress(repo)?;
    let committed = commit_local_changes(repo, message, merging)?;
    let Some(remote) = default_remote(repo)? else {
        return Ok(GitSyncReport {
            stage: if committed > 0 {
                GitSyncStage::CommittedLocally
            } else {
                GitSyncStage::UpToDate
            },
            committed,
            pulled: 0,
            pushed: 0,
            conflicts: Vec::new(),
            detail: "No git remote configured".to_string(),
        });
    };

    let mut detail = String::new();
    // fetch 不动工作区、不产生冲突,所以可以无条件跑。它让下面的 ahead/behind 有意义。
    let fetched = run_git_network_for(repo, &["fetch", "--", remote.as_str()])?;
    if !fetched.status.success() {
        return Err(combined_output(&fetched));
    }

    let upstream = match upstream_ref(repo)? {
        Some(upstream) => upstream,
        // 本地分支还没有上游(第一次推送)。直接 push -u,没什么可 pull 的。
        None => {
            // 但仓库可能一个提交都还没有(刚 clone 一个空裸库,而本地也没东西要提交)。
            // 那时没有任何东西可推,`push` 会失败在「src refspec 不匹配」上 —— 而那不是
            // 故障,是「无事可做」。
            if !has_head(repo)? {
                return Ok(GitSyncReport {
                    stage: GitSyncStage::UpToDate,
                    committed,
                    pulled: 0,
                    pushed: 0,
                    conflicts: Vec::new(),
                    detail: "Nothing to sync yet".to_string(),
                });
            }
            let branch = current_branch(repo)?;
            let pushed = run_git_network_for(
                repo,
                &["push", "--set-upstream", remote.as_str(), branch.as_str()],
            )?;
            if !pushed.status.success() {
                return Err(combined_output(&pushed));
            }
            return Ok(GitSyncReport {
                stage: GitSyncStage::Synced,
                committed,
                pulled: 0,
                pushed: count_commits(repo, "HEAD", None)?,
                conflicts: Vec::new(),
                detail: combined_output(&pushed),
            });
        }
    };

    let behind = count_commits(repo, "HEAD", Some(&upstream))?;
    let ahead = count_commits(repo, &upstream, Some("HEAD"))?;

    let mut pulled = 0;
    if behind > 0 {
        // `--no-rebase` 显式给出:用户的 `pull.rebase = true` 会把这里变成 rebase,而 rebase
        // 冲突留下的中途状态是我们刻意避开的(见模块文档)。
        let merged = run_git_network_for(repo, &["pull", "--no-rebase", "--", remote.as_str()])?;
        detail = combined_output(&merged);
        let conflicts = unmerged_paths(repo)?;
        if !conflicts.is_empty() {
            // 冲突就停在这里,**不 push**。带着未解决的合并推上去会把冲突扩散给别的设备。
            return Ok(GitSyncReport {
                stage: GitSyncStage::Conflicted,
                committed,
                pulled: 0,
                pushed: 0,
                conflicts,
                detail,
            });
        }
        if !merged.status.success() {
            return Err(detail);
        }
        pulled = behind;
    }

    // 合并之后 ahead 可能变了(merge 提交本身也是一个),重算而不是用上面那个。
    let mut pushed = 0;
    let ahead_now = count_commits(repo, &upstream, Some("HEAD"))?;
    if ahead_now > 0 {
        let output = run_git_network_for(repo, &["push", "--", remote.as_str()])?;
        if !output.status.success() {
            return Err(combined_output(&output));
        }
        pushed = ahead_now;
        if detail.is_empty() {
            detail = combined_output(&output);
        }
    }

    let stage = if committed == 0 && pulled == 0 && pushed == 0 && ahead == 0 {
        GitSyncStage::UpToDate
    } else {
        GitSyncStage::Synced
    };
    Ok(GitSyncReport {
        stage,
        committed,
        pulled,
        pushed,
        conflicts: Vec::new(),
        detail: detail.trim().to_string(),
    })
}

/// 未合并的路径。空表示没有冲突。
///
/// 用 `diff --diff-filter=U` 而不是解析 `status` 的两字母码:后者要区分 `DD/AU/UD/UA/DU/AA/UU`
/// 七种组合,漏一种就等于漏一次冲突,而漏冲突的后果是把标记提交上去。
fn unmerged_paths(repo: &str) -> Result<Vec<String>, String> {
    let output = run_git_for(repo, &["diff", "--name-only", "--diff-filter=U", "-z"])?;
    if !output.status.success() {
        return Err(combined_output(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// 有没有一次没收尾的合并。
///
/// `MERGE_HEAD` 在 `git merge` 产生冲突时出现,提交之后消失。它存在而又没有未合并路径,
/// 说明用户已经解完冲突但那次合并还没提交。
fn merge_in_progress(repo: &str) -> Result<bool, String> {
    let output = run_git_for(repo, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])?;
    Ok(output.status.success())
}

/// 暂存全部改动并提交。返回提交进去的文件数,0 表示没有本地改动。
///
/// `git commit` 在无可提交时返回非零并打印「nothing to commit」。那**不是错误** ——
/// 定时同步里绝大多数轮次都是这种情况,当成错误会让状态栏一直亮红。所以先问有没有东西
/// 要提交,再决定调不调 commit。
///
/// ## `force` 是给合并收尾用的,它不是一个优化
///
/// 用户可能把冲突解成**和自己 HEAD 完全一样**的内容(等价于「保留本地」)。那时索引与
/// HEAD 没有差异,`diff --cached` 是空的,按上面的规则就不提交 —— 于是 `MERGE_HEAD` 一直
/// 留着,而那种状态下 `git pull` 每一轮都报「尚未结束您的合并」,同步彻底卡死,而且用户
/// 从界面上完全看不出为什么。
///
/// 所以合并进行中时无条件提交:那一次提交的意义不是「有几个文件变了」,而是「这次合并
/// 结束了」。
fn commit_local_changes(repo: &str, message: &str, force: bool) -> Result<usize, String> {
    // `-A` 而不是 `.`:两者在 git 2.x 里都会暂存删除(实测确认,`add . ` 忽略删除是 1.x
    // 的行为),差别在作用域 —— `.` 只覆盖 cwd,`-A` 覆盖整个工作区。这里 cwd 恰好就是
    // 仓库根,所以当前是等价的;用 `-A` 是为了不把正确性挂在「cwd 恰好是根」上。
    //
    // `.notebook/` 由它自己的 `.gitignore`(内容是 `*`)排除,所以本地历史、回收站、
    // 索引不会入库。
    let staged = run_git_for(repo, &["add", "-A"])?;
    if !staged.status.success() {
        return Err(combined_output(&staged));
    }

    let names = run_git_for(repo, &["diff", "--cached", "--name-only", "-z"])?;
    if !names.status.success() {
        return Err(combined_output(&names));
    }
    let count = String::from_utf8_lossy(&names.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .count();
    if count == 0 && !force {
        return Ok(0);
    }

    let committed = run_git_for(repo, &["commit", "-m", message])?;
    if !committed.status.success() {
        return Err(combined_output(&committed));
    }
    Ok(count)
}

/// 默认远端名。没有配置远端时返回 `None`。
///
/// 取第一个而不是硬编码 `origin`:clone 出来的确实叫 origin,但用户手工 `remote add` 时
/// 可能起了别的名字,硬编码会让那种仓库「看起来没有远端」。
fn default_remote(repo: &str) -> Result<Option<String>, String> {
    let output = run_git_for(repo, &["remote"])?;
    if !output.status.success() {
        return Err(combined_output(&output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut names: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    // origin 优先,其次字典序 —— 顺序要稳定,否则同一个仓库在两轮里可能推到不同远端。
    names.sort_unstable();
    let chosen = names
        .iter()
        .find(|name| **name == "origin")
        .copied()
        .unwrap_or(names[0]);
    Ok(Some(chosen.to_string()))
}

/// 当前分支的上游 ref。没有上游时返回 `None`(第一次推送的情况)。
fn upstream_ref(repo: &str) -> Result<Option<String>, String> {
    let output = run_git_for(
        repo,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )?;
    if !output.status.success() {
        // 没有上游时 git 就是返回非零。这不是故障。
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if name.is_empty() { None } else { Some(name) })
}

/// 当前分支名。
///
/// 用 `branch --show-current` 而不是 `rev-parse --abbrev-ref HEAD`:后者在**还没有任何
/// 提交**的分支上会失败(打印 `HEAD` 到 stdout 再报「有歧义的参数」),而那正是刚绑定一个
/// 空远端时的状态。前者对未出生的分支照样给出名字。
///
/// 返回空表示处于 detached HEAD。那时不该推 —— 推一个游离的头会在远端造出一条没有分支
/// 指向的提交,而用户在自己这边看不出发生了什么。
fn current_branch(repo: &str) -> Result<String, String> {
    let output = run_git_for(repo, &["branch", "--show-current"])?;
    if !output.status.success() {
        return Err(combined_output(&output));
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return Err("Repository is in a detached HEAD state".to_string());
    }
    Ok(name)
}

/// `from..to` 之间有几个提交。`to` 为 `None` 表示数 `from` 可达的全部提交。
///
/// ## `--` 的位置
///
/// 放在**范围之后**。`rev-list --count -- HEAD` 里的 `HEAD` 会被当成路径(git 报「有歧义
/// 的参数」),而 `rev-list --count HEAD --` 才是「数这个范围,没有路径过滤」。这个 `--`
/// 不是可选的装饰:少了它,一个恰好和分支同名的文件会让 git 猜,而它猜哪边取决于工作区
/// 内容。
///
/// ## 失败不吞成 0
///
/// 在调用方那里 0 的含义是「没什么要推/要拉」。把错误吞成 0 会让一次坏掉的调用变成
/// 「同步成功且无事可做」—— 用户看到绿灯,而实际上一个字节都没传。所以只有一种情况算
/// 0:仓库还没有任何提交(空仓库),那时 `HEAD` 解析不出来,而「没有提交」和「差 0 个
/// 提交」确实是同一件事。
fn count_commits(repo: &str, from: &str, to: Option<&str>) -> Result<usize, String> {
    validate_rev(from)?;
    if let Some(to) = to {
        validate_rev(to)?;
    }
    let range = match to {
        Some(to) => format!("{from}..{to}"),
        None => from.to_string(),
    };
    let output = run_git_for(repo, &["rev-list", "--count", &range, "--"])?;
    if !output.status.success() {
        if !has_head(repo)? {
            return Ok(0);
        }
        return Err(combined_output(&output));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| {
            format!(
                "Cannot parse commit count for {range}: {}",
                combined_output(&output)
            )
        })
}

/// 仓库里有没有任何提交。
fn has_head(repo: &str) -> Result<bool, String> {
    let output = run_git_for(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    Ok(output.status.success())
}

/// 版本名不能以 `-` 开头。
///
/// 这里的名字有一个是**远端给的**:`upstream_ref` 的结果来自 `rev-parse @{u}`,而分支名
/// 由远端仓库决定。`refs/heads/--output=/path` 是可以用 `update-ref` 造出来的
/// (见 `git::validate_git_revision` 的文档),于是它会以 `origin/--output=…` 的形式流到
/// 这里。虽然带了 `origin/` 前缀不以 `-` 开头,但 `@{u}` 的输出形态不由我们决定,而这道
/// 检查的成本是一次字符比较。
fn validate_rev(rev: &str) -> Result<(), String> {
    if rev.is_empty() {
        return Err("Git revision must not be empty".to_string());
    }
    if rev.starts_with('-') {
        return Err(format!("Invalid git revision: {rev}"));
    }
    Ok(())
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    /// 一个「裸库当远端 + 若干克隆当设备」的台子。
    ///
    /// 用真实 git 而不是假实现:这一层的全部职责就是**按正确次序驱动真实 git**,而假实现
    /// 只能验证我对 git 行为的想象。冲突态、nothing-to-commit 的退出码、`@{u}` 缺失时的
    /// 返回值,这几件事恰好都是想象最容易出错的地方。
    struct Fixture {
        root: PathBuf,
        remote: PathBuf,
    }

    impl Fixture {
        /// `seeded = true` 时远端带一个初始提交,对应真实的「先建远端再 clone」流程,
        /// 各设备因此共享历史。`false` 留给专门测「无关历史」的用例。
        fn new(tag: &str) -> Self {
            let fx = Self::bare(tag);
            let seed = fx.device("seed");
            seed.write("README.md", "notebook");
            seed.git(&["add", "-A"]);
            seed.git(&["commit", "-m", "seed"]);
            seed.git(&["push", "--set-upstream", "origin", "main"]);
            let _ = fs::remove_dir_all(&seed.path);
            fx
        }

        fn bare(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("aeroric-nbgit-{tag}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).expect("mkdir root");
            let remote = root.join("remote.git");
            let out = std::process::Command::new("git")
                .args(["init", "--bare", "--initial-branch=main"])
                .arg(&remote)
                .output()
                .expect("git init --bare");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            Self { root, remote }
        }

        /// 建一个绑到裸库的克隆,配好身份。
        fn device(&self, name: &str) -> Device {
            let path = self.root.join(name);
            let out = std::process::Command::new("git")
                .arg("clone")
                .arg(&self.remote)
                .arg(&path)
                .output()
                .expect("git clone");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let device = Device { path };
            device.git(&["config", "user.email", "dev@example.test"]);
            device.git(&["config", "user.name", "Device"]);
            // 空裸库克隆出来没有 HEAD 分支,显式建一个,两台设备才在同一条分支上。
            device.git(&["checkout", "-B", "main"]);
            device
        }

        /// 一个不带远端的普通仓库。
        fn standalone(&self, name: &str) -> Device {
            let path = self.root.join(name);
            fs::create_dir_all(&path).expect("mkdir");
            let device = Device { path };
            device.git(&["init", "--initial-branch=main"]);
            device.git(&["config", "user.email", "dev@example.test"]);
            device.git(&["config", "user.name", "Device"]);
            device
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct Device {
        path: PathBuf,
    }

    impl Device {
        fn as_str(&self) -> String {
            self.path.to_string_lossy().to_string()
        }

        fn git(&self, args: &[&str]) -> String {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&self.path)
                .output()
                .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
            assert!(
                out.status.success(),
                "git {args:?} failed: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        fn write(&self, rel: &str, body: &str) {
            let target = self.path.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("mkdir parent");
            }
            fs::write(target, body).expect("write");
        }

        fn read(&self, rel: &str) -> String {
            fs::read_to_string(self.path.join(rel)).expect("read")
        }

        fn sync(&self) -> GitSyncReport {
            run(&self.path, "notebook: test round").expect("sync round")
        }
    }

    #[test]
    fn a_clean_repository_with_nothing_to_do_reports_up_to_date() {
        let fx = Fixture::new("noop");
        let a = fx.device("a");

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::UpToDate);
        assert_eq!(report.committed, 0);
        assert_eq!(report.pushed, 0);
        assert!(report.conflicts.is_empty());
    }

    #[test]
    fn a_local_note_is_committed_and_pushed() {
        let fx = Fixture::new("push");
        let a = fx.device("a");
        a.write("note.md", "hello");

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::Synced);
        assert_eq!(report.committed, 1);
        assert_eq!(report.pushed, 1);
        // 另一台设备拉得到 —— 证明真的推上去了,而不只是本地提交。
        let b = fx.device("b");
        assert_eq!(b.read("note.md"), "hello");
    }

    #[test]
    fn a_remote_note_is_pulled() {
        let fx = Fixture::new("pull");
        // 两台设备都要在 a 推送**之前**克隆好 —— 克隆晚了的话 b 一开始就有 a 那一条,
        // 于是 pulled 恒为 0,测试变成空的。
        let a = fx.device("a");
        let b = fx.device("b");

        a.write("from-a.md", "written by a");
        a.sync();

        b.write("from-b.md", "written by b");
        let report = b.sync();

        assert_eq!(report.stage, GitSyncStage::Synced);
        assert_eq!(report.committed, 1);
        assert!(report.pulled >= 1, "应该合进来 a 那一条:{report:?}");
        assert_eq!(b.read("from-a.md"), "written by a");
        // 而 b 自己那条也上去了:第三台设备看得到两边。
        let c = fx.device("c");
        assert_eq!(c.read("from-a.md"), "written by a");
        assert_eq!(c.read("from-b.md"), "written by b");
    }

    #[test]
    fn a_repository_without_a_remote_still_commits_locally() {
        // 「有版本管理但不同步」是正当配置,不该报错。
        let fx = Fixture::new("noremote");
        let a = fx.standalone("solo");
        a.write("note.md", "local only");

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::CommittedLocally);
        assert_eq!(report.committed, 1);
        assert_eq!(report.pushed, 0);
        assert!(report.detail.contains("No git remote"));
    }

    #[test]
    fn a_pull_conflict_stops_the_round_without_pushing() {
        // 这是这一层最重要的一条。带着未解决的合并推上去会把冲突扩散给所有设备。
        let fx = Fixture::new("conflict");
        let a = fx.device("a");
        let b = fx.device("b");

        a.write("note.md", "written by a");
        a.sync();
        b.write("note.md", "written by b");

        let report = b.sync();

        assert_eq!(report.stage, GitSyncStage::Conflicted);
        assert_eq!(report.conflicts, vec!["note.md".to_string()]);
        assert_eq!(report.pushed, 0, "冲突时一次推送都不该发生");

        // 远端还是 a 那一版 —— b 的冲突没有扩散出去。
        let c = fx.device("c");
        assert_eq!(c.read("note.md"), "written by a");
    }

    #[test]
    fn an_unresolved_conflict_is_never_committed_as_content() {
        // 最危险的失败模式:在冲突态上 `add -A && commit` 会把 `<<<<<<<` 标记当成用户
        // 已经解决的正文提交上去,而且沿着两条历史都合法 —— 别的设备拉下来看不出异常,
        // 只会看到一篇正文里带着标记的笔记。
        let fx = Fixture::new("markers");
        let a = fx.device("a");
        let b = fx.device("b");

        a.write("note.md", "written by a");
        a.sync();
        b.write("note.md", "written by b");
        b.sync(); // 这一轮产生冲突

        let head_before = b.git(&["rev-parse", "HEAD"]);
        assert!(
            b.read("note.md").contains("<<<<<<<"),
            "工作区应该有冲突标记"
        );

        // 再跑一轮 —— 这是定时同步一定会做的事。
        let report = b.sync();

        assert_eq!(report.stage, GitSyncStage::Conflicted);
        assert_eq!(report.committed, 0, "冲突态下不能有任何提交");
        assert_eq!(
            b.git(&["rev-parse", "HEAD"]),
            head_before,
            "HEAD 不该动 —— 动了就意味着标记被提交了"
        );
        // 而远端始终没被污染。
        let c = fx.device("c");
        assert!(!c.read("note.md").contains("<<<<<<<"));
    }

    #[test]
    fn a_resolved_conflict_completes_on_the_next_round() {
        // 解完冲突之后,下一轮要能把合并收尾并推上去 —— 否则用户解了也出不去。
        let fx = Fixture::new("resolved");
        let a = fx.device("a");
        let b = fx.device("b");

        a.write("note.md", "written by a");
        a.sync();
        b.write("note.md", "written by b");
        b.sync();
        assert_eq!(b.sync().stage, GitSyncStage::Conflicted);

        // 用户解决冲突。两步都要:改内容,再 `git add` 标记已解决 —— 这正是
        // `git_resolve_conflict` 做的事(它在 checkout --ours/--theirs 之后 add)。
        // 只改内容的话路径仍在未合并状态,同步会一直停在 Conflicted。
        b.write("note.md", "merged by hand");
        b.git(&["add", "note.md"]);

        let report = b.sync();

        assert_eq!(report.stage, GitSyncStage::Synced);
        assert!(report.conflicts.is_empty());
        let c = fx.device("c");
        assert_eq!(c.read("note.md"), "merged by hand");
    }

    #[test]
    fn resolving_a_conflict_in_favour_of_the_local_side_still_concludes_the_merge() {
        // 卡死场景:用户选「保留本地」,于是解决后的索引和自己的 HEAD 一模一样,
        // `diff --cached` 是空的。按「没东西就不提交」的规则,那次合并永远收不了尾,
        // `MERGE_HEAD` 留着,之后每一轮 `git pull` 都报「尚未结束您的合并」。
        let fx = Fixture::new("keepours");
        let a = fx.device("a");
        let b = fx.device("b");

        a.write("note.md", "written by a");
        a.sync();
        b.write("note.md", "written by b");
        b.sync();
        assert_eq!(b.sync().stage, GitSyncStage::Conflicted);

        // 等价于 UI 里点「保留本地」:git_resolve_conflict 的 Ours 分支。
        b.git(&["checkout", "--ours", "--", "note.md"]);
        b.git(&["add", "note.md"]);

        let report = b.sync();

        assert_eq!(report.stage, GitSyncStage::Synced, "{report:?}");
        assert!(
            !b.path.join(".git").join("MERGE_HEAD").exists(),
            "合并必须收尾 —— MERGE_HEAD 还在的话下一轮 pull 就会失败"
        );
        // 再跑一轮要干净落地,证明没有留下卡死状态。
        assert_eq!(b.sync().stage, GitSyncStage::UpToDate);
        assert_eq!(b.read("note.md"), "written by b");
    }

    #[test]
    fn unrelated_histories_are_refused_rather_than_spliced_together() {
        // 两台机器各有一个已存在的 vault,都 init 后指向同一个新建的空远端 —— 于是各自
        // 有一个独立的根提交。
        //
        // `--allow-unrelated-histories` 能合上,但**刻意不加**:用户填错远端 URL 时那个
        // 开关会把别人的整个笔记库悄悄拼进他的库里,而那比报错糟得多。想合并两个独立
        // vault 的用户可以自己明确地做一次。
        let fx = Fixture::bare("unrelated");
        let a = fx.device("a");
        let b = fx.device("b");

        a.write("from-a.md", "a");
        a.sync();
        b.write("from-b.md", "b");

        let error = run(&b.path, "notebook: test round").unwrap_err();

        assert!(
            error.contains("unrelated histories") || error.contains("无关的历史"),
            "error = {error}"
        );
        // b 自己的东西还在,没有被这次失败动过。
        assert_eq!(b.read("from-b.md"), "b");
    }

    #[test]
    fn the_private_directory_is_never_committed() {
        // `.notebook/` 里是本地历史、回收站、索引。它们跟着同步会把回收站在设备间复制,
        // 而删除本来就该是本地动作。靠 `.notebook/.gitignore`(内容是 `*`)排除。
        let fx = Fixture::new("private");
        let a = fx.device("a");
        a.write(".notebook/.gitignore", "*\n");
        a.write(".notebook/history/note.md", "old version");
        a.write(".notebook/trash/deleted.md", "deleted note");
        a.write("note.md", "real note");

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::Synced);
        let tracked = a.git(&["ls-files"]);
        assert!(tracked.contains("note.md"), "tracked = {tracked}");
        assert!(
            !tracked.contains(".notebook/"),
            "私有目录不该入库:tracked = {tracked}"
        );
    }

    #[test]
    fn a_deleted_note_is_propagated() {
        // 删除也要走 `-A`。只 `add .` 的话删除不会进暂存区,于是笔记在别的设备上永远
        // 不消失。
        let fx = Fixture::new("delete");
        let a = fx.device("a");
        let b = fx.device("b");
        a.write("note.md", "hello");
        a.sync();
        b.sync();
        assert_eq!(b.read("note.md"), "hello");

        fs::remove_file(a.path.join("note.md")).expect("rm");
        let report = a.sync();
        assert_eq!(report.stage, GitSyncStage::Synced);
        assert_eq!(report.committed, 1, "删除本身算一次改动");

        b.sync();
        assert!(!b.path.join("note.md").exists(), "删除应该传播到 b");
    }

    #[test]
    fn a_first_push_sets_the_upstream() {
        // 刚 init + remote add 的仓库没有上游。要能自己 push -u,否则用户得手工跑一次。
        let fx = Fixture::bare("firstpush");
        let a = fx.standalone("a");
        a.git(&[
            "remote",
            "add",
            "origin",
            fx.remote.to_string_lossy().as_ref(),
        ]);
        a.write("note.md", "first");

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::Synced);
        assert!(report.pushed >= 1);
        let upstream = a.git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]);
        assert_eq!(upstream, "origin/main");
    }

    #[test]
    fn a_bound_but_empty_remote_with_nothing_local_does_nothing() {
        // 刚绑定、两边都是空的。不能报错(会让状态栏亮红),也不能试着 push
        // (`src refspec` 不匹配)。
        let fx = Fixture::bare("emptyboth");
        let a = fx.standalone("a");
        a.git(&[
            "remote",
            "add",
            "origin",
            fx.remote.to_string_lossy().as_ref(),
        ]);

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::UpToDate);
        assert_eq!(report.pushed, 0);
    }

    #[test]
    fn a_detached_head_is_refused_rather_than_pushed() {
        // 用户翻旧版本时会停在 detached HEAD 上。推一个游离的头会在远端造出一条没有分支
        // 指向的提交 —— 用户在自己这边完全看不出发生了什么,而那条提交也不会被任何人拉到。
        let fx = Fixture::new("detached");
        let a = fx.device("a");
        a.write("note.md", "one");
        a.sync();
        a.write("note.md", "two");
        a.sync();

        // 回到上一个提交,并且断掉上游(detached 本身就没有 @{u})。
        a.git(&["checkout", "--detach", "HEAD~1"]);
        a.write("scratch.md", "written while detached");

        let error = run(&a.path, "notebook: test round").unwrap_err();

        assert!(error.contains("detached HEAD"), "error = {error}");
    }

    #[test]
    fn a_remote_that_is_not_called_origin_still_works() {
        // 用户手工 `remote add backup <url>` 是正当用法。硬编码 origin 会让那种仓库
        // 「看起来没有远端」,或者报一个「'origin' 不像是仓库」的费解错误。
        let fx = Fixture::bare("named");
        let a = fx.standalone("a");
        a.git(&[
            "remote",
            "add",
            "backup",
            fx.remote.to_string_lossy().as_ref(),
        ]);
        a.write("note.md", "via backup");

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::Synced, "{report:?}");
        assert!(report.pushed >= 1);
        let upstream = a.git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]);
        assert_eq!(upstream, "backup/main");
    }

    #[test]
    fn origin_wins_when_several_remotes_exist() {
        // 顺序必须稳定,否则同一个仓库在两轮里可能推到不同远端。
        let fx = Fixture::bare("multi");
        let a = fx.standalone("a");
        let url = fx.remote.to_string_lossy().to_string();
        a.git(&["remote", "add", "zzz", &url]);
        a.git(&["remote", "add", "origin", &url]);
        a.git(&["remote", "add", "aaa", &url]);

        assert_eq!(
            default_remote(&a.as_str()).expect("remotes"),
            Some("origin".to_string())
        );
    }

    #[test]
    fn a_broken_revision_range_is_an_error_not_a_silent_zero() {
        // 在调用方那里 0 的含义是「没什么要推」。把错误吞成 0 会让一次坏掉的调用变成
        // 「同步成功且无事可做」—— 用户看到绿灯,而实际上一个字节都没传。
        let fx = Fixture::new("countfail");
        let a = fx.device("a");
        a.write("note.md", "x");
        a.sync();

        let error = count_commits(&a.as_str(), "no-such-ref-anywhere", Some("HEAD")).unwrap_err();
        assert!(!error.is_empty());

        // 而空仓库仍然算 0:「没有提交」和「差 0 个提交」确实是同一件事。
        let empty = fx.standalone("empty");
        assert_eq!(
            count_commits(&empty.as_str(), "HEAD", None).expect("count"),
            0
        );
    }

    #[test]
    fn a_revision_starting_with_a_dash_is_refused() {
        // `@{u}` 的输出形态不由我们决定,而分支名由远端仓库决定 ——
        // `refs/heads/--output=/path` 是可以用 update-ref 造出来的。
        assert!(validate_rev("--output=/tmp/evil").is_err());
        assert!(validate_rev("-x").is_err());
        assert!(validate_rev("").is_err());
        assert!(validate_rev("origin/main").is_ok());
    }

    #[test]
    fn a_non_repository_is_refused() {
        // init / clone 属于绑定阶段,不在同步一轮里做 —— 那两个动作各有自己的失败模式和
        // 用户确认,混进定时任务里等于让一次网络抖动触发一次仓库创建。
        let fx = Fixture::bare("norepo");
        let plain = fx.root.join("not-a-repo");
        fs::create_dir_all(&plain).expect("mkdir");

        let error = run(&plain, "notebook: test round").unwrap_err();

        assert!(error.contains("not a git repository"), "error = {error}");
    }

    #[test]
    fn an_empty_round_on_a_standalone_repository_is_not_an_error() {
        // `git commit` 无可提交时返回非零。定时同步里绝大多数轮次都是这种情况,
        // 当成错误会让状态栏一直亮红。
        let fx = Fixture::new("emptycommit");
        let a = fx.standalone("solo");
        a.write("note.md", "x");
        a.sync();

        let report = a.sync();

        assert_eq!(report.stage, GitSyncStage::UpToDate);
        assert_eq!(report.committed, 0);
    }
}
