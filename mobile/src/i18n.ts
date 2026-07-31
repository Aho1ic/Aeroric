/**
 * RN 端极简 i18n:系统语言检测(Intl,Hermes 内建)+ en/zh 字典 + {var} 插值。
 * 纯 TS 无 RN 依赖,vitest 直测;语言在 App 启动时确定(跟随系统,无运行时切换)。
 * 新增用户可见文案必须同时补 en 与 zh(与桌面端约定一致)。
 */

export type Language = "en" | "zh";

const zh = {
  // 通用
  "common.task": "任务",
  "common.retry": "重试",
  "common.close": "关闭",
  "common.loading": "加载中…",
  "common.opFailed": "操作失败",

  // 导航标题
  "nav.pair": "配对电脑",
  "nav.hosts": "已配对主机",
  "nav.newTask": "新建任务",

  // 通知
  "notify.inputRequired": "任务等待确认",
  "notify.done": "任务已完成",
  "notify.failed": "任务失败",
  "notify.body": "{name}",

  // 任务状态(与桌面语义一致)
  "status.todo": "待办",
  "status.pending": "准备中",
  "status.running": "运行中",
  "status.input_required": "需要输入",
  "status.detached": "后台运行",
  "status.interrupted": "已中断",
  "status.done": "已完成",
  "status.failed": "失败",
  "status.cancelled": "已取消",

  // 首页
  "home.online": "已连接",
  "home.connecting": "连接中…",
  "home.reconnecting": "连接已断开,自动重连中…",
  "home.authExpired": "授权已失效,请重新配对",
  "home.rePairAction": "重新配对",
  "home.rePair": "此主机是旧版本配对的(未启用加密),请删除后重新扫码配对",
  "home.unnamedTask": "(未命名任务)",
  "home.pairIntro":
    "在电脑端打开 设置 → 远程访问,启动远程服务并生成配对二维码,然后用手机扫码连接。",
  "home.pairNow": "扫码配对",
  "home.emptyTasks": "还没有任务。在电脑端创建任务后,这里会实时显示状态。",
  "home.hostsFallback": "主机",
  "home.expandProject": "展开项目 {name}",
  "home.collapseProject": "折叠项目 {name}",

  // 配对
  "pair.steps": "电脑端:Aeroric 设置 → 远程访问 → 启动远程服务 → 生成配对二维码",
  "pair.cameraDenied": "相机权限被拒绝,请在系统设置中开启,或在下方手动粘贴配对码。",
  "pair.cameraNeeded": "扫码需要相机权限",
  "pair.grantCamera": "授权相机",
  "pair.pairing": "配对中…",
  "pair.manualHint": "或手动粘贴配对码",
  "pair.connect": "连接",
  "pair.cannotReach": "无法连接到电脑",

  // 主机管理
  "hosts.removeTitle": "删除主机",
  "hosts.removeMessage": "确定删除「{name}」的配对吗?需要重新扫码才能再次连接。",
  "hosts.remove": "删除",
  "hosts.cancel": "取消",
  "hosts.removeFailed": "删除失败",
  "hosts.switchFailed": "切换失败",
  "hosts.active": "当前",
  "hosts.edit": "地址",
  "hosts.collapse": "收起",
  "hosts.endpointsHint":
    "每行一个地址,连接时并行竞速。可添加 Tailscale IP、frp / cloudflared 隧道或 relay 地址(ws:// 或 wss://)。",
  "hosts.saveEndpoints": "保存地址",
  "hosts.saveFailed": "保存失败",
  "hosts.endpointCount": "{first}(共 {count} 个地址)",
  "hosts.empty": "还没有配对的电脑。",
  "hosts.addNew": "+ 配对新电脑",
  "hosts.keepOneEndpoint": "至少保留一个 ws:// 或 wss:// 地址",

  // 新建任务
  "newTask.offline": "未连接到电脑,连接恢复后才能创建任务。",
  "newTask.project": "项目",
  "newTask.noProjects": "桌面端还没有项目。",
  "newTask.permission": "权限模式",
  "newTask.promptPlaceholder": "描述要让 agent 做什么…",
  "newTask.submitting": "提交中…",
  "newTask.submit": "创建并运行",
  "newTask.footnote": "任务在电脑端启动(本地模式)。worktree、附件等高级选项请在桌面端使用。",
  "newTask.sent": "已发送",
  "newTask.sentBody": "任务已提交到桌面端执行,列表稍候会显示新任务。",
  "newTask.ok": "好",
  "newTask.createFailed": "创建失败",
  "perm.ask": "每次询问",
  "perm.ask.hint": "工具调用逐一确认,最稳妥",
  "perm.auto_edit": "自动编辑",
  "perm.auto_edit.hint": "允许自动改文件,命令仍需确认",
  "perm.full_access": "完全访问",
  "perm.full_access.hint": "全自动执行,不再询问",

  // 任务详情
  "task.tab.session": "会话",
  "task.tab.terminal": "终端",
  "task.tab.changes": "变更",
  "task.actions": "操作",
  "task.markComplete": "标记完成",
  "task.cancelTask": "取消任务",
  "task.startTask": "启动任务",
  "task.resumeTask": "恢复任务",
  "task.startRequested": "已请求桌面端启动任务,稍候状态会更新。",
  "task.resumeRequested": "已请求桌面端恢复任务,稍候状态会更新。",
  "task.currentStatus": "当前状态:{label}",
  "task.loadFailed": "加载失败:{error}",

  // 会话 tab
  "session.approvalTitle": "Agent 请求授权",
  "session.waitingInput": "Agent 正在等待输入",
  "session.sending": "发送中…",
  "session.approve": "允许",
  "session.deny": "拒绝",
  "session.approvalStale": "授权请求变化后旧按钮会自动失效。",
  "session.replyHint": "请在下方发送回复,或切到「终端」标签继续操作。",
  "session.sshUnavailable": "SSH 远程任务暂不支持会话视图,请使用「终端」标签。",
  "session.notStarted":
    "会话尚未建立。任务启动后消息会显示在这里;也可切到「终端」标签查看原始输出。",
  "session.sendPlaceholder": "发送 prompt 给 agent…",
  "session.cannotSend": "任务未在运行,无法发送",
  "session.send": "发送",

  // 终端 tab
  "term.paste": "粘贴",
  "term.fit": "适配",
  "term.keyboard": "键盘",
  "term.disconnected": "连接已断开,恢复后将自动重新同步终端…",

  // 变更(diff)
  "changes.empty": "工作区没有未提交的变更。",
  "changes.unavailable.ssh": "SSH 项目的变更请在桌面端查看。",
  "changes.unavailable.wsl": "WSL 项目的变更请在桌面端查看。",
  "changes.staged": "已暂存",
  "changes.diffEmpty": "该文件没有可显示的 diff(可能是二进制文件)。",
  "changes.browseFiles": "浏览项目文件",

  // 文件浏览
  "files.title": "文件",
  "files.truncated": "文件过大,仅显示前 {kb}KB",
  "files.empty": "空目录",
} as const;

