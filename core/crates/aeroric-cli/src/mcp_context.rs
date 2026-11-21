//! `aeroric-cli --data-dir D mcp-context`: the Aeroric context MCP server an
//! agent's session starts over stdio (B4′, 2026-10-30). Each tool call goes
//! to the host with the token from `AERORIC_CONTEXT_TOKEN`, which names the
//! one task this session belongs to; the host decides what it may read.

use std::path::Path;

use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::client::HostClient;

const PROTOCOL: &str = "2025-06-18";
/// CU 2026-11-13: DSH waits for `tools/list` before a turn starts. A host
/// busy at startup (a Keychain prompt after an ad hoc re-sign) held the yes/no
/// probes, and the turn never began. Past this, only the base tools are offered.
const HOST_PROBE: std::time::Duration = std::time::Duration::from_secs(3);
/// How often `ask_expert` looks for the expert's answer.
const EXPERT_POLL: std::time::Duration = std::time::Duration::from_secs(3);
/// C8 (2026-11-20): the longest line read from the agent. Past it the
/// line is answered with a structured error, never read whole into memory.
pub const MAX_LINE_BYTES: usize = 1 << 20;
/// What is kept of a too-long line to find its `id` in.
const HEAD_BYTES: usize = 4096;
/// C8: tools with a side effect that take an `idempotency_key`; the host
/// answers a repeated key with the first call's result.
pub(crate) const KEYED: [&str; 4] = ["delegate", "board_write", "board_decide", "memory_propose"];
pub(crate) const KEY_ARGUMENT: &str = "idempotency_key";

/// C8: every tool of every family, as a host with all of them on would
/// list them (the trigger evaluation offers the agent the full set; H2:
/// `ctx --help` lists them).
pub(crate) fn all_tools() -> Vec<Value> {
    [
        tools(),
        memory_tools(),
        wiki_tools(),
        reference_tools(),
        expert_tools(),
    ]
    .into_iter()
    .flat_map(|list| list.as_array().cloned().unwrap_or_default())
    .collect()
}

