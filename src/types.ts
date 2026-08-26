export interface Project {
  id: string;
  name: string;
  path: string;
  location?: ProjectLocation;
  branch?: string;
  lastOpenedAt: number;
  orderIndex?: number;
  /** Optional project group name. Projects without a group stay in the ungrouped section. */
  group?: string;
  /** 为 true 时不在左侧常驻竖条显示，仅可从首页或「展开全部」抽屉访问。缺省=常驻。 */
  hiddenFromRail?: boolean;
  /** 置顶：在各自分组内排最前，分组折叠时仍露出。桌面与手机共享同一份状态。 */
  pinned?: boolean;
}

export type ProjectLocation =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connectionId: string; remotePath: string }
  | { kind: "wsl"; distribution: string; linuxPath: string };

export type LocalTarget = { kind: "local"; path: string };
export type SshTarget = {
  kind: "ssh";
  connection: SshConnection;
  projectPath: string;
};
export type WslTarget = {
  kind: "wsl";
  distribution: string;
  projectPath: string;
};
export type ProjectTarget = LocalTarget | SshTarget | WslTarget;
export type RemoteProjectTarget = SshTarget | WslTarget;

export function resolveProjectLocation(project: Project): ProjectLocation {
  return project.location ?? { kind: "local", path: project.path };
}

export function isRemoteProject(project: Project): boolean {
  return resolveProjectLocation(project).kind !== "local";
}

export function sshProjectPath(connectionId: string, remotePath: string): string {
  const normalizedRemotePath = remotePath.startsWith("/") ? remotePath : `/${remotePath}`;
  return `ssh://${connectionId}${normalizedRemotePath}`;
}

export function wslProjectPath(distribution: string, linuxPath: string): string {
  const normalizedLinuxPath = linuxPath.startsWith("/") ? linuxPath : `/${linuxPath}`;
  return `wsl://${encodeURIComponent(distribution)}${normalizedLinuxPath}`;
}

export interface WslDistribution {
  name: string;
  state: string;
  version?: number;
  isDefault: boolean;
}

export interface WslStatus {
  supported: boolean;
  installed: boolean;
  distributionCount: number;
  defaultDistribution?: string;
  error?: string;
}

export interface WslDistributionProbe {
  distribution: string;
  state: string;
  version?: number;
  home: string;
  shell: string;
  user: string;
  claudePath?: string;
  codexPath?: string;
}

export interface WslEnvironment {
  distribution: string;
  home: string;
  shell: string;
  path: string;
  variables: Record<string, string>;
  sensitiveNames?: string[];
}

export interface WslDistributionSettings {
  shellOverride?: string;
  agentPaths: Record<string, string>;
  agentConfigPaths: Record<string, string>;
}

export interface WslSettings {
  defaultDistribution?: string;
  distributions: Record<string, WslDistributionSettings>;
}

export interface SshConnection {
  id: string;
  name: string;
  group?: string;
  host: string;
  port: number;
  username: string;
  identityFile?: string;
  password?: string;
  remotePath?: string;
  autoSudoWithPassword?: boolean;
  /** 勾选后这条连接每次都经「设置 > 代理」里配置的全局代理建立。 */
  useProxy?: boolean;
  createdAt: number;
  lastConnectedAt?: number;
}

/** 服务端提供的一把 host key。`knownHostsLine` 只回传给后端落盘,不展示。 */
export interface SshHostKey {
  keyType: string;
  fingerprint: string;
  knownHostsLine: string;
}

/** `check_ssh_host_key` 的结果,决定连接前是否需要用户确认指纹。 */
export type SshHostKeyStatus =
  | { state: "trusted" }
  | { state: "unknown"; target: string; keys: SshHostKey[] }
  | { state: "unreachable"; target: string };

export interface CondaEnvironment {
  name: string;
  path: string;
  pythonPath: string;
}

export interface DockerImageSummary {
  id: string;
  repository: string;
  tag: string;
  digest: string;
  createdSince: string;
  size: string;
}

export interface DockerContainerSummary {
  id: string;
  image: string;
  names: string;
  state: string;
  status: string;
  ports: string;
  createdAt: string;
}

export interface DockerResources {
  images: DockerImageSummary[];
  containers: DockerContainerSummary[];
}