export type MessageKey = keyof typeof zh;

const en: Record<MessageKey, string> = {
  "common.task": "Task",
  "common.retry": "Retry",
  "common.close": "Close",
  "common.loading": "Loading…",
  "common.opFailed": "Operation failed",

  "nav.pair": "Pair a computer",
  "nav.hosts": "Paired hosts",
  "nav.newTask": "New task",

  "notify.inputRequired": "Task needs your confirmation",
  "notify.done": "Task completed",
  "notify.failed": "Task failed",
  "notify.body": "{name}",

  "status.todo": "Todo",
  "status.pending": "Pending",
  "status.running": "Running",
  "status.input_required": "Needs input",
  "status.detached": "Background",
  "status.interrupted": "Interrupted",
  "status.done": "Done",
  "status.failed": "Failed",
  "status.cancelled": "Cancelled",

  "home.online": "Connected",
  "home.connecting": "Connecting…",
  "home.reconnecting": "Connection lost — reconnecting…",
  "home.authExpired": "Authorization expired — pair again",
  "home.rePairAction": "Pair again",
  "home.rePair":
    "This host was paired with an old app version (no encryption). Remove it and pair again.",
  "home.unnamedTask": "(untitled task)",
  "home.pairIntro":
    "On your computer, open Settings → Remote Access, start the server and generate the pairing QR code, then scan it here.",
  "home.pairNow": "Scan to pair",
  "home.emptyTasks": "No tasks yet. Create one on the desktop and its status will appear here live.",
  "home.hostsFallback": "Hosts",
  "home.expandProject": "Expand project {name}",
  "home.collapseProject": "Collapse project {name}",

  "pair.steps":
    "On your computer: Aeroric Settings → Remote Access → start the server → generate the pairing QR code",
  "pair.cameraDenied":
    "Camera permission denied. Enable it in system settings, or paste the pairing code below.",
  "pair.cameraNeeded": "Camera permission is required to scan",
  "pair.grantCamera": "Grant camera access",
  "pair.pairing": "Pairing…",
  "pair.manualHint": "Or paste the pairing code manually",
  "pair.connect": "Connect",
  "pair.cannotReach": "Cannot reach the computer",

  "hosts.removeTitle": "Remove host",
  "hosts.removeMessage":
    "Remove the pairing with “{name}”? You will need to scan again to reconnect.",
  "hosts.remove": "Remove",
  "hosts.cancel": "Cancel",
  "hosts.removeFailed": "Remove failed",
  "hosts.switchFailed": "Switch failed",
  "hosts.active": "Active",
  "hosts.edit": "Endpoints",
  "hosts.collapse": "Collapse",
  "hosts.endpointsHint":
    "One address per line; all are dialed in parallel. Add a Tailscale IP, frp / cloudflared tunnel or relay address (ws:// or wss://).",
  "hosts.saveEndpoints": "Save endpoints",
  "hosts.saveFailed": "Save failed",
  "hosts.endpointCount": "{first} (+{count} endpoints)",
  "hosts.empty": "No paired computers yet.",
  "hosts.addNew": "+ Pair a new computer",
  "hosts.keepOneEndpoint": "Keep at least one ws:// or wss:// address",

  "newTask.offline": "Not connected — you can create tasks once the connection is back.",
  "newTask.project": "Project",
  "newTask.noProjects": "No projects on the desktop yet.",
  "newTask.permission": "Permission mode",
  "newTask.promptPlaceholder": "Describe what the agent should do…",
  "newTask.submitting": "Submitting…",
  "newTask.submit": "Create & run",
  "newTask.footnote":
    "The task starts on the desktop (local mode). Use the desktop app for worktrees, attachments and other advanced options.",
  "newTask.sent": "Sent",
  "newTask.sentBody": "Task submitted to the desktop — it will appear in the list shortly.",
  "newTask.ok": "OK",
  "newTask.createFailed": "Create failed",
  "perm.ask": "Ask every time",
  "perm.ask.hint": "Confirm each tool call — safest",
  "perm.auto_edit": "Auto edit",
  "perm.auto_edit.hint": "Edits are automatic; commands still ask",
  "perm.full_access": "Full access",
  "perm.full_access.hint": "Fully automatic, no questions",

  "task.tab.session": "Session",
  "task.tab.terminal": "Terminal",
  "task.tab.changes": "Changes",
  "task.actions": "Actions",
  "task.markComplete": "Mark complete",
  "task.cancelTask": "Cancel task",
  "task.startTask": "Start task",
  "task.resumeTask": "Resume task",
  "task.startRequested": "Requested the desktop to start the task — status will update shortly.",
  "task.resumeRequested": "Requested the desktop to resume the task — status will update shortly.",
  "task.currentStatus": "Status: {label}",
  "task.loadFailed": "Load failed: {error}",

  "session.approvalTitle": "Agent requests approval",
  "session.waitingInput": "Agent is waiting for input",
  "session.sending": "Sending…",
  "session.approve": "Approve",
  "session.deny": "Deny",
  "session.approvalStale": "These buttons expire automatically when the approval request changes.",
  "session.replyHint": "Reply below, or switch to the Terminal tab to continue.",
  "session.sshUnavailable": "Session view is unavailable for SSH tasks — use the Terminal tab.",
  "session.notStarted":
    "No session yet. Messages appear here once the task runs; the Terminal tab shows raw output.",
  "session.sendPlaceholder": "Send a prompt to the agent…",
  "session.cannotSend": "Task is not running — cannot send",
  "session.send": "Send",

  "term.paste": "Paste",
  "term.fit": "Fit",
  "term.keyboard": "Keyboard",
  "term.disconnected": "Connection lost — the terminal will resync automatically…",

  "changes.empty": "No uncommitted changes in the working tree.",
  "changes.unavailable.ssh": "View changes for SSH projects on the desktop.",
  "changes.unavailable.wsl": "View changes for WSL projects on the desktop.",
  "changes.staged": "Staged",
  "changes.diffEmpty": "No diff to show for this file (it may be binary).",
  "changes.browseFiles": "Browse project files",

  "files.title": "Files",
  "files.truncated": "File too large — showing the first {kb}KB",
  "files.empty": "Empty directory",
};

const dictionaries: Record<Language, Record<MessageKey, string>> = { en, zh };

function detectLanguage(): Language {
  try {
    const locale = Intl.DateTimeFormat().resolvedOptions().locale ?? "";
    return /^zh(\b|-)/i.test(locale) ? "zh" : "en";
  } catch {
    return "en";
  }
}

let currentLanguage: Language = detectLanguage();

export function getLanguage(): Language {
  return currentLanguage;
}

/** 测试与未来的手动切换入口。 */
export function setLanguage(language: Language): void {
  currentLanguage = language;
}

export function t(key: MessageKey, vars?: Record<string, string | number>): string {
  const template = dictionaries[currentLanguage][key] ?? zh[key] ?? key;
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}