/// MCP tool → host method.
fn tools() -> Value {
    json!([
        {
            "name": "search_sessions",
            "description": "在本项目的历史会话（Claude Code、Codex、omp、DSH 都算）里按关键词检索，返回命中的会话与片段。用来找以前讨论过、做过的事。",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string", "description": "关键词，不区分大小写"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "最多返回几条，默认 10"}
            }}
        },
        {
            "name": "read_session",
            "description": "读取本项目某个会话的消息（默认当前会话的最近 40 条），工具调用以“[工具] 名称：参数 → 结果开头”一行出现。每条最多 4000 字，被截断的带 truncated 和 nextOffset，用 from=该条 index、offset=nextOffset 续读。taskId 来自 search_sessions 的结果。",
            "inputSchema": {"type": "object", "properties": {
                "taskId": {"type": "string"},
                "from": {"type": "integer", "minimum": 0, "description": "从第几条消息开始"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200},
                "offset": {"type": "integer", "minimum": 0, "description": "第 from 条从第几个字开始（续读被截断的消息）"},
                "mode": {"type": "string", "enum": ["latest_assistant"], "description": "latest_assistant：只要这个会话里 Agent 最后一条回复的原文（最多 2 万字），不分页、不带工具行；只想引用另一个任务的结论时用"}
            }}
        },
        {
            "name": "read_handoff",
            "description": "读取当前任务的交接清单：目标、已确认决定、已发生的改动、结果未知的操作、未完成义务、约束。每条都标明是已验证、推断还是未知。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "delegate",
            "description": "把一块工作委派给一个新的子会话（出现在用户侧栏，用户可随时接手）。在这些时候用它：要换一种 Harness 或模型、要用户能随时接手、要跑很久、要一份隔离的可写工作区、要按验收条款检查结果。同一会话里快速分头检索、查资料，用你自己的子代理就够了（在 Claude 和 OMP 里，Aeroric 角色库中适合这样用的角色也提供成了你的子代理，名字带 aeroric）。子会话权限不能高于你；同一时刻只能有一个可写的子会话；子会话不能再委派。立即返回；你发起的委派全部结束后，Aeroric 会把结果合并成一条消息发给你。请尽量给出可执行的验收条款。",
            "inputSchema": {"type": "object", "required": ["instruction"], "properties": {
                "instruction": {"type": "string", "description": "要子会话做什么，写清目标和边界"},
                "role": {"type": "string", "description": "角色 id 或名字（见 list_roles），例如 researcher / 调研员、reviewer、implementer、tester"},
                "agent": {"type": "string", "description": "same（与你同一种，默认）、other（另一种）、claude 或 codex"},
                "model": {"type": "string", "description": "agent 为 claude / codex 时可指定模型"},
                "contextMode": {"type": "string", "enum": ["fork", "brief", "isolated"], "description": "fork 带上你的对话（只能同一种）；brief 给交接清单；isolated 只给指令和文件"},
                "contextRefs": {"type": "array", "items": {"type": "string"}, "description": "子会话应先读的文件（相对工作目录）"},
                "permission": {"type": "string", "enum": ["plan", "ask", "auto_edit", "full_access"]},
                "workspace": {"type": "string", "enum": ["readOnly", "shared"], "description": "readOnly 只读；shared 与你共用目录可写"},
                "acceptance": {"type": "array", "items": {"type": "object", "required": ["text"], "properties": {
                    "text": {"type": "string"}, "check": {"type": "string", "description": "能判定通过的 shell 命令"}}}},
                "budgetMinutes": {"type": "integer", "minimum": 1, "description": "时长上限，默认 30"},
                "budgetTokens": {"type": "integer", "minimum": 1000}
            }}
        },
        {
            "name": "delegations",
            "description": "查看你发起过的委派及其状态、结论和验收结果；returnDelivery 说明结论有没有送到你这里（delivered 才是已送达，unknown 是结果不明）。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "list_roles",
            "description": "列出角色库：每个角色的默认上下文模式、权限和工作区。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "board_read",
            "description": "读共享黑板（主会话与它委派的子会话共用）：只返回你上次读取之后的变化。条目有目标、假设、认领、决定、证据、问题。",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "board_write",
            "description": "在共享黑板上新增一条（给 kind 和 text）或改一条（给 id，再给要改的 text / state / claimedBy）。你写的决定只是提议，由用户或主会话采纳。",
            "inputSchema": {"type": "object", "properties": {
                "id": {"type": "string", "description": "要改的条目，如 b3；新增时不填"},
                "kind": {"type": "string", "enum": ["goal", "hypothesis", "claim", "decision", "evidence", "question"]},
                "text": {"type": "string"},
                "state": {"type": "string", "enum": ["open", "confirmed", "refuted", "proposed", "answered", "done"],
                    "description": "假设：open/confirmed/refuted；认领：open/done；问题：open/answered"},
                "claimedBy": {"type": "string", "description": "认领这项工作的成员"},
                "subagent": {"type": "string", "description": "你是被派出来的子代理时填自己的名字（如 aeroric-researcher），黑板上会标明是你写的；主会话不填"}
            }}
        },
        {
            "name": "board_decide",
            "description": "采纳或否决黑板上提议中的决定。只有主会话能用。",
            "inputSchema": {"type": "object", "required": ["entryId", "accept"], "properties": {
                "entryId": {"type": "string"},
                "accept": {"type": "boolean"}
            }}
        },
        {
            "name": "todo_propose",
            "description": "把本轮范围以外、值得以后做的一件事提议成用户的待办，例如顺带发现的另一个 bug、该补的测试。只会进入待确认，由用户在 Aeroric 的待办页收下或丢掉。不要用它代替把当前的事做完，也不要提议你这一轮马上就会做的事。每个会话最多 5 条待确认的提议；同一项目里已有同名待办会被拒绝。",
            "inputSchema": {"type": "object", "required": ["title"], "properties": {
                "title": {"type": "string", "description": "要做什么，一句话，200 字以内"},
                "notes": {"type": "string", "description": "背景、涉及的文件、怎么算做完"}
            }}
        },
        {
            "name": "plan_propose",
            "description": "把一个大目标拆成几个有先后依赖的工作单元，提议成一个计划。只会存成草稿，用户在 Aeroric 里确认后才按依赖自动开委派：没有依赖的可以同时开始，有依赖的等前置通过验收后开始，并拿到前置的产物。最多 8 个单元，同时最多跑 3 个，同一目录同时只有一个会写文件的单元；单元之间不能有环。一两件事直接用 delegate，不要用它。",
            "inputSchema": {"type": "object", "required": ["title", "units"], "properties": {
                "title": {"type": "string", "description": "计划要达成的目标，一句话"},
                "units": {"type": "array", "maxItems": 8, "items": {"type": "object", "required": ["id", "title", "brief"], "properties": {
                    "id": {"type": "string", "description": "单元的短编号，如 u1，dependsOn 用它"},
                    "title": {"type": "string", "description": "单元名，几个字"},
                    "brief": {"type": "string", "description": "交给这个单元的子会话做什么、怎么算做完"},
                    "dependsOn": {"type": "array", "items": {"type": "string"}, "description": "必须先通过验收的单元编号"},
                    "workspace": {"type": "string", "enum": ["readOnly", "shared", "copy"], "description": "readOnly 只读（默认）；shared 在当前目录写；copy 在独立副本里写"},
                    "role": {"type": "string", "description": "角色库里的角色 id，可省略"},
                    "acceptance": {"type": "array", "items": {"type": "string"}, "description": "验收条款，全部通过才算这个单元完成"}
                }}}
            }}
        },
        {
            "name": "artifact_list",
            "description": "列出一个任务交付的文件（交付物）：每个有 id、路径、当前第几版、最后一次检查是否通过。默认是当前任务；readable 列出你还能读的任务（你委派或分出的、上级、同一上级下的），用 taskId 指定。委派结果里的“交付物”就是这里的 id@版本。",
            "inputSchema": {"type": "object", "properties": {
                "taskId": {"type": "string", "description": "要看的任务，默认当前任务"},
                "cursor": {"type": "string", "description": "上一页返回的 nextCursor"}
            }}
        },
        {
            "name": "artifact_read",
            "description": "读交付物的某一版，内容和用户在 Aeroric 里打开的同一版逐字节一致。文本按 utf8 返回，二进制按 base64；没读完时带 nextOffset，用 offset 续读。版本被撤回或内容已不在仓库里时会明说。",
            "inputSchema": {"type": "object", "required": ["artifact"], "properties": {
                "artifact": {"type": "string", "description": "交付物 id，可带版本，如 art-1a2b3c4d5e6f@3；不带版本读当前版"},
                "taskId": {"type": "string", "description": "交付物所在任务；不填就在你能读的任务里找"},
                "offset": {"type": "integer", "minimum": 0, "description": "从第几个字节读起"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 262144, "description": "最多读几个字节，默认 65536"}
            }}
        }
    ])
}

