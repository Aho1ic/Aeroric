# P3-02 侦察底稿:四族命令构造与托管配置(2026-09-26)

> 性质:实施前的机械盘点,数字来自 P3-01 收口后的真实源码。消费方式:每节都是后续切片的输入。

## 1. 命令构造层(pty.rs)

四个 builder 全部很薄——真正的"计划"早在 `AgentLaunchSpec`(launch_spec.rs 从用户配置/内置档案解析 program+args),builder 只做权限档注入与托管 env:

| builder | 权限注入 | 特有逻辑 |
|---|---|---|
| `build_claude_cmd` (718) | `claude_permission_args` | 无(鼠标上报的历史决策只留在注释) |
| `build_codex_cmd` (744) | `codex_permission_args` | 无 |
| `build_dsh_cmd` (767) | `dsh_permission_mode` → env | `--profile headless`(非交互)、`DSH_TELEMETRY_DISABLED=1` 兜底 |
| `build_omp_cmd` (801) | `omp_permission_flag` → `--approval-mode` | **绝不传 `--mode`**(上游按 undefined 判定交互 TUI) |

launch-args 层:`add_dsh_launch_args`(DSH_HOME + --patch 顺序)、`add_omp_launch_args`(托管 home env + `--cwd/--model/--thinking/--resume`,**flag 必须在位置参数前**)。env 层:`setup_env`(login-shell 合并 + 强制 truecolor,354)、`setup_omp_env`(**必须后于 setup_env**:重删 `PI_PROFILE`/`OMP_PROFILE`、重放 omp_agent_env、setup_aeroric_env)——后者依赖前者,顺序是硬约束。

## 2. 映射表层(agent_family_maps.rs,132 行,纯)

`omp_permission_flag`/`omp_thinking_level`/`dsh_permission_mode`/`claude_permission_args`/`codex_permission_args` + 2 条测试。零依赖,**P3-02 首片已 move 到 core `agents/launch.rs`**(补记八十八)。

## 3. app_settings 各文件规模与性质

| 文件 | 行数 | 性质 |
|---|---|---|
| `launch_spec.rs` | 752 | `AgentLaunchSpec{program,args,...}` 解析;`get_agent_launch_spec` 读用户 start 脚本内容动态判 launch 族(G18 三站点之一)——**读真实用户数据,不做单测**(补记二十九口径) |
| `agent_env.rs` | 312 | login-shell 环境合并的解析半边(纯);执行半边在 pty `setup_env` |
| `agent_scripts.rs` | 3525 | 内置脚本生成器(纯字符串) |
| `builtin.rs` | 328 | 内置档案表(纯) |
| `custom_agents.rs` | 372 | 自定义档案读写(文件 I/O) |
| `agent_family_maps.rs` | 132 | 已迁入 core(首片) |
| `dsh_home.rs` | 1046 | 托管 DSH_HOME/patch 层(文件 I/O + registry) |
| `omp_home.rs` | 537 | 托管 omp home(config.yml 版本戳 + mcp.json 重生成) |
| `hooks.rs` | 1051 | hook 配置改写 + `usable_for` 版本探测(会启子进程) |
| `mcp.rs` | 1233 | MCP 配置打包 |

## 4. 切片顺序(每片自身可绿)

1. ~~映射表 move + `LaunchPlan` 类型~~(已落,补记八十八)。
2. `build_*_cmd`/`add_*_launch_args` 的**计划半边**进 core(产出 `LaunchPlan`),宿主 builder 变"LaunchPlan → CommandBuilder"的机械翻译;**parity fixtures**(4 家族×3 权限档×prompt 空/非空×resume/new×custom/home/env)由旧宿主路径亲自算出期望、core 重放(沿用 P2-04 parity 方法)。
3. `agent_env.rs` 纯半边 + `setup_env` 顺序约束的 core 侧表达;omp_env 的 PI_PROFILE 清除反例(OMP 不变量)。
4. `managed_home/`:omp_home/dsh_home/hooks 安装逻辑迁入;`include_str!` hook 文件随迁改路径;单一资源来源(删旧前改全引用)。
5. SwitchEvidence/`adopt_session_for_agent` 真实实现(P2-04 Q43 移交项)+ G18 裁决窗口。
6. `tests/agent_launch.rs` + `omp_invariants.rs` 齐后注册 `pnpm native:check agents`;`grill-me [agents]` 收口。

## 5. 门禁对照

- `pnpm native:check agents`:**尚未注册**(suite 未成形);OMP 反例(hook 扩展名、willContinue、`--mode` 禁用、7 档 thinking)在 omp_invariants 成形时同轮注册。
- core 基线:309 → **312**(首片 +3:映射表 2 条迁入 + LaunchPlan 1 条新增)。
- src-tauri `--lib`:2467 → **2465**(−2 搬家;地板 2465 两处同改读回)。
