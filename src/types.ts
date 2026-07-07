export interface Project {
  id: string;
  name: string;
  path: string;
  location?: ProjectLocation;
  branch?: string;
  lastOpenedAt: number;
  orderIndex?: number;
  /** 为 true 时不在左侧常驻竖条显示，仅可从首页或「展开全部」抽屉访问。缺省=常驻。 */
  hiddenFromRail?: boolean;
}

export type ProjectLocation =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connectionId: string; remotePath: string };

export function resolveProjectLocation(project: Project): ProjectLocation {
  return project.location ?? { kind: "local", path: project.path };
}

export function isRemoteProject(project: Project): boolean {
  return resolveProjectLocation(project).kind === "ssh";
}

export function sshProjectPath(connectionId: string, remotePath: string): string {
  const normalizedRemotePath = remotePath.startsWith("/") ? remotePath : `/${remotePath}`;
  return `ssh://${connectionId}${normalizedRemotePath}`;
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
  createdAt: number;
  lastConnectedAt?: number;
}

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
  DbxObjectSource,
  DbxObjectSourceKind,
  DbxQueryResult,
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

export type BuiltInAgentType = "claude" | "claude_gpt55" | "codex";
export type AgentType = BuiltInAgentType | (string & {});
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
export const DEFAULT_UI_FONT: FontFamily =
  '"SF Pro Display", "IBM Plex Sans", "PingFang SC", "Noto Sans SC", sans-serif';
export const DEFAULT_MONO_FONT: FontFamily =
  '"JetBrains Mono", "Fira Code", ui-monospace, monospace';

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