/// B5-1: offered only in a project that uses memory (R2-1).
fn memory_tools() -> Value {
    json!([
        {
            "name": "memory_recall",
            "description": "检索用户长期记忆：本项目的事实与约定、用户的偏好。返回带 ID、来源、日期的条目；找不到会明说。开始一项工作前、或不确定用户的约定时先查。",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string", "description": "关键词，空格分开，任一命中即可"}
            }}
        },
        {
            "name": "memory_propose",
            "description": "提议一条值得长期记住的内容（用户的偏好、本项目的约定或决定）。只会进入待确认，由用户在 Aeroric 里确认；不要提议密钥、临时状态或这次对话才用得上的细节。",
            "inputSchema": {"type": "object", "required": ["text"], "properties": {
                "text": {"type": "string", "description": "一句话，500 字以内"},
                "kind": {"type": "string", "enum": ["profile", "fact"], "description": "profile：关于用户本人；fact：项目的事实与约定（默认）"},
                "category": {"type": "string", "enum": ["language", "interaction", "preference", "constraint"], "description": "kind 为 profile 时的类别"},
                "project": {"type": "boolean", "description": "false 表示对用户所有项目都适用；默认只属于本项目"},
                "evidence": {"type": "string", "description": "用户原话或依据"}
            }}
        },
        {
            "name": "memory_feedback",
            "description": "告诉 Aeroric 一条用 memory_recall 查到的记忆或经验这次有没有用：有用的会更靠前、更可信，不对的会降级。",
            "inputSchema": {"type": "object", "required": ["id", "helpful"], "properties": {
                "id": {"type": "string", "description": "记忆的 ID，如 mem-1a2b3c"},
                "helpful": {"type": "boolean", "description": "true 有用；false 不对或过时"}
            }}
        }
    ])
}

fn method_of(tool: &str) -> Option<&'static str> {
    match tool {
        "search_sessions" => Some("context.searchSessions"),
        "read_session" => Some("context.readSession"),
        "read_handoff" => Some("context.readHandoff"),
        "delegate" => Some("context.delegate"),
        "delegations" => Some("context.delegations"),
        "list_roles" => Some("context.roles"),
        "board_read" => Some("context.boardRead"),
        "board_write" => Some("context.boardWrite"),
        "board_decide" => Some("context.boardDecide"),
        "todo_propose" => Some(aeroric_protocol::todo::PROPOSE_METHOD),
        "plan_propose" => Some(aeroric_protocol::work_plan::CONTEXT_PROPOSE_METHOD),
        "memory_recall" => Some("context.memoryRecall"),
        "memory_propose" => Some("context.memoryPropose"),
        "memory_feedback" => Some("context.memoryFeedback"),
        "read_wiki" => Some("context.wikiRead"),
        "search_wiki" => Some("context.wikiSearch"),
        "flag_wiki_issue" => Some("context.wikiFlag"),
        "search_references" => Some("context.referenceSearch"),
        "read_reference" => Some("context.referenceRead"),
        "artifact_list" => Some(aeroric_protocol::artifacts::CONTEXT_LIST_METHOD),
        "artifact_read" => Some(aeroric_protocol::artifacts::CONTEXT_READ_METHOD),
        _ => None,
    }
}

/// One MCP request → its response (`None` for notifications).
async fn answer(
    client: &mut Option<HostClient>,
    data_dir: &Path,
    token: &str,
    message: &Value,
) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let memory = memory_enabled(client, data_dir, token).await;
            let wiki = host_says(
                client,
                data_dir,
                token,
                "context.wikiAvailable",
                "available",
            )
            .await;
            let references = host_says(
                client,
                data_dir,
                token,
                "context.referencesAvailable",
                "available",
            )
            .await;
            let expert = host_says(
                client,
                data_dir,
                token,
                "context.expertAvailable",
                "available",
            )
            .await;
            Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "aeroric-context", "version": env!("CARGO_PKG_VERSION")},
                "instructions": instructions(memory, wiki, references, expert)
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let mut list = tools().as_array().cloned().unwrap_or_default();
            if memory_enabled(client, data_dir, token).await {
                list.extend(memory_tools().as_array().cloned().unwrap_or_default());
            }
            if host_says(
                client,
                data_dir,
                token,
                "context.wikiAvailable",
                "available",
            )
            .await
            {
                list.extend(wiki_tools().as_array().cloned().unwrap_or_default());
            }
            if host_says(
                client,
                data_dir,
                token,
                "context.referencesAvailable",
                "available",
            )
            .await
            {
                list.extend(reference_tools().as_array().cloned().unwrap_or_default());
            }
            if host_says(
                client,
                data_dir,
                token,
                "context.expertAvailable",
                "available",
            )
            .await
            {
                list.extend(expert_tools().as_array().cloned().unwrap_or_default());
            }
            for tool in &mut list {
                let keyed = tool["name"].as_str().is_some_and(|n| KEYED.contains(&n));
                if let (true, Some(properties)) =
                    (keyed, tool["inputSchema"]["properties"].as_object_mut())
                {
                    properties.insert(
                        KEY_ARGUMENT.into(),
                        json!({"type": "string", "description": "可选。自己起一个唯一的键；因为超时或断线要重试时带上同一个键，Aeroric 只执行一次并返回第一次的结果"}),
                    );
                }
            }
            Ok(json!({"tools": list}))
        }
        "tools/call" => Ok(call_tool(client, data_dir, token, &params).await),
        _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