export type {
  AeroricDbConnectionConfig,
  DbCellValue,
  DbColumn,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbForeignKey,
  DbIndex,
  DbObject,
  DbQueryResult,
  DbRow,
  DbSchema,
  DbSshConnection,
  DbTrigger,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxDatabaseType,
  DbxObjectInfo,
  DbxListObjectsOptions,
  DbxObjectSource,
  DbxObjectSourceKind,
  DbxQueryResult,
  DbxTransferProgress,
  DbxTransferRequest,
  ExecuteMultiRequest,
  ExecuteQueryRequest,
  DataGridCopyInsertStatementOptions,
  DataGridCopyUpdateStatementOptions,
  DataGridContextFilterConditionOptions,
  DataGridContextFilterMode,
  DataGridColumnInfo,
  DataGridSaveStatementOptions,
  DataGridTableMeta,
  DatabaseDriverCapabilities,
  DatabaseDriverManifest,
  DatabaseDriverManifestEntry,
  DatabaseObjectType,
  DatabaseExportRequest,
  DatabaseSearchColumn,
  DatabaseSearchSql,
  DatabaseSearchSqlOptions,
  DriverRuntimeMode,
  DriverSupportLevel,
  ExecuteSqlFileRequest,
  EditableStructureColumn,
  GridSaveRequest,
  MongoDeleteDocumentsRequest,
  MongoDocumentResult,
  MongoFindDocumentsRequest,
  MongoInsertDocumentRequest,
  MongoUpdateDocumentRequest,
  RedisDatabaseInfo,
  RedisCollectionPage,
  RedisKeyInfo,
  RedisKeyRequest,
  RedisScanKeysRequest,
  RedisScanResult,
  RedisSetTtlRequest,
  RedisSetValueRequest,
  RedisValue,
  SearchResultWhereOptions,
  SqlPreviewResponse,
  TableExportRequest,
  TableImportColumnMapping,
  TableImportMode,
  TableImportPreview,
  TableImportRequest,
  TableImportSummary,
  TableDataRequest,
  TableDataResponse,
  TableChildObjectType,
} from "./types/database";

export type BuiltInAgentType = "claude" | "claude_gpt55" | "codex" | "dsh";
export type AgentType = BuiltInAgentType | (string & {});
/** 协议族:决定启动参数、会话格式与配置文件形态;codexLike 布尔为其派生。 */
export type ProtocolFamily = "claude" | "codex" | "dsh";
export type ThemeMode = "system" | "dark" | "light" | "eyecare";
export type ThemeVariant = "dark" | "light" | "eyecare";
export type PermissionMode = "ask" | "auto_edit" | "full_access";
export type TaskDisplayWindow = 3 | 7 | 15 | 30 | "all";

export const TASK_DISPLAY_WINDOW_VALUES = [3, 7, 15, 30, "all"] as const;
export const DEFAULT_TASK_DISPLAY_WINDOW: TaskDisplayWindow = 3;

export function normalizeTaskDisplayWindow(value: unknown): TaskDisplayWindow {
  if (value === "all") return "all";
  const parsed = typeof value === "number" ? value : Number(value);
  return TASK_DISPLAY_WINDOW_VALUES.includes(parsed as TaskDisplayWindow)
    ? (parsed as TaskDisplayWindow)
    : DEFAULT_TASK_DISPLAY_WINDOW;
}

export type TerminalFontSize = number;

export const TERMINAL_FONT_SIZE_MIN = 10;
export const TERMINAL_FONT_SIZE_MAX = 20;
export const TERMINAL_FONT_SIZE_STEP = 1;
export const DEFAULT_TERMINAL_FONT_SIZE: TerminalFontSize = 11;

export function clampTerminalFontSize(value: number): TerminalFontSize {
  if (!Number.isFinite(value)) return DEFAULT_TERMINAL_FONT_SIZE;
  const snapped = Math.round(value / TERMINAL_FONT_SIZE_STEP) * TERMINAL_FONT_SIZE_STEP;
  return Math.min(TERMINAL_FONT_SIZE_MAX, Math.max(TERMINAL_FONT_SIZE_MIN, snapped));
}

