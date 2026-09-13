# Rust 空转测试清单(12 条)

## src-tauri/src/database/transfer.rs:446

test_name: transfer_progress_event_helper_is_callable | kind: 2 | severity: 重要 | why: 整个测试体只有 `let _ = emit_transfer_progress as fn(&AppHandle, TransferProgress);` —— 一个纯编译期函数指针 coercion,运行时零行为、零断言,连函数都没调用。被测函数只有一行 `app.emit("dbx-transfer-progress", progress)`(transfer.rs:19),而真正的契约是那个事件名字符串:前端在 src/lib/databaseApi.ts:107 用 listen("dbx-transfer-progress") 收。把第 19 行改成 emit("dbx-transfer-progres") 或 emit("transfer-progress"),测试照绿,而前端进度条永久不动、传输永远显示不出终态。改 payload 结构、改成不 emit 直接 return,同样照绿 —— 只要签名不变就抓不到任何东西。 | suggest: 用 tauri::test::mock_app 收一次事件,断言事件名等于 "dbx-transfer-progress" 且 payload 的 transferId/status 字段按 camelCase 落在前端 DbxTransferProgress 期望的形状上(与 query.rs:939 的 confirm_request_payload_is_camel_case_for_the_frontend 同口径);若不便起 mock_app,至少断言 serde_json::to_value(TransferProgress{..}) 的键名与前端契约一致,并把事件名提成常量供两处共享。

## src-tauri/src/agent_tools.rs:2729

test_name: platform_matrix_covers_all_requested_native_targets | kind: 6 | severity: 重要 | why: 14 条断言全是 `assert!(claude_platform_for(..).is_ok())` / `assert!(codex_target_for(..).is_ok())`,只校验「不是 Err」,从不校验返回的 slug 是什么。这两个函数(agent_tools.rs:542 与 :567)的每个 match 臂都返回 Ok,唯一的 Err 是兜底臂,所以把 544 与 545 行对调(macos/aarch64 → "darwin-x64"、macos/x86_64 → "darwin-arm64")测试照绿;把 569/570 的 aarch64-apple-darwin 与 x86_64-apple-darwin 对调、或把 linux 的 musl/glibc 后缀写反(547~550 与 551~554),同样照绿。这些 slug 直接拼进下载 URL,写反的后果是用户装到跑不起来的异架构二进制,而这是全仓库唯一覆盖这两个函数的测试(grep claude_platform_for/codex_target_for 只有实现、一个 env 包装和这条测试)。 | suggest: 改成 assert_eq! 逐对钉住映射表,例如 assert_eq!(claude_platform_for("macos", "aarch64", LinuxLibc::Glibc).unwrap(), "darwin-arm64") 覆盖 8 个 claude 组合 + 6 个 codex 组合,并保留一条 unsupported 组合返回 AgentInstallErrorCode::UnsupportedPlatform 的负例。

## src-tauri/src/permissions/linux.rs:347

test_name: session_detection_prefers_xdg_session_type | kind: 2 | severity: 重要 | why: 唯一断言是 assert!(matches!(session(), Session::X11 | Session::Wayland | Session::Unknown)),而 Session 枚举(linux.rs:45)恰好只有这三个变体 —— 这个 pattern 穷尽整个枚举,除了 session() 自己 panic 之外不存在能让它变红的返回值。测试名声称验证「优先看 XDG_SESSION_TYPE」,但把 linux.rs:52 起的整个函数体删掉、改成 `fn session() -> Session { Session::Unknown }`,它照绿;把 54/55 行的 wayland 与 x11 分支对调、或把 XDG_SESSION_TYPE 的 match 整段删掉只留 DISPLAY 兜底,也照绿。而 session() 的返回值直接决定 screen_recording_probe(linux.rs:141 起)报 Granted 还是 Unknown,判错就是权限面板上一句错的结论。测试自己的注释「只验证纯函数分支」其实描述的是「什么都没验」。 | suggest: session() 依赖进程环境,不宜在并行测试里改 env;把 XDG_SESSION_TYPE/WAYLAND_DISPLAY/DISPLAY 的判定抽成纯函数 session_from(xdg: Option<&str>, wayland: bool, display: bool) -> Session,然后逐组断言:session_from(Some("wayland"), false, true) == Wayland(证明 XDG 优先于 DISPLAY)、session_from(Some("x11"), true, false) == X11、session_from(None, true, false) == Wayland、session_from(None, false, true) == X11、session_from(None, false, false) == Unknown。

## src-tauri/src/database/connections.rs:792