/// R4-3: Codex defers this server's tools and shows the model only these
/// instructions, so a tool family they leave out is never loaded. The first
/// Wiki evaluation had all 16 sessions skip the Wiki for exactly that reason.
fn instructions(memory: bool, wiki: bool, references: bool, expert: bool) -> String {
    // H3: the same sentence the opening's purpose carries.
    aeroric_core::sessions::live::opening::tool_uses(
        aeroric_core::sessions::live::tool_catalog::Offered {
            memory,
            wiki,
            references,
            expert,
        },
    )
}

/// 2026-11-13 Smart Friend: offered once the user set an expert model.
fn expert_tools() -> Value {
    let minutes = aeroric_protocol::delegation::EXPERT_MINUTES;
    let per_turn = aeroric_protocol::delegation::EXPERT_ASKS_PER_TURN;
    json!([
        {
            "name": "ask_expert",
            "description": format!("遇到难点时请教一个更强的专家模型：卡住了、连续失败、拿不准方案，或要改安全相关的代码。专家只读，能看到你的交接清单，需要时会自己读项目文件。把背景、已试过什么、具体卡在哪写进问题里。会等专家答完（最多 {minutes} 分钟）再返回答案；每回合最多问 {per_turn} 次。"),
            "inputSchema": {"type": "object", "required": ["question"], "properties": {
                "question": {"type": "string", "description": "要问的问题，带上背景、已试过的做法和具体卡点"}
            }}
        }
    ])
}

/// Ask the expert and wait for the answer, looking every few seconds. Past
/// the wait the answer comes back as a delegation result after the turn.
async fn ask_expert(
    client: &mut Option<HostClient>,
    data_dir: &Path,
    token: &str,
    arguments: &serde_json::Map<String, Value>,
) -> Value {
    if client.is_none() {
        match HostClient::connect(data_dir, &uuid::Uuid::new_v4().to_string()).await {
            Ok(connected) => *client = Some(connected),
            Err(error) => return tool_error(format!("连不上 Aeroric：{error}")),
        }
    }
    let Some(host) = client.as_mut() else {
        return tool_error("连不上 Aeroric".into());
    };
    let question = arguments.get("question").cloned().unwrap_or(Value::Null);
    let started = match host
        .call(
            "context.askExpert",
            json!({"token": token, "question": question}),
        )
        .await
    {
        Ok(started) => started,
        Err(error) => {
            *client = None;
            return tool_error(error.to_string());
        }
    };
    let id = started["delegationId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let label = started["label"].as_str().unwrap_or("专家").to_string();
    let minutes = aeroric_protocol::delegation::EXPERT_MINUTES;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(u64::from(minutes) * 60);
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(EXPERT_POLL).await;
        let Some(host) = client.as_mut() else {
            break;
        };
        match host
            .call(
                "context.expertAnswer",
                json!({"token": token, "delegationId": id}),
            )
            .await
        {
            Ok(reply) => match reply["state"].as_str() {
                Some("answered") => {
                    let text = reply["text"].as_str().unwrap_or_default();
                    return json!({"content": [{"type": "text",
                        "text": format!("专家（{label}）的回答：\n\n{text}")}], "isError": false});
                }
                Some("failed") => {
                    let why = reply["text"].as_str().unwrap_or("没有回答");
                    return tool_error(format!("专家（{label}）没答出来：{why}"));
                }
                _ => {}
            },
            Err(error) => {
                *client = None;
                return tool_error(format!("等专家回答时和 Aeroric 断开了：{error}"));
            }
        }
    }
    json!({"content": [{"type": "text", "text": format!(
        "专家（{label}）还在想，已经等了 {minutes} 分钟。它答完后，答案会作为委派结果在你这一回合结束后送回；现在可以先按已知信息继续，或结束这一回合等它。"
    )}], "isError": false})
}

/// Q-R6: offered when the user registered at least one reference folder.
fn reference_tools() -> Value {
    json!([
        {
            "name": "search_references",
            "description": "在用户收集的参考代码库（本机的其他仓库，只读）里检索：类名、函数名优先匹配定义，其余按全文相关度。返回 仓库 · 路径:行号 和那一行。动手实现前想看别人怎么写同类功能时用。",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string", "description": "关键词，空格分开，任一命中即可；中英文都行"},
                "repo": {"type": "string", "description": "只搜这个参考库（名字或 id）；省略搜全部"}
            }}
        },
        {
            "name": "read_reference",
            "description": "读参考代码库里的一个文件（带行号，一次最多 400 行）。路径相对于该参考库，不能跳出它。",
            "inputSchema": {"type": "object", "required": ["repo", "path"], "properties": {
                "repo": {"type": "string", "description": "参考库的名字或 id（search_references 结果里的仓库名）"},
                "path": {"type": "string", "description": "相对路径，如 ultralytics/nn/modules/head.py"},
                "start": {"type": "integer", "description": "起始行，默认 1"},
                "end": {"type": "integer", "description": "结束行，默认起始行后 200 行"}
            }}
        }
    ])
}