export type FontFamily = string;
export const DEFAULT_UI_FONT_BY_PLATFORM: Record<"windows" | "macos" | "linux", FontFamily> = {
  windows: '"Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  macos: '"SF Pro Display", "IBM Plex Sans", "PingFang SC", "Noto Sans SC", sans-serif',
  linux:
    '"Inter", "Noto Sans", "Noto Sans CJK SC", "WenQuanYi Micro Hei", "DejaVu Sans", sans-serif',
};
export const DEFAULT_UI_FONT: FontFamily = DEFAULT_UI_FONT_BY_PLATFORM.macos;
// 旧默认值（缺 CJK 字形，会导致终端中文乱码/错位）。用于把老用户 localStorage 里
// 存下的旧值自动迁移到新的含 CJK fallback 的字体链，见 App.tsx 的迁移逻辑。
export const LEGACY_DEFAULT_MONO_FONTS: readonly FontFamily[] = [
  '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
];
// 西文等宽字体在前用于测量 cell 宽度，末尾补 CJK 等宽/全角字体，确保 Claude
// 输出的中文有字形可回退，避免 WebGL renderer 下的乱码与宽度错位。
export const DEFAULT_MONO_FONT_BY_PLATFORM: Record<"windows" | "macos" | "linux", FontFamily> = {
  windows:
    '"Cascadia Mono", "Cascadia Code", "Sarasa Mono SC", Consolas, "Microsoft YaHei", monospace',
  macos:
    '"JetBrains Mono", "Fira Code", "Sarasa Mono SC", "Maple Mono NF CN", ui-monospace, "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", "Noto Sans SC", monospace',
  linux:
    '"JetBrains Mono", "Noto Sans Mono CJK SC", "Sarasa Mono SC", "DejaVu Sans Mono", "Noto Sans Mono", monospace',
};
export const DEFAULT_MONO_FONT: FontFamily = DEFAULT_MONO_FONT_BY_PLATFORM.macos;

export type TaskStatus =
  | "todo"
  | "pending"
  | "running"
  | "input_required"
  | "detached"
  | "interrupted"
  | "done"
  | "failed"
  | "cancelled";

export interface Task {
  id: string;
  projectId: string;
  name?: string;
  prompt: string;
  agent: AgentType;
  selectedModel?: string;
  dshAgentPreset?: string;
  reasoningEffort?: string;
  speed?: string;
  permissionMode: PermissionMode;
  status: TaskStatus;
  createdAt: number;
  attentionRequestedAt?: number;
  starred?: boolean;
  failureReason?: string;
  codexSessionId?: string;
  codexSessionPath?: string;
  claudeSessionId?: string;
  claudeSessionPath?: string;
  dshSessionId?: string;
  dshSessionPath?: string;
  dshWorkspaceId?: string;
  dshPromptMode?: string;
  /** 实际创建当前会话的 Agent；切换配置失败后仍用于定位原会话 home。 */
  sessionAgent?: AgentType;
  /** 实际会话所属协议族；避免切换后的 task.agent 误导 resume/session 解析。 */
  sessionCodexLike?: boolean;
  /** 三值协议族;读取优先于 sessionCodexLike(旧任务缺省时由其推导)。 */
  sessionFamily?: ProtocolFamily;
  worktreePath?: string;
  worktreeBranch?: string;
  baseBranch?: string;
  /** worktree 已被合并或丢弃后置 true：保留分支/路径用于审计，但禁用 resume / 合并 / 丢弃 */
  worktreeDiscarded?: boolean;
  /** 任务完成时计算的相对 baseBranch merge-base 的累计新增行数（仅 worktree 任务） */
  additions?: number;
  /** 任务完成时计算的相对 baseBranch merge-base 的累计删除行数（仅 worktree 任务） */
  deletions?: number;
}

export const PERM_LABELS: Record<PermissionMode, string> = {
  ask: "Ask Permission",
  auto_edit: "Auto-edit",
  full_access: "Full Access",
};

export function permissionModeLabel(
  mode: PermissionMode,
  agent?: AgentType,
  askLabel = PERM_LABELS.ask,
): string {
  if ((agent === "codex" || agent === "claude_gpt55") && mode === "auto_edit") {
    return "Auto Mode";
  }
  if (mode === "ask") return askLabel;
  return PERM_LABELS[mode];
}

