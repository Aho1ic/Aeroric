# Agent Note: 导入 DeepSeek Harness、OMP、Qoder、Cursor、OpenCode 的会话

Status: implemented

## Problem

导入只支持 Claude 与 Codex。用户本机另有 DeepSeek Harness 58 个会话（另 18 个子代理）、OMP 三个配置档约 116 个、Qoder 国内版 31 个、OpenCode 18 个、Cursor 命令行版 1 个，都看不到。Aeroric 之后要分发给别人用，所以位置不能写死成本机的情况：别人可能装的是国际版、改过环境变量、只用 Cursor 编辑器。

另外两处旧限制：超过 32 MB 的会话不读；core 的 DSH 解析器只认格式 v0，而 DSH 0.9 写的是 `session.v4.jsonl.zstd`、header `version: 4`，Aeroric 读 DSH 历史会直接报"请升级"。

## Decision

- **结构**：`ImportAgent` 扩成七家。Claude / Codex 留在 `import.rs`；其余五家的发现、摘要、消息放进新文件 `import_sources.rs`。`import.rs` 只按 `is_external()` 分派，授权、索引、增量、排除、归档的规则不变。
- **发现按各家自己的规则**（宿主注入 home 与环境变量快照 `DiscoveryEnv`，core 不读真实进程环境）：
  - DSH：`DSH_HOME`，否则 `~/.dsh`；
  - OMP：`~/.omp/agent/sessions` 与任意名字的 `~/.omp/profiles/<名>/agent/sessions`，会话记下配置档名；
  - Qoder：`QODER_CONFIG_DIR`，否则 `QODER_CONFIG_DIR_NAME` 与已知发行目录（`.qoder`、`.qoder-cn`）；
  - Cursor：命令行版 `CURSOR_CONFIG_DIR` → `$XDG_CONFIG_HOME/cursor` → `~/.cursor`，编辑器 `~/Library/Application Support/Cursor/User`（Linux `~/.config/Cursor/User`）；
  - OpenCode：`$XDG_DATA_HOME/opencode`，默认 `~/.local/share/opencode`。
  主根没有会话、而别的位置有时，有会话的那个升为主根。一家多个根时，每个根一条授权记录（`grant_roots`）。
- **数据库源**：OpenCode 与 Cursor 编辑器是一个库里多个会话，索引新增 `sourceKey` 区分；会话 id 仍是 `imp_` + sha256，只是定位串带上 `#key`。SQLite 一律 `SQLITE_OPEN_READ_ONLY` + 3 秒 busy_timeout。数据库的增量标记用库文件加 `-wal` 的 mtime 与大小，或行内的更新时间。
- **跳过不是独立会话的**：DSH `delegationDepth > 0`、OpenCode `parent_id` 非空、Cursor `subagentInfo`、空的 Cursor 编辑器对话、Qoder 的 `subagents/`。这几种 `describe` 返回 `Ok(None)`，不计入错误。
- **来源自己归档的**：OpenCode `time_archived`、Cursor `isArchived` 记为 `archivedAtSource`，与 Codex 的 `archived_sessions` 一样默认显示为已归档。
- **DSH 解析器升到 v4**：接受 header 版本 0–4；会话目录里取最新的 `session.vN` 一代（同代两种编码取压缩的）；`source.kind` 不是 `user` 的 `user/message` 不显示（运行时上下文、压缩检查点、子代理通知）；v4 的 `tool/result` 本身就是结果消息（`toolCallId` + 文本 content），按此读。这同时修好了 DSH 会话历史的读取。
- **不再按体积拒绝**：超过 32 MB 的 jsonl 改用 `read_session_tail` 读最近的部分；`too_large` 不再置位，`TITLE_REV` 升到 3，让旧索引条目重算一次。
- **接着聊与分叉**（设计稿方案 A，液态玻璃胶囊）：
  - Claude、Codex、DSH、OMP 的会话点「接着聊」→ 宿主 `imports.continue` 新建任务，`attach_session` 绑定 Agent 自己的会话 id（Claude 文件名、Codex rollout 末尾的 uuid、DSH / OMP 的 header id）和源文件路径；下一次发送由对应 Harness 恢复原会话。`prefs.continued` 记下导入 id → 任务 id，接管后导入列表不再显示它，再次调用返回同一个任务；
  - 会话不在任何项目里时，按钮变成「添加为项目并接着聊」，先打开它的 cwd；
  - Qoder、Cursor、OpenCode 只能分叉：选一个 Harness，新建会话草稿，开场说明是标题加最近 6 轮文字（每轮 600 字、合计 4000 字封顶，工具调用不带），不自动发送；
  - 胶囊用现有的 `glassSurface(.floatingCard)`：macOS 26 是 Liquid Glass，更早的系统是材质，「降低透明度」时是不透明底。