/// B5.5-1: offered when the session's project has Wiki pages (Q-W5).
fn wiki_tools() -> Value {
    json!([
        {
            "name": "read_wiki",
            "description": "读本项目 Wiki 的一页（Markdown，段落带来源）。不给 slug 时读索引页，索引里列出所有页面。决策页记录了项目做过的设计决定与理由，操作手册记录了可复用的做法。",
            "inputSchema": {"type": "object", "properties": {
                "slug": {"type": "string", "description": "页面 slug，如 decision-delegation、runbook；省略读索引"}
            }}
        },
        {
            "name": "search_wiki",
            "description": "按关键词检索本项目 Wiki，返回页面 slug、标题与片段；再用 read_wiki 读全文。",
            "inputSchema": {"type": "object", "required": ["query"], "properties": {
                "query": {"type": "string", "description": "关键词，空格分开，任一命中即可"}
            }}
        },
        {
            "name": "flag_wiki_issue",
            "description": "发现 Wiki 某页内容过时、错误或缺失时报告给用户（进入该项目 Wiki 的 issue 列表），不会直接改页面。",
            "inputSchema": {"type": "object", "required": ["slug", "text"], "properties": {
                "slug": {"type": "string"},
                "text": {"type": "string", "description": "哪里不对、依据是什么"}
            }}
        }
    ])
}

/// Asks the host a yes/no about this session (`field` of `method`'s reply);
/// `false` when the host cannot be reached.
async fn host_says(
    client: &mut Option<HostClient>,
    data_dir: &Path,
    token: &str,
    method: &str,
    field: &str,
) -> bool {
    if client.is_none() {
        match HostClient::connect(data_dir, &uuid::Uuid::new_v4().to_string()).await {
            Ok(connected) => *client = Some(connected),
            Err(_) => return false,
        }
    }
    let Some(host) = client.as_mut() else {
        return false;
    };
    match tokio::time::timeout(HOST_PROBE, host.call(method, json!({ "token": token }))).await {
        Ok(Ok(result)) => result[field].as_bool().unwrap_or(false),
        // A late answer would land on the next call: start that one afresh.
        Ok(Err(_)) | Err(_) => {
            *client = None;
            false
        }
    }
}

/// Whether the session's project uses memory; `false` when the host cannot
/// be reached.
async fn memory_enabled(client: &mut Option<HostClient>, data_dir: &Path, token: &str) -> bool {
    if client.is_none() {
        match HostClient::connect(data_dir, &uuid::Uuid::new_v4().to_string()).await {
            Ok(connected) => *client = Some(connected),
            Err(_) => return false,
        }
    }
    let Some(host) = client.as_mut() else {
        return false;
    };
    let call = host.call("context.memoryEnabled", json!({ "token": token }));
    match tokio::time::timeout(HOST_PROBE, call).await {
        Ok(Ok(result)) => result["enabled"].as_bool().unwrap_or(false),
        Ok(Err(_)) | Err(_) => {
            *client = None;
            false
        }
    }
}

/// One MCP `tools/call` → its result. H2: `aeroric-cli ctx` calls it too,
/// so the command line and MCP answer alike.
pub(crate) async fn call_tool(
    client: &mut Option<HostClient>,
    data_dir: &Path,
    token: &str,
    params: &Value,
) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if name == "ask_expert" {
        let arguments = params
            .get("arguments")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        // C8: its answers say how they went too; a failure there (no answer,
        // a lost host) may be tried again.
        let mut result = ask_expert(client, data_dir, token, &arguments).await;
        if result.get("structuredContent").is_none() {
            let failed = result["isError"].as_bool() == Some(true);
            result["structuredContent"] = json!({
                "status": if failed { Status::Unclear.as_str() } else { Status::Ok.as_str() },
                "retryable": failed,
            });
        }
        return result;
    }
    let Some(method) = method_of(name) else {
        return outcome(
            format!("没有这个工具：{name}"),
            Status::Rejected,
            &json!({}),
        );
    };
    let mut arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // C8: a call with a side effect always carries a key — the agent's, or
    // one made here — and says it back as `operationId`.
    let key = KEYED
        .contains(&name)
        .then(|| match arguments.remove(KEY_ARGUMENT) {
            Some(Value::String(key)) if !key.trim().is_empty() => key,
            _ => format!("auto-{}", uuid::Uuid::new_v4().simple()),
        });
    arguments.insert("token".into(), Value::String(token.to_string()));
    let mut extra = json!({});
    if let Some(key) = &key {
        extra["operationId"] = json!(key);
    }
    if client.is_none() {
        match HostClient::connect(data_dir, &uuid::Uuid::new_v4().to_string()).await {
            Ok(connected) => *client = Some(connected),
            // Nothing was sent: safe to call again.
            Err(error) => {
                return outcome(format!("连不上 Aeroric：{error}"), Status::Retry, &extra)
            }
        }
    }
    let Some(host) = client.as_mut() else {
        return outcome("连不上 Aeroric".into(), Status::Retry, &extra);
    };
    match host
        .call_keyed(method, Value::Object(arguments), key.clone())
        .await
    {
        Ok(result) => {
            let text = match result
                .get("brief")
                .or_else(|| result.get("text"))
                .and_then(Value::as_str)
            {
                Some(brief) => brief.to_string(),
                None => serde_json::to_string_pretty(&result).unwrap_or_default(),
            };
            let status = if result.get("partial").and_then(Value::as_bool) == Some(true) {
                Status::Partial
            } else {
                Status::Ok
            };
            if let Some(next) = result.get("nextOffset") {
                extra["cursor"] = next.clone();
            }
            let refs: Vec<Value> = result
                .get("artifacts")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(|a| a.get("id").cloned()).collect())
                .unwrap_or_default();
            if !refs.is_empty() {
                extra["artifactRefs"] = json!(refs);
            }
            if result.get("replayed").and_then(Value::as_bool) == Some(true) {
                extra["replayed"] = json!(true);
            }
            outcome(text, status, &extra)
        }
        Err(error) => {
            // A lost connection is retried on the next call.
            let (status, text) = match &error {
                crate::client::ClientError::Rpc { code, message } => {
                    extra["code"] = json!(code.as_str());
                    if code.is_retryable_by_default() {
                        (Status::Retry, message.clone())
                    } else {
                        (Status::Rejected, message.clone())
                    }
                }
                // Sent, but no answer: a side effect may or may not have
                // happened. The same key finds out without doing it twice.
                crate::client::ClientError::Transport(reason) if key.is_some() => {
                    *client = None;
                    (
                        Status::Unclear,
                        format!("Aeroric 没有回应（{reason}），不知道这次有没有做成；带上同一个 {KEY_ARGUMENT} 再调用一次，做过的不会重复做，会返回第一次的结果"),
                    )
                }
                other => {
                    *client = None;
                    (Status::Retry, other.to_string())
                }
            };
            outcome(text, status, &extra)
        }
    }
}

