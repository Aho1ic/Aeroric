# Agent Note: 设置 › Git 与工作副本

Status: implemented

## Problem

设置方案（`proposed/feature/2026-11-24-settings-redesign.md`）里，两页的内容是：
- **Git：** 委派和审查开工作副本时的分支前缀、提交说明的附加指引；
- **工作副本：** 根目录、自动清理、保留数量，以及现有工作副本的列表。

第一版只放了“即将支持”。方案还定了一条：不放没作用的控件。

动手前核对了这几项各由什么代码管：
- **分支前缀：** 委派、审查、重跑开的都是独立副本（R2-3b，`isolated_copy::create_from`），用的是 `git worktree add --detach`，不建分支。唯一会建分支的是 `git.create_task_worktree`（分支名 `aeroric-task/<任务>`），App 从不调它。所以分支前缀管不到任何东西。
- **提交说明：** Git 面板的“生成”调 `git.generate_commit_message`，用项目配置里的 `git.commit_prompt`。没有全局可加的要求。
- **根目录：** 固定在 `<项目>/.aeroric/worktrees`。带回改动、合并、删除前的安全检查（`ensure_path_under_worktrees_root`、`is_under_worktrees_root`）都认这个位置，`.git/info/exclude` 也只排除这一处。
- **自动清理：** 已经有一条，`isolated_copy::prune`：宿主启动时，删掉清空满 30 天的副本，清空指里面的改动都已带回或放弃。还有改动的副本从不删。
- **任务定期清理：** `tasks::cleanup::expired_task_ids` 明确跳过没处理的工作副本。理由写在那里：删除要 `--force` 加删分支，会丢掉没提交的工作，“用户按删除是用户的选择，定时器不行”。

## Decision

**设置存在 `<数据目录>/git-settings.json`**（`settings::git_settings`，和 `budget.json` 同一种做法），宿主方法：
- `gitSettings.get`；
- `gitSettings.set {settings}`：检查后保存，返回存下的内容；
- `copies.list`：现有的独立副本，从新到旧，带任务标题。

**Git 页：提交说明的附加指引。**
- 一段文字，最多 2000 字，保存时去掉首尾空白；
- 生成提交说明时接在项目自己的提示后面（`GitSettings::commit_prompt`，`git::service::generate_commit_message_with`），宿主从设置文件读。

**分支前缀不做。** 页面上去掉这一行，搜索里也去掉。

**工作副本页：**
- **根目录：** 只显示，不能改，写明原因。
- **没有改动的副本保留多久：** 7 天、30 天（默认）、90 天、一年、不自动删。宿主启动时按这个天数调 `isolated_copy::prune_after`；还有改动的副本照旧不删。
- **现有的独立副本：** 每个写任务标题、项目名、创建日期、状态（“还有改动没带回”，或“已清空，约 N 天后删除”），可以在访达中显示。

## Alternatives considered

**照方案做分支前缀。** 没选：App 现在开的副本都不建分支，这个开关不会改变任何东西，违反“不放没作用的控件”。以后真有建分支的路径再加。

**根目录可改。** 没选：要改所有认这个位置的安全检查、排除规则和存储统计，而且已有副本的位置会和新设置不一致。现在没有人提出要改。

**按数量清理（超过 N 个删最旧的）。** 没选：最旧的副本可能还有没带回的改动，按数量删会丢工作，和任务清理定下的规则冲突。只按“清空了多久”删，删的都是没有东西可带回的。

**设置放进 `settings.json`。** 没选：那边每个字段一个方法，还有协议文档和调用测试要一起改。这几项只有设置页和两个宿主调用点用，单独一个文件更简单；预算设置也是这样存的。

## Consequences

- **收益：**
  - 生成的提交说明能按用户的要求写，比如用中文；
  - 能看到现在有哪些副本、各自什么时候会被删；
  - 能把清理调短、调长或关掉。
- **局限：**
  - 改了保留天数，要等下次宿主启动才按新天数清理；
  - 副本列表不显示大小，大小在“存储”页；
  - 页面上不能删副本，删除仍在会话的改动审查里做；
  - 界面没有目检。
- **验证：**
  - Rust：
    - `git_settings` 三条（默认值、保存时检查和去空白、指引接在项目提示后面）；
    - `only_an_emptied_copy_goes_after_the_kept_days`（还有改动的副本过一年也不删）；
    - 宿主 `git_settings_are_saved_and_checked`；
  - Swift：
    - `GitSettingsClientTests`；
    - `testACopySaysWhetherItWillBeRemoved`。