- **目检后修的三处（2026-10-28 晚）**：
  - 宿主写回答复时，若某个答复超过一帧（1 MiB），以前 `write_frame` 失败后整条连接停写，同一连接上其他请求全部变成“disconnected”（用户看到“读取历史失败：disconnected”）。现在 dispatcher 的 `fit_frame` 把超限的成功答复换成同 id 的 IoError，连接不断；
  - 接着聊时客户端把这个 Harness 的默认配置（与新会话同一规则：本机登录优先）随请求带上，宿主写进任务的 `agentConfigId`、模型、思考强度、权限；
  - 会话页的模型列表以前是该 Agent 全部配置的并集，而且任务的 `agentConfigId`（`ds`）与目录里的 `agent-config:ds` 永远对不上，于是 Agent 配置的模型会跑在 CLI 自己的登录上（OpenAI 401）。现在只列任务自己那份配置；没固定配置的任务只列本机登录的模型。会话页也会主动加载项目的配置目录，不再空着；
  - Codex 新增的 `thread/goal/*` 通知按簿记处理，不再显示“未识别的内容”。
  - 连接一出错，客户端就杀掉并重启宿主（`HostSupervisor` 的 connectionFault 分支），正在启动的任务随之“已中断”。原因以前只写进 phase，没留下痕迹；2026-10-29 起客户端用 os_log（subsystem `com.aeroric.native`，category `supervisor`）记下每次断开的原因，宿主在读帧失败断开时往 host.log 写出是哪条帧规则（不写内容）。
- **界面**：导入页列出七家；会话行用三家的官方单色标志（lobehub 图标库，MIT）；Session ID 按各家命名读取（DSH 取 `session-<id>` 目录名，OMP 取文件名 `_` 之后，Cursor 取会话目录名，数据库源取 `sourceKey`）。

## Alternatives considered

- **搬 codeg 的 Tauri 解析器**：依赖 sea-orm、tokio 与 codeg 自己的模型，耦合重。只借它的路径规则与格式说明，自己写只读解析。
- **把记录复制进 Aeroric**：重复存储、和源不同步，也违背只读。否掉。
- **只扫默认目录**：别人电脑上的国内版、自定义目录、OMP 配置档会漏，违背"别人电脑上也能全量扫描"。否掉。
- **上 App Store、开 App Sandbox**：沙盒会把宿主与 Agent 命令行一起关住，读不了 `~/.claude` 之类的目录，也跑不了 Homebrew 里的命令行。用户选了 Developer ID 签名 + 公证（A1）。发现层与访问层分开，以后要换安全作用域书签，只改访问层。

## Consequences

- **收益**：
  - 七家会话都能扫描、查看；
  - 别人电脑上按各家自己的环境变量和发行目录找；
  - DSH 历史读取不再报版本错；
  - 大会话不再打不开。
- **代价与已知上限**：
  - Cursor 编辑器的读取是按公开格式写的，本机没有样本，只用合成库测过，要等真实数据验证；
  - Cursor 命令行版的 cwd 不落盘，只能按已打开项目路径的 md5 反查，项目没打开时归到未归类；
  - Cursor 命令行版只读 JSON 形式的消息 blob，protobuf 形式的工具细节不显示；
  - 原会话的工作目录若是项目的子文件夹，Claude 恢复时可能找不到原会话（它按 cwd 找记录）；
  - AGENTS.md 里"Native OMP session history boundary"那一节已经不在了，`omp.rs` 注释引用的是旧规则；导入走"用户逐家授权、只读"的通道，OMP 是用户明确选的（I3）。