test_name: default_connection_config_round_trips_new_dbx_fields | kind: 5 | severity: 重要 | why: 名字承诺「新 dbx 字段能往返」,但五条断言里四条断的都是空值/默认值:agent_java_options.is_empty()、init_script == None、!is_production、production_databases.is_empty()。default_connection_config(connections.rs:106/153/154)本来就把这四个字段初始化成 Vec::new()/None/false/Vec::new(),于是「解析器完整读回」和「解析器根本不读这四个键、靠 serde default 兜底」两种情况的结果完全一样。把 parse_core_config(connections.rs:300)改成丢弃 dbx JSON 里的 agent_java_options 和 production_databases,或者把 init_script 硬编码成 None,这条测试照绿 —— 而它是唯一以「往返」命名的守卫。唯一有效的断言是 parsed.read_only,而那个值来自测试自己显式设的 connection.read_only = true,走的是另一条(旧的)路径,与「新字段」无关。 | suggest: 给这四个字段填非默认值再往返:agent_java_options = vec!["-Xmx512m"]、init_script = Some("SET NAMES utf8mb4")、is_production = true、production_databases = vec!["prod_app"],序列化后 assert_eq! 逐字段读回原值;顺带断言 dbx JSON 里的键名(蛇形/驼峰)与 dbx_core 的 serde 契约一致,那才是「新字段」真正会漏的地方。

## src-tauri/src/notebook/html2md.rs:1283

test_name: test_truncated_html_no_panic | kind: 2 | severity: 次要 | why: 11 个畸形输入全部 `let _ = html_to_markdown(inp, false); let _ = html_to_markdown(inp, true);`,返回值一律丢弃,零断言,注释也明写「只要求不 panic」。把 html_to_markdown 改成对任何含 '<' 的输入直接 return String::new(),这条测试照绿,而用户导入半截 HTML 的笔记会变成空白笔记 —— 那正是这个函数最该守的下限(同文件 1279 行的 richtext_keeps_text_when_structure_cannot_survive 就是按「文字不能丢」写的)。当前它只能抓 index out of bounds 那一类崩溃。 | suggest: 保留 fuzz 式遍历不 panic 的部分,但至少给几个输入加实质断言:md("<p>unclosed paragraph") 含 "unclosed paragraph"、md("<ul><li>item") 含 "item" 且以列表标记开头、md("plain text only") == "plain text only"、md("<pre><code>code without close") 含 "code without close" —— 即「结构可以退化,文字不能丢」。

## src-tauri/src/database/schema.rs:128

test_name: required_uses_fallback_for_missing_scope | kind: 5 | severity: 次要 | why: 被测的 required(schema.rs:7)整体就是 `value.unwrap_or_else(|| fallback.to_string())`,两条断言 required(None, "") == "" 与 required(Some("public"), "") == "public" 测的是 Option::unwrap_or 这条标准库管道,且 fallback 两次都传空串,连「fallback 真的被用上了」都区分不出来 —— 把实现改成 `value.unwrap_or_default()`(完全忽略 fallback 参数)测试照绿。真正值得守的契约在调用方:dbx_get_object_source(schema.rs 上方)对缺失的 database/schema 该退到空串而不是报错。 | suggest: 要么删掉这条(实现是单行 stdlib 转发,没有可观测契约值得单测),要么传一个非空 fallback 让它有意义:assert_eq!(required(None, "public"), "public") 且 assert_eq!(required(Some("app".into()), "public"), "app");更有价值的是给调用侧补一条「database/schema 为 None 时仍能取到对象源、不报错」的测试。

## src-tauri/src/database/import_export.rs:379

test_name: overrides_table_export_format | kind: 4 | severity: 次要 | why: 唯一断言 assert_eq!(export_request_with_format(request(), "csv").format, "csv") 断的就是刚传进去的那个 "csv";被测函数(import_export.rs:39)整体是 `request.format = format.to_string(); request`。它只能抓「函数完全没赋值」(fixture 初始 format 是 "json",算勉强能区分),抓不到这个函数真正会出的错:顺手改坏别的字段。把实现加一行 request.file_path = String::new() 或 request.skip_count = false,测试照绿,而导出会写到空路径/多跑一次 count 查询。 | suggest: 断言「只有 format 变了」:let base = request(); let out = export_request_with_format(base.clone(), "csv"); assert_eq!(out.format, "csv"); assert_eq!(TableExportRequest { format: base.format.clone(), ..out.clone() }, base) —— 或逐字段比对 file_path/table_name/skip_count/columns 未被动过。

## src-tauri/src/notebook/rag/commands.rs:610

test_name: omitted_search_options_fall_back_to_the_defaults | kind: 6 | severity: 次要 | why: 断言的右侧就是实现自己读的那个来源。into_options(commands.rs:195-203)第一行是 `let defaults = SearchOptions::default();`,随后 positive_or(self.limit, defaults.limit);而测试写 let defaults = SearchOptions::default(); assert_eq!(options.limit, defaults.limit)。两边同源,所以把 SearchOptions::default() 的 limit 从 8 改成 0(search.rs:178)测试照绿 —— 而那意味着任何省略 limit 的搜索请求都会拿到 0、界面上表现为「搜索永远没有结果」,恰好是同文件 a_zero_upper_bound_is_treated_as_omitted 注释里点名的「最难查的一种 bug」。全仓库没有任何测试钉住 limit 的字面量 8(grep 只有实现那一处)。剩下的断言(expand_links/per_doc/rerank.is_none)同理,且与 commands.rs:650 every_upper_bound_goes_through_the_same_rule 重叠。 | suggest: 至少钉一个字面量:assert_eq!(SearchOptionsDto::default().into_options().limit, 8) 和 assert!(SearchOptionsDto::default().into_options().expand_links),让「默认值本身被改坏」也能变红;逐字段与 defaults 比对的部分交给已有的 every_upper_bound_goes_through_the_same_rule。