export const STATUS_LABEL: Record<TaskStatus, string> = {
  todo: "Todo",
  pending: "Pending",
  running: "Running...",
  input_required: "Needs confirmation",
  detached: "Terminal disconnected",
  interrupted: "Interrupted",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

export function isActiveTaskStatus(status: TaskStatus): boolean {
  return (
    status === "pending" ||
    status === "running" ||
    status === "input_required" ||
    status === "detached"
  );
}

// ── System permissions ───────────────────────────────────────────────────────

/** "未授权"合并了"被拒绝"与"从未询问":对用户来说都是同一个动作。 */
export type SystemPermissionStatus = "granted" | "notGranted" | "unknown";

export interface SystemPermission {
  /** 稳定标识,同时是 `permissions.item.<id>.*` 文案的 key 后缀。 */
  id: string;
  status: SystemPermissionStatus;
  canRequestInApp: boolean;
  canOpenSettings: boolean;
  /** 授权后必须重启应用才生效。 */
  needsRestart: boolean;
  /** 检测本身会触发系统询问,所以列表里默认显示为未知。 */
  probePrompts: boolean;
  /** 检测失败的原因(后端英文原文),仅作补充说明。 */
  detail?: string;
}

export interface SystemPermissionReport {
  platform: string;
  /** 本平台是否有逐项授权模型;false 时 `permissions` 为空。 */
  supported: boolean;
  permissions: SystemPermission[];
}

export interface SystemPermissionGrantAllResult {
  report: SystemPermissionReport;
  requested: string[];
  /** 只能到系统设置里手工勾选、且当前仍未授权的权限。 */
  manual: string[];
}

// ── Startup diagnostics ──────────────────────────────────────────────────────

/**
 * 一条启动期降级记录。
 *
 * 后端启动时若 `~/.aeroric` 不可写,会退到临时目录甚至内存库而不是崩溃退出;
 * 这条记录就是那次降级的原因,前端启动后查一次并告知用户——否则「数据下次启动
 * 就没了」会静默发生。
 */
export interface StartupDegradation {
  /** 组件标识,如 `dbx-state`、`local-router`。 */
  component: string;
  reason: string;
  /** 实际退到了哪儿(临时目录路径,或 `:memory:`)。 */
  fallback: string;
}

// ── Notifications ────────────────────────────────────────────────────────────

export interface NotificationItem {
  id: string;
  level: "info" | "warning" | "error" | string;
  title: string;
  body: string;
  bodyZh: string | null;
  url: string | null;
  createdAt: string;
  isRead: boolean;
  releaseTag?: string | null;
  newerThanCurrent?: boolean;
  updateInstallSupported?: boolean;
}

export interface NotificationResult {
  notifications: NotificationItem[];
  unreadCount: number;
}

export interface ReleaseInstallResult {
  tagName: string;
  assetName: string;
  installedAppPath: string;
  restarted: boolean;
}

export interface ReleaseUpdatePrepareResult {
  tagName: string;
  assetName: string;
  installerPath: string;
  readyToRestart: boolean;
  checksumVerified: boolean;
  helperStatus: "ready" | "running" | "failed" | string;
  error: string | null;
}

export interface TextSearchMatch {
  path: string;
  name: string;
  line: number;
  column: number;
  lineText: string;
  matchText: string;
}

export interface TextSearchOptions {
  caseSensitive?: boolean;
  regex?: boolean;
  wholeWord?: boolean;
  includeGlob?: string | null;
  excludeGlob?: string | null;
  limit?: number;
}

export interface TextSearchFileGroup {
  path: string;
  name: string;
  matches: TextSearchMatch[];
}

export interface TextReplacement {
  path: string;
  start: number;
  end: number;
  matchText: string;
  replacementText: string;
}

export interface ReplacePreviewMatch extends TextSearchMatch {
  replacementText: string;
  start: number;
  end: number;
}

export interface ReplacePreviewFile {
  path: string;
  name: string;
  matches: ReplacePreviewMatch[];
}

export interface ReplacePreview {
  query: string;
  replacement: string;
  files: ReplacePreviewFile[];
  totalMatches: number;
  truncated: boolean;
}

export interface ReplaceSummary {
  filesChanged: number;
  replacementsApplied: number;
  replacementsSkipped: number;
}

export interface LspPosition {
  line: number;
  character: number;
}

export interface LspRange {
  start: LspPosition;
  end: LspPosition;
}

export interface LspSymbol {
  name: string;
  kind: number;
  detail?: string | null;
  containerName?: string | null;
  uri: string;
  path: string;
  range: LspRange;
  selectionRange: LspRange;
}

export interface LspInlayHint {
  label: string;
  position: LspPosition;
  kind?: number | null;
  tooltip?: string | null;
  paddingLeft: boolean;
  paddingRight: boolean;
}

export type DiagnosticSeverity = "error" | "warning" | "info";

export interface DiagnosticItem {
  source: string;
  severity: DiagnosticSeverity;
  message: string;
  file: string;
  line: number;
  column: number;
  code?: string | null;
}

export interface DiagnosticRunResult {
  profile: string;
  diagnostics: DiagnosticItem[];
  rawOutput: string;
}

export interface GitBlameLine {
  line: number;
  commit: string;
  shortCommit: string;
  author: string;
  authorTime: number;
  summary: string;
  content: string;
}

export interface GitBlameResult {
  filePath: string;
  lines: GitBlameLine[];
}

export interface LocalHistoryEntry {
  id: string;
  filePath: string;
  relativePath: string;
  createdAtMs: number;
  size: number;
}

export interface LocalHistorySnapshot {
  entry: LocalHistoryEntry;
  content: string;
}

export interface GitBranchGraphCommit {
  hash: string;
  shortHash: string;
  parents: string[];
  refs: string[];
  subject: string;
  author: string;
  relativeTime: string;
}

export interface GitBranchGraphResult {
  commits: GitBranchGraphCommit[];
  truncated: boolean;
}

export interface GitStashEntry {
  index: number;
  name: string;
  commit: string;
  date: string;
  message: string;
}

export interface GitStashDiff {
  stashRef: string;
  diff: string;
  truncated: boolean;
}

export interface GitConflictFile {
  path: string;
}

export interface GitConflictHunk {
  index: number;
  ours: string;
  base?: string | null;
  theirs: string;
}

export interface GitConflictPreview {
  filePath: string;
  hunks: GitConflictHunk[];
}

export type GitConflictResolution = "ours" | "theirs" | "both";

export interface ListeningPort {
  port: number;
  address: string;
  protocol: string;
  pid: number;
  processName: string;
  url: string;
  projectContext: "project" | "other" | "unknown";
}

export type DebugConfigType = "node" | "python";
export type DebugRequestType = "launch" | "attach";

export interface DebugBreakpoint {
  file: string;
  line: number;
  column: number;
  condition?: string | null;
  logMessage?: string | null;
}

export interface DebugConfig {
  id: string;
  name: string;
  type: DebugConfigType;
  request?: DebugRequestType;
  program: string;
  cwd: string;
  attachHost?: string;
  attachPort?: number | null;
  args: string[];
  env: Record<string, string>;
  breakpoints: DebugBreakpoint[];
}

export interface DebugConfigDocument {
  version: 1;
  configs: DebugConfig[];
}

export type DebugSessionStatus =
  | "starting"
  | "running"
  | "paused"
  | "exited"
  | "failed"
  | "stopped";

export interface DebugCallFrame {
  functionName: string;
  file: string;
  line: number;
  column: number;
  frameId?: string | null;
}

export interface DebugVariable {
  name: string;
  value: string;
  typeName?: string | null;
  objectId?: string | null;
  hasChildren?: boolean;
}

export interface DebugVariableScope {
  name: string;
  variables: DebugVariable[];
}

export interface DebugSessionSnapshot {
  debugId: string;
  configId: string;
  name: string;
  program: string;
  cwd: string;
  status: DebugSessionStatus;
  output: string;
  pausedReason?: string | null;
  callStack: DebugCallFrame[];
  scopes: DebugVariableScope[];
  exitCode?: number | null;
  startedAt: number;
  finishedAt?: number | null;
}

export interface DebugEvaluateResult {
  expression: string;
  result: string;
  typeName?: string | null;
  objectId?: string | null;
  hasChildren?: boolean;
}

export type RunConfigType = "shell" | "debug";
export type RunDebugConfigType = DebugConfigType;

export interface ShellRunConfig {
  id: string;
  name: string;
  type: "shell";
  command: string;
  cwd: string;
  env: Record<string, string>;
}

export interface DebugRunConfig {
  id: string;
  name: string;
  type: "debug";
  debugType: RunDebugConfigType;
  program: string;
  cwd: string;
  args: string[];
  env: Record<string, string>;
  breakpoints: DebugBreakpoint[];
}

export type RunConfig = ShellRunConfig | DebugRunConfig;

export interface RunConfigDocument {
  version: 1;
  configs: RunConfig[];
}

export type RunProcessStatus = "running" | "exited" | "failed" | "stopped";

export interface RunProcessSnapshot {
  runId: string;
  configId: string;
  name: string;
  command: string;
  cwd: string;
  status: RunProcessStatus;
  output: string;
  exitCode?: number | null;
  startedAt: number;
  finishedAt?: number | null;
}

export type TestRunStatus = "passed" | "failed" | "error";

export interface TestProfile {
  id: string;
  label: string;
  command: string;
}

export interface TestCase {
  profile: string;
  name: string;
  file: string;
  line: number;
  column: number;
  status: TestRunStatus;
  durationMs?: number | null;
}

export interface TestFailure {
  profile: string;
  name: string;
  file: string;
  line: number;
  column: number;
  message: string;
}

export interface TestRunResult {
  profile: string;
  status: TestRunStatus;
  total: number;
  passed: number;
  failed: number;
  tests: TestCase[];
  failures: TestFailure[];
  coverage?: TestCoverageSummary | null;
  rawOutput: string;
}

export interface TestCoverageMetric {
  covered: number;
  total: number;
  percent: number;
}

export interface TestCoverageSummary {
  lines: TestCoverageMetric;
  functions: TestCoverageMetric;
  branches: TestCoverageMetric;
  files?: TestCoverageFile[];
}

export interface TestCoverageFile {
  file: string;
  lines: TestCoverageLine[];
}

export interface TestCoverageLine {
  line: number;
  hits: number;
}

export interface TestRunTarget {
  filePath?: string | null;
  testName?: string | null;
}

export interface TestDiscoveryResult {
  profiles: TestProfile[];
}

export interface FormatFileResult {
  filePath: string;
  command: string;
}

export interface UsageWindow {
  usedPercent: number;
  remainingPercent: number;
  resetAt?: number | null;
}

export interface ClaudeUsageData {
  fiveHour?: UsageWindow | null;
  sevenDay?: UsageWindow | null;
}

export interface CodexUsageData {
  email?: string | null;
  planType?: string | null;
  primary?: UsageWindow | null;
  secondary?: UsageWindow | null;
}

export type UsageSource<T> =
  | { status: "available"; data: T }
  | { status: "unavailable"; reason: string };

export interface UsageSnapshot {
  claude: UsageSource<ClaudeUsageData>;
  codex: UsageSource<CodexUsageData>;
  fetchedAt: number;
}

export type UsageStatisticsAgent = "all" | "codex" | "claude" | "dsh";
export type UsageStatisticsRange = 1 | 7 | 14 | 30;

export interface UsageStatisticsTotals {
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  cacheHitRate: number;
  requestCount: number;
  totalCost: number;
  /** 命中公开价目表的请求数。 */
  pricedRequestCount: number;
  /** 无公开价目、按同档模型推算单价的请求数（成本仍计入 totalCost）。 */
  estimatedRequestCount: number;
}

export interface UsageStatisticsDay extends UsageStatisticsTotals {
  date: string;
  hour?: number;
}

export interface UsageStatistics {
  rangeDays: UsageStatisticsRange;
  from: string;
  to: string;
  agent: UsageStatisticsAgent;
  updatedAt: number;
  totals: UsageStatisticsTotals;
  series: UsageStatisticsDay[];
  breakdown: {
    codex: UsageStatisticsTotals;
    claude: UsageStatisticsTotals;
    dsh?: UsageStatisticsTotals;
  };
}

// ── Skill Hub ────────────────────────────────────────────────────────────────

export interface SkillHubConfig {
  hubProjectId?: string;
  hubPath?: string;
  createdAt?: number;
}

export interface Skill {
  /** SKILL 目录名（权威标识） */
  name: string;
  /** frontmatter 的 name 字段，可与目录名不同 */
  displayName?: string;
  /** 解析后的 description，可能包含换行 */
  description?: string;
  /** skill 目录绝对路径 */
  path: string;
  /** frontmatter 解析失败时的错误说明 */
  hasError?: string;
}

export interface PromptSkill {
  /** CLI slash command 名称，不含前导 `/`。 */
  name: string;
  description?: string;
  path: string;
}

export type SkillInstallationHealth = "ok" | "broken" | "diverged";

export interface SkillInstallation {
  skillName: string;
  projectId: string;
  agent: AgentType;
  installedAt: number;
  linkPath: string;
  targetPath: string;
  health?: SkillInstallationHealth;
}

export type SkillInstallStrategy = "detect" | "skip" | "overwrite" | "cancel";

export interface SkillConflictInfo {
  existingKind: "directory" | "file" | "symlink";
  existingTarget?: string;
  linkPath: string;
}

export interface SkillInstallResult {
  ok: boolean;
  conflict?: SkillConflictInfo;
  alreadyInstalled?: boolean;
  skipped?: boolean;
  cancelled?: boolean;
  installation?: SkillInstallation;
}

export interface SkillDeleteResult {
  ok: boolean;
  removedLinks: number;
}

export interface SetSkillHubResult {
  config: SkillHubConfig;
  project: Project;
  createdNewProject: boolean;
  /** 后端写入后的权威 projects 列表 */
  projects: Project[];
}

export type MarketplaceSort = "downloads" | "stars" | "installs" | "updated" | "published";

export type MarketplaceCategory =
  | "all"
  | "agents"
  | "integrations"
  | "automation"
  | "operations"
  | "security"
  | "research"
  | "development"
  | "finance"
  | "lifestyle"
  | "productivity"
  | "other"
  | "communication"
  | "creative"
  | "knowledge";

export type MarketplaceInstallStatus =
  | "available"
  | "installing"
  | "installed"
  | "update"
  | "conflict";

export interface MarketplaceSkill {
  id: string;
  source: string;
  skillId: string;
  name: string;
  publisher: string;
  publisherAvatar?: string;
  repositoryUrl?: string;
  skillPath?: string;
  description?: string;
  latestVersion: string;
  latestRef: string;
  categories: MarketplaceCategory[];
  downloads7d: number;
  totalInstalls: number;
  stars: number;
  publishedAt?: string;
  updatedAt?: string;
  installStatus: MarketplaceInstallStatus;
  isOfficial: boolean;
}

export interface MarketplacePage {
  items: MarketplaceSkill[];
  total: number;
  page: number;
  pageSize: number;
  hasMore: boolean;
  stale: boolean;
  warning?: string;
}

export interface MarketplaceInstallRecord {
  source: string;
  skillId: string;
  skillName: string;
  version: string;
  gitRef: string;
  installedAt: number;
  targetPath: string;
}

export interface DshSessionHistory {
  events: unknown[];
  hasMore: boolean;
  projections?: {
    asOfSeq: number;
    values: Record<string, unknown>;
  };
}

export interface DshModelCatalogFailure {
  id: string;
  name: string;
  message: string;
}

export interface DshModelInfo {
  id: string;
  name?: string;
  reasoning?: {
    efforts: Array<{ id: string; name: string; description?: string }>;
    defaultEffort?: string;
  };
}

export interface DshModelGroup {
  id: string;
  name: string;
  models: DshModelInfo[];
}

export interface DshGlobalModels {
  groups: DshModelGroup[];
  failures: DshModelCatalogFailure[];
}

export interface DshDiscoveredModel {
  id: string;
  name?: string;
  contextWindow?: number;
  maxTokens?: number;
}

export interface DshHostInfo {
  version?: string;
  cwd: string;
  provider?: string;
  model?: string;
  attachedSessions?: number;
  canOpenPath: boolean;
}

export interface DshDirectoryEntry {
  name: string;
  path: string;
  hidden?: boolean;
}

export interface DshDirectoryListing {
  path: string;
  home: string;
  crumbs: DshDirectoryEntry[];
  entries: DshDirectoryEntry[];
  truncated: boolean;
}

export interface DshPresetReadResult {
  content: string;
  preset: string;
  trust?: "system" | "user" | string;
  name?: string;
  description?: string;
}

export interface DshPresetList {
  presets: Array<{
    id: string;
    trust: "system" | "user" | string;
    isDefault: boolean;
    name?: string;
    description?: string;
    broken?: string;
  }>;
  authorable: boolean;
  hasDocument: boolean;
}

// ── DeepSeek Harness live session projection / jobs / queue types ────────────
// Mirror the session/projection, session/jobs, session/queue push frames the
// dsh web subprocess emits over events.mux. The backend forwards each frame
// verbatim as a Tauri event (dispatch_mux_frame in dsh_webui.rs), so these
// are the wire shapes the frontend consumes.

/** `session/projection` push frame: one projection unit's finished value. */
export interface DshProjectionFrame {
  type: "session/projection";
  sessionId: string;
  /** Projection unit key: title | goal | plan | todo | permissions | subagents | tokenMeter | imageLimits … */
  key: string;
  /** The unit's schema-validated view output (shape varies per key). */
  value: unknown;
  /** Higher-seq-wins watermark at emission. */
  seq: number;
}

/** TodoWrite projection view ( shapes the `todo` projection unit emits). */
export interface DshTodoItem {
  content: string;
  status: "pending" | "in_progress" | "completed";
  activeForm?: string;
}

export interface DshTodoProjection {
  items: DshTodoItem[];
}

/** Goal projection view (the `goal` unit). */
export interface DshGoalProjection {
  goal: {
    id: string;
    /** Current Harness uses `objective`; older builds exposed `title`. */
    objective?: string;
    title?: string;
    revision: number;
    phase: "active" | "paused" | "blocked" | "complete";
    blockedReason?: { code: string; message: string };
    maxGoalRounds?: number;
  } | null;
  roundsStarted?: number;
  createdAt?: number;
  updatedAt?: number;
}

/** Plan-mode projection view (the `plan` unit). */
export interface DshPlanProjection {
  active: boolean;
}

/** `session/jobs` push frame: live background jobs for a session. */
export interface DshJobView {
  id: string;
  kind: string;
  status: "running" | "stopping" | "completed" | "killed" | "failed";
  label?: string;
}

export interface DshJobsFrame {
  type: "session/jobs";
  sessionId: string;
  jobs: DshJobView[];
}

/** `session/queue` push frame: pending-prompt queue snapshot. */
export interface DshQueueItem {
  /** Message id used by session.updateQueue. */
  id?: string;
  placement?: "queued" | "steering" | "context";
  message?: {
    id: string;
    role: "system" | "user" | "assistant";
    content: unknown[];
    source?: { kind: string; [key: string]: unknown };
  };
  /** Legacy aliases accepted while talking to pre-rc.5 Web builds. */
  itemId?: string;
  text?: string;
}

export interface DshQueueFrame {
  type: "session/queue";
  sessionId: string;
  items: DshQueueItem[];
}

/** `session/subscribed` frame: confirms the mux subscription with lastSeq. */
export interface DshSubscribedFrame {
  type: "session/subscribed";
  sessionId: string;
  lastSeq?: number;
}

export interface DshSessionStatsProjection {
  turns: number;
  steps: number;
  llmMs: number;
  toolMs: number;
  ttftMs: number;
  ttftSteps: number;
  decodeMs: number;
  decodeTokens: number;
}

export interface DshTokenUsageProjection {
  uncachedInputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

/** Aggregated per-session live state derived from projection/jobs/queue frames. */
export interface DshLiveSessionState {
  title?: string;
  todo?: DshTodoItem[];
  goal?: DshGoalProjection["goal"];
  planMode?: boolean;
  permissions?: unknown;
  jobs?: DshJobView[];
  queue?: DshQueueItem[];
  lastSeq?: number;
  /** Every projection unit is retained so optional Harness UI plugins can consume it. */
  projections?: Record<string, unknown>;
}