/// C8: what a tool call came to, besides its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    Partial,
    /// Safe to call again (nothing happened, or the host was busy).
    Retry,
    /// The host refused it; calling again the same way will not help.
    Rejected,
    /// It may or may not have happened.
    Unclear,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Partial => "partial",
            Status::Retry => "retry",
            Status::Rejected => "rejected",
            Status::Unclear => "unclear",
        }
    }
}

/// A tool result: the text, as every harness reads it, and (C8)
/// `structuredContent` with the status and whatever `extra` carries.
fn outcome(text: String, status: Status, extra: &Value) -> Value {
    let mut structured = json!({
        "status": status.as_str(),
        "retryable": matches!(status, Status::Retry | Status::Unclear),
    });
    if let (Some(into), Some(from)) = (structured.as_object_mut(), extra.as_object()) {
        for (key, value) in from {
            into.insert(key.clone(), value.clone());
        }
    }
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": !matches!(status, Status::Ok | Status::Partial),
    })
}

fn tool_error(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

pub async fn serve(data_dir: &Path) -> i32 {
    let token =
        std::env::var(aeroric_core::sessions::live::context_server::TOKEN_ENV).unwrap_or_default();
    if token.is_empty() {
        eprintln!(
            "aeroric-cli mcp-context: no token; Aeroric starts this server for its own sessions"
        );
        return crate::args::exit::USAGE;
    }
    serve_with(
        data_dir,
        &token,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await;
    0
}

/// One line from the agent, or (C8) how long it was when past the cap.
enum Line {
    Text(Vec<u8>),
    TooLong { observed: usize, head: Vec<u8> },
}

/// A line of at most `max` bytes; a longer one is read through to its end
/// but only its head kept. `None` at the end of the input.
async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<Line>> {
    let mut kept = Vec::new();
    let mut observed = 0usize;
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            if observed == 0 {
                return Ok(None);
            }
            break;
        }
        let (chunk, done) = match buf.iter().position(|b| *b == b'\n') {
            Some(at) => (&buf[..at], Some(at + 1)),
            None => (buf, None),
        };
        observed += chunk.len();
        let room = if observed <= max { max } else { HEAD_BYTES };
        let take = room.saturating_sub(kept.len()).min(chunk.len());
        kept.extend_from_slice(&chunk[..take]);
        let used = done.unwrap_or(buf.len());
        reader.consume(used);
        if done.is_some() {
            break;
        }
    }
    if observed > max {
        kept.truncate(HEAD_BYTES);
        return Ok(Some(Line::TooLong {
            observed,
            head: kept,
        }));
    }
    Ok(Some(Line::Text(kept)))
}

/// C8: the top-level `id` of a line that did not parse (or was cut), when
/// one can be read from its text: the first `"id":` followed by a number
/// or a string.
fn salvage_id(text: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(text);
    let mut rest: &str = &text;
    while let Some(at) = rest.find("\"id\"") {
        let after = rest[at + 4..].trim_start();
        rest = &rest[at + 4..];
        let Some(value) = after.strip_prefix(':').map(str::trim_start) else {
            continue;
        };
        if let Some(string) = value.strip_prefix('"') {
            if let Some(end) = string.find('"') {
                return Some(Value::String(string[..end].to_string()));
            }
        }
        let digits: String = value
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        if let Ok(number) = digits.parse::<i64>() {
            return Some(json!(number));
        }
    }
    None
}