## src-tauri/src/notebook/rag/commands.rs:692

test_name: context_options_fall_back_to_the_defaults | kind: 6 | severity: 次要 | why: 与上一条同病,且是 commands.rs:650 every_upper_bound_goes_through_the_same_rule 的真子集 —— 后者用 ContextOptionsDto { max_tokens: Some(0), current_chars: Some(0) } 跑同一条路径并做同样的 assert_eq!(.., ContextOptions::default().x),这条只是把 current_chars 换成 None。断言右侧的 ContextOptions::default() 正是 into_options(commands.rs:218-221)自己读的来源,所以把 DEFAULT_MAX_TOKENS 或 DEFAULT_CURRENT_CHARS(context.rs:57/58)改成 0,两条测试一起绿,而 RAG 上下文会变成空串。 | suggest: 合并进 every_upper_bound_goes_through_the_same_rule(补一条 current_chars: None 的入参即可),并在 context.rs 的测试里钉住 DEFAULT_MAX_TOKENS / DEFAULT_CURRENT_CHARS 的字面量下限(例如 assert!(DEFAULT_MAX_TOKENS >= 1000)),让默认值被改坏时有东西变红。

## src-tauri/src/storage_backend/mod.rs:356

test_name: every_protocol_has_a_display_name | kind: 2 | severity: 次要 | why: protocol_display_name(mod.rs:109)是对 StorageProtocol 的穷尽 match,每个臂返回一个非空字面量 —— 「每个协议都有名字」这件事已由编译器保证(漏一个变体编译不过),测试只能抓「某个臂被人手写成 ""」这种不会真发生的情况。把 "Amazon S3" 改成 "Dropbox"、把两个臂的名字对调,测试照绿,而错误信息里会指向另一个协议、用户按提示排查错的连接。对比同文件 347 行 every_protocol_can_at_least_read 断的是具体能力位,那条是有内容的。 | suggest: 要么改成断言「名字互不相同」(let names: Vec<_> = ALL.map(display_name); 去重后长度等于 ALL.len()),这能抓复制粘贴串臂;要么对少数关键协议钉字面量(S3 → "Amazon S3"、Smb → 含 "SMB"),其余交给编译器穷尽性。

## src-tauri/src/agent_tools/dsh.rs:635

test_name: every_source_upgrade_block_explains_why_it_fell_back | kind: 2 | severity: 次要 | why: 唯一断言 assert!(!block.reason().is_empty()),而 reason()(dsh.rs:350)是穷尽 match、每臂都是非空字面量,所以断言恒真。更弱的是遍历用的是手写数组(NotAGitRepository/DirtyWorktree/NoUpstream/MissingGit/MissingPnpm)而非枚举自身,新增一个变体既不会让这条测试变红、也不在覆盖范围内 —— 测试名承诺的「每一个」并不成立。把 MissingGit 与 MissingPnpm 两臂的文案对调,用户会看到「装了 git 却提示缺 pnpm」,测试照绿。 | suggest: 断言文案与变体对得上:逐个 assert!(SourceUpgradeBlock::MissingGit.reason().contains("git"))、MissingPnpm 含 "pnpm"、DirtyWorktree 含 "uncommitted"/"dirty"、NoUpstream 含 "upstream";并加一条「五条 reason 互不相同」的去重断言替代 is_empty。

## src-tauri/src/permissions/mod.rs:852

test_name: identity_reports_a_subject_and_signature_kind | kind: 2 | severity: 次要 | why: 两条断言都只是 !x.is_empty()。imp::identity() 在 macOS 上(permissions/macos.rs:508)会去读 bundle identifier 与签名信息并按分支拼 signature 文案,测试对「拼出来的是哪一种」毫无约束 —— 把 macos.rs:511 的 match 各臂返回值互换(例如未签名报成 "apple-development"、已签名报成 "adhoc"),或让 subject 恒返回 "unknown",测试照绿,而权限面板会告诉用户一个错的签名状态(它决定 TCC 授权是否会在重签后失效这条提示)。对比 permissions/linux.rs:365 那条同名测试断了 signature == "not-applicable" 这个具体值,是有内容的。 | suggest: 把 signature 的取值域钉住:assert!(["apple-development", "developer-id", "adhoc", "unsigned", "not-applicable"].contains(&identity.signature.as_str()), "{}", identity.signature),并对可判定的分支断言 stable 标志与 signature 的对应关系(adhoc/unsigned ⇒ 不稳定 ⇒ 面板要提示重签后需重新授权)。