/// The server over any pair of streams (stdio in use, a pipe in tests):
/// every request with an id gets an answer, whatever its line held.
pub(crate) async fn serve_with<R, W>(data_dir: &Path, token: &str, mut reader: R, mut out: W)
where
    R: AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut client: Option<HostClient> = None;
    loop {
        let line = match read_line(&mut reader, MAX_LINE_BYTES).await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => break,
        };
        let response = match line {
            Line::TooLong { observed, head } => match salvage_id(&head) {
                Some(id) => Some(json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32600,
                    "message": format!("这一行有 {observed} 字节，超过 {MAX_LINE_BYTES} 字节的上限，没有处理；请把内容拆小，或写进文件再传路径"),
                    "data": {"limitKind": "lineBytes", "allowed": MAX_LINE_BYTES, "observed": observed, "retryable": false}
                }})),
                None => {
                    eprintln!("aeroric-cli mcp-context: dropped a {observed}-byte line with no readable id");
                    None
                }
            },
            Line::Text(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(message) => answer(&mut client, data_dir, token, &message).await,
                Err(error) => match salvage_id(&bytes) {
                    Some(id) => Some(json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32700,
                        "message": format!("不是合法的 JSON：{error}"),
                        "data": {"retryable": false}
                    }})),
                    None => {
                        if !bytes.iter().all(u8::is_ascii_whitespace) {
                            eprintln!("aeroric-cli mcp-context: dropped a line that is not JSON and has no id: {error}");
                        }
                        None
                    }
                },
            },
        };
        if let Some(response) = response {
            let mut bytes = serde_json::to_vec(&response).unwrap_or_default();
            bytes.push(b'\n');
            if out.write_all(&bytes).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// C8 (2026-11-20): every line with an id is answered — a line past the
    /// cap with a structured error saying how long it was, a line that is
    /// not JSON with a parse error — and the server goes on; a line with no
    /// readable id is not answered (there is no one to answer).
    #[tokio::test]
    async fn every_line_with_an_id_gets_an_answer() {
        let long = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{{\"text\":\"{}\"}}}}\n",
            "a".repeat(super::MAX_LINE_BYTES + 10)
        );
        let input = format!(
            "{long}{{\"jsonrpc\":\"2.0\",\"id\":\"x8\",\"method\":\nnot json at all\n\n{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}}\n"
        );
        let mut out = Vec::new();
        super::serve_with(
            Path::new("/nonexistent"),
            "t",
            tokio::io::BufReader::new(input.as_bytes()),
            &mut out,
        )
        .await;
        let answers: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(answers.len(), 3, "{answers:?}");
        assert_eq!(answers[0]["id"], 7);
        assert_eq!(answers[0]["error"]["code"], -32600);
        let data = &answers[0]["error"]["data"];
        assert_eq!(data["limitKind"], "lineBytes");
        assert_eq!(data["allowed"], super::MAX_LINE_BYTES);
        assert!(data["observed"].as_u64().unwrap() > super::MAX_LINE_BYTES as u64);
        assert_eq!(data["retryable"], false);
        assert_eq!(answers[1]["id"], "x8");
        assert_eq!(answers[1]["error"]["code"], -32700);
        assert_eq!(answers[2]["id"], 9);
        assert_eq!(answers[2]["result"], json!({}));
    }

    #[test]
    fn an_id_is_salvaged_from_a_broken_line() {
        assert_eq!(
            super::salvage_id(br#"{"jsonrpc":"2.0", "id" : 12, "me"#),
            Some(json!(12))
        );
        assert_eq!(
            super::salvage_id(br#"{"id":"abc","params":{"id":3}"#),
            Some(json!("abc"))
        );
        assert_eq!(super::salvage_id(br#"{"method":"x"}"#), None);
        assert_eq!(super::salvage_id(b"garbage"), None);
    }

    /// C8: a tool result carries `structuredContent` with its status; a
    /// keyed tool says its operation id (the agent's key, or one made);
    /// tools/list offers the key only on the tools that take one.
    #[tokio::test]
    async fn results_say_how_the_call_went() {
        let dir = Path::new("/nonexistent");
        let mut client = None;
        let call = |name: &str, arguments: Value| {
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": name, "arguments": arguments}})
        };
        let unknown = answer(&mut client, dir, "t", &call("rm_rf", json!({})))
            .await
            .unwrap();
        assert_eq!(unknown["result"]["structuredContent"]["status"], "rejected");
        assert_eq!(unknown["result"]["structuredContent"]["retryable"], false);
        assert_eq!(unknown["result"]["isError"], true);
        let keyed = answer(
            &mut client,
            dir,
            "t",
            &call(
                "delegate",
                json!({"instruction": "x", "idempotency_key": "k-1"}),
            ),
        )
        .await
        .unwrap();
        let structured = &keyed["result"]["structuredContent"];
        assert_eq!(structured["status"], "retry", "no host: nothing was sent");
        assert_eq!(structured["retryable"], true);
        assert_eq!(structured["operationId"], "k-1");
        let made = answer(
            &mut client,
            dir,
            "t",
            &call("board_write", json!({"text": "x"})),
        )
        .await
        .unwrap();
        assert!(made["result"]["structuredContent"]["operationId"]
            .as_str()
            .unwrap()
            .starts_with("auto-"));
        let read = answer(&mut client, dir, "t", &call("board_read", json!({})))
            .await
            .unwrap();
        assert!(read["result"]["structuredContent"]
            .get("operationId")
            .is_none());

        let list = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .await
        .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        let schema = |name: &str| {
            tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"]["properties"].clone()
        };
        assert!(schema("delegate").get("idempotency_key").is_some());
        assert!(schema("board_decide").get("idempotency_key").is_some());
        assert!(schema("board_read").get("idempotency_key").is_none());
    }

    /// C8: the host took a keyed call and went away before answering: the
    /// result says it is unclear whether it happened, and how to find out.
    #[tokio::test]
    async fn a_host_lost_mid_call_is_unclear_for_a_keyed_tool() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!(
            "aeroric-mcp-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("host.token"), "tok").unwrap();
        let socket = aeroric_host::uds::default_socket_path(&dir);
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let seen = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = tokio::io::BufReader::new(read).lines();
            let hello: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            let reply = json!({"v": 1, "id": hello["id"], "ok": true, "result": {}});
            write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            let call: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            // Gone without an answer.
            call
        });
        let mut client = None;
        let lost = answer(
            &mut client,
            &dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": "memory_propose", "arguments": {"text": "x", "idempotency_key": "k-9"}}}),
        )
        .await
        .unwrap();
        let sent = seen.await.unwrap();
        assert_eq!(sent["method"], "context.memoryPropose");
        assert_eq!(
            sent["idempotencyKey"], "k-9",
            "the key rides in the envelope"
        );
        assert!(
            sent["params"].get("idempotency_key").is_none(),
            "not in the method's own params"
        );
        let structured = &lost["result"]["structuredContent"];
        assert_eq!(structured["status"], "unclear");
        assert_eq!(structured["retryable"], true);
        assert_eq!(structured["operationId"], "k-9");
        assert!(lost["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("同一个 idempotency_key"));
        assert!(client.is_none(), "the next call connects again");
    }

    /// C8 (2026-11-20): every tool this server can list is in the shared
    /// table the tool catalog is built from, with the same family; nothing
    /// in the table is missing here.
    #[test]
    fn the_listed_tools_match_the_shared_table() {
        use aeroric_protocol::tools::{ToolGroup, AERORIC_TOOLS};
        let families = [
            (super::tools(), ToolGroup::Base),
            (super::memory_tools(), ToolGroup::Memory),
            (super::wiki_tools(), ToolGroup::Wiki),
            (super::reference_tools(), ToolGroup::References),
            (super::expert_tools(), ToolGroup::Expert),
        ];
        let mut listed: Vec<(String, ToolGroup)> = families
            .iter()
            .flat_map(|(list, group)| {
                list.as_array()
                    .unwrap()
                    .iter()
                    .map(|t| (t["name"].as_str().unwrap().to_string(), *group))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut table: Vec<(String, ToolGroup)> = AERORIC_TOOLS
            .iter()
            .map(|(name, group, _)| (name.to_string(), *group))
            .collect();
        listed.sort();
        table.sort();
        assert_eq!(listed, table);
    }

    use super::*;

    #[tokio::test]
    async fn initialize_and_tools_list_answer_without_a_host() {
        let mut client = None;
        let dir = Path::new("/nonexistent");
        let init = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}}),
        )
        .await
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "aeroric-context");
        let list = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .await
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "search_sessions",
                "read_session",
                "read_handoff",
                "delegate",
                "delegations",
                "list_roles",
                "board_read",
                "board_write",
                "board_decide",
                "todo_propose",
                "plan_propose",
                "artifact_list",
                "artifact_read"
            ]
        );
        assert_eq!(method_of("artifact_list"), Some("context.artifactList"));
        assert_eq!(method_of("artifact_read"), Some("context.artifactRead"));
        assert!(answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .await
        .is_none());
    }

    #[test]
    fn the_instructions_name_every_offered_tool_family() {
        let base = instructions(false, false, false, false);
        assert!(base.contains("历史会话") && !base.contains("记忆") && !base.contains("Wiki"));
        assert!(base.contains("todo_propose"));
        assert!(base.contains("artifact_read"));
        assert!(!base.contains("参考代码库") && !base.contains("专家"));
        let all = instructions(true, true, true, true);
        assert!(all.contains("长期记忆") && all.contains("Wiki") && all.contains("参考代码库"));
        assert!(all.contains("ask_expert"));
    }

    /// 2026-11-13 Smart Friend: one tool, waiting on the host, not a plain mapping.
    #[test]
    fn the_expert_tool_waits_instead_of_mapping_to_one_call() {
        let tools = expert_tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["ask_expert"]);
        assert!(tools[0]["description"]
            .as_str()
            .unwrap()
            .contains("10 分钟"));
        assert_eq!(method_of("ask_expert"), None);
    }

    #[test]
    fn the_reference_tools_map_to_host_methods() {
        assert_eq!(
            method_of("search_references"),
            Some("context.referenceSearch")
        );
        assert_eq!(method_of("read_reference"), Some("context.referenceRead"));
        let tools = reference_tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["search_references", "read_reference"]);
    }

    #[tokio::test]
    async fn the_memory_tools_map_to_host_methods() {
        assert_eq!(method_of("memory_recall"), Some("context.memoryRecall"));
        assert_eq!(method_of("memory_propose"), Some("context.memoryPropose"));
        let names: Vec<String> = memory_tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            ["memory_recall", "memory_propose", "memory_feedback"]
        );
        assert_eq!(method_of("memory_feedback"), Some("context.memoryFeedback"));
        let wiki: Vec<String> = wiki_tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(wiki, ["read_wiki", "search_wiki", "flag_wiki_issue"]);
        assert_eq!(method_of("search_wiki"), Some("context.wikiSearch"));
    }

    #[tokio::test]
    async fn an_unknown_tool_or_method_is_an_error_not_a_crash() {
        let mut client = None;
        let dir = Path::new("/nonexistent");
        let call = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "rm_rf", "arguments": {}}}),
        )
        .await
        .unwrap();
        assert_eq!(call["result"]["isError"], true);
        let unknown = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}),
        )
        .await
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        // No host to reach: an error result, the server keeps going.
        let offline = answer(
            &mut client,
            dir,
            "t",
            &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": {"name": "read_handoff"}}),
        )
        .await
        .unwrap();
        assert_eq!(offline["result"]["isError"], true);
    }
}
