import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  AlertCircle,
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  Bot,
  Brain,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Code2,
  Copy,
  DatabaseZap,
  Eye,
  EyeOff,
  Gauge,
  Globe2,
  House,
  ListOrdered,
  Network,
  Plus,
  Power,
  RefreshCw,
  RotateCcw,
  Route,
  Search,
  ScrollText,
  ShieldCheck,
  Shuffle,
  SlidersHorizontal,
  Timer,
  Trash2,
  XCircle,
  type LucideIcon,
} from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { AnimatedSelectionGroup } from "../ui/AnimatedSelection";
import { Button } from "../ui/Button";
import { writeClipboardText } from "../file-explorer/clipboard";
import {
  APP_SETTINGS_CHANGED_EVENT,
  DEFAULT_LOCAL_ROUTER_SETTINGS,
  normalizeLocalRouterSettings,
  type AppSettings,
  type LocalRouterAgent,
  type LocalRouterAgentSettings,
  type LocalRouterRequestRecord,
  type LocalRouterSettings,
  type LocalRouterStatus,
  type LocalRouterTargetStatus,
} from "./types";

const STATUS_REFRESH_INTERVAL_MS = 5_000;
const REQUEST_REFRESH_INTERVAL_MS = 10_000;
const MIN_ROUTER_PORT = 1024;
const MAX_ROUTER_PORT = 65535;
const REQUEST_LOG_LIMIT = 20;
const ROUTER_AGENTS: readonly LocalRouterAgent[] = ["claude", "codex"];
const MIN_ACCESS_TOKEN_LENGTH = 32;

type Translate = ReturnType<typeof useI18n>["t"];

const labelStyle: CSSProperties = {
  display: "block",
  marginBottom: 5,
  color: "var(--text-secondary)",
  fontSize: 12,
  fontWeight: 600,
};

const hintStyle: CSSProperties = {
  marginTop: 4,
  color: "var(--text-hint)",
  fontSize: 11,
  lineHeight: 1.45,
};

const inputStyle: CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  padding: "8px 10px",
  border: "1px solid var(--border-medium)",
  borderRadius: "var(--radius-sm)",
  outline: "none",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 12.5,
};

const insetPanelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 10,
  padding: 12,
  border: "1px solid var(--border-dim)",
  borderRadius: "var(--radius-md)",
  background: "color-mix(in srgb, var(--bg-input) 64%, transparent)",
};

function stripHostBrackets(value: string): string {
  const trimmed = value.trim();
  return trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1).trim() : trimmed;
}

function isValidIpv4(value: string): boolean {
  const parts = value.split(".");
  return (
    parts.length === 4 &&
    parts.every((part) => {
      if (!/^\d{1,3}$/.test(part)) return false;
      if (part.length > 1 && part.startsWith("0")) return false;
      const parsed = Number(part);
      return parsed >= 0 && parsed <= 255;
    })
  );
}

function isValidIpv6(value: string): boolean {
  const candidate = stripHostBrackets(value);
  if (!candidate.includes(":") || candidate.includes("%")) return false;
  try {
    const parsed = new URL(`http://[${candidate}]:${DEFAULT_LOCAL_ROUTER_SETTINGS.listen_port}/`);
    return parsed.hostname.length > 0;
  } catch {
    return false;
  }
}

function isValidRouterHost(value: string): boolean {
  const candidate = stripHostBrackets(value);
  return (
    candidate.toLowerCase() === "localhost" || isValidIpv4(candidate) || isValidIpv6(candidate)
  );
}

function normalizeRouterHost(value: string): string {
  const candidate = stripHostBrackets(value);
  return candidate.toLowerCase() === "localhost" ? "localhost" : candidate;
}

function generateRouterAccessToken(): string {
  const bytes = new Uint8Array(24);
  crypto.getRandomValues(bytes);
  return `aeroric-${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function isLoopbackRouterHost(value: string): boolean {
  const candidate = stripHostBrackets(value).toLowerCase();
  if (candidate === "localhost" || candidate === "::1" || candidate === "0:0:0:0:0:0:0:1") {
    return true;
  }
  return isValidIpv4(candidate) && candidate.split(".")[0] === "127";
}

function routerSettingsEqual(a: LocalRouterSettings, b: LocalRouterSettings): boolean {
  return (
    a.show_on_home === b.show_on_home &&
    a.enabled === b.enabled &&
    a.listen_host === b.listen_host &&
    a.listen_port === b.listen_port &&
    a.access_token === b.access_token &&
    a.claude_enabled === b.claude_enabled &&
    a.codex_enabled === b.codex_enabled &&
    a.record_usage === b.record_usage &&
    a.use_global_proxy === b.use_global_proxy &&
    agentSettingsEqual(a.claude, b.claude) &&
    agentSettingsEqual(a.codex, b.codex)
  );
}

function agentSettingsEqual(a: LocalRouterAgentSettings, b: LocalRouterAgentSettings): boolean {
  return (
    a.auto_failover_enabled === b.auto_failover_enabled &&
    a.max_retries === b.max_retries &&
    a.streaming_first_byte_timeout === b.streaming_first_byte_timeout &&
    a.streaming_idle_timeout === b.streaming_idle_timeout &&
    a.non_streaming_timeout === b.non_streaming_timeout &&
    a.circuit_failure_threshold === b.circuit_failure_threshold &&
    a.circuit_success_threshold === b.circuit_success_threshold &&
    a.circuit_timeout_seconds === b.circuit_timeout_seconds &&
    a.circuit_error_rate_percent === b.circuit_error_rate_percent &&
    a.circuit_min_requests === b.circuit_min_requests &&
    a.active_target === b.active_target &&
    a.failover_queue.length === b.failover_queue.length &&
    a.failover_queue.every((target, index) => target === b.failover_queue[index]) &&
    a.model_mapping_enabled === b.model_mapping_enabled &&
    a.rectifier_enabled === b.rectifier_enabled &&
    a.thinking_optimizer_enabled === b.thinking_optimizer_enabled &&
    a.cache_injection_enabled === b.cache_injection_enabled
  );
}

function isAgentSettingsValid(settings: LocalRouterAgentSettings): boolean {
  const inRange = (value: number, min: number, max: number) =>
    Number.isInteger(value) && value >= min && value <= max;
  return (
    inRange(settings.max_retries, 0, 10) &&
    inRange(settings.streaming_first_byte_timeout, 1, 120) &&
    (settings.streaming_idle_timeout === 0 || inRange(settings.streaming_idle_timeout, 60, 600)) &&
    inRange(settings.non_streaming_timeout, 60, 1200) &&
    inRange(settings.circuit_failure_threshold, 1, 20) &&
    inRange(settings.circuit_success_threshold, 1, 10) &&
    inRange(settings.circuit_timeout_seconds, 0, 300) &&
    inRange(settings.circuit_error_rate_percent, 0, 100) &&
    inRange(settings.circuit_min_requests, 5, 100)
  );
}

function isFailoverQueueValid(
  settings: LocalRouterAgentSettings,
  targets: LocalRouterTargetStatus[],
): boolean {
  if (!settings.auto_failover_enabled) return true;
  if (settings.failover_queue.length === 0) return false;
  if (targets.length === 0) return true;
  const targetIds = new Set(targets.map((target) => target.target_id));
  return settings.failover_queue.some((targetId) => targetIds.has(targetId));
}

function preferredTargetIds(targets: LocalRouterTargetStatus[]): string[] {
  const active = targets.find((target) => target.active)?.target_id;
  return targets
    .map((target) => target.target_id)
    .sort((left, right) => {
      if (left === active) return -1;
      if (right === active) return 1;
      return 0;
    });
}

function sanitizeAgentSettings(
  settings: LocalRouterAgentSettings,
  targets: LocalRouterTargetStatus[],
): LocalRouterAgentSettings {
  if (!targets.length) {
    return { ...settings, failover_queue: [...settings.failover_queue] };
  }

  const targetIds = new Set(targets.map((target) => target.target_id));
  const seen = new Set<string>();
  let failoverQueue = settings.failover_queue.filter(
    (targetId) => targetIds.has(targetId) && !seen.has(targetId) && seen.add(targetId),
  );
  if (settings.auto_failover_enabled && failoverQueue.length === 0) {
    failoverQueue = preferredTargetIds(targets);
  }
  const runtimeActive = targets.find((target) => target.active)?.target_id;
  const activeTarget = targetIds.has(settings.active_target)
    ? settings.active_target
    : (runtimeActive ?? failoverQueue[0] ?? targets[0].target_id);

  return {
    ...settings,
    active_target: activeTarget,
    failover_queue: failoverQueue,
  };
}

function isAgentEnabled(settings: LocalRouterSettings, agent: LocalRouterAgent): boolean {
  return agent === "claude" ? settings.claude_enabled : settings.codex_enabled;
}

function SectionHeading({
  icon: Icon,
  children,
  action,
}: {
  icon?: LucideIcon;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 10,
        marginBottom: 9,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          color: "var(--text-secondary)",
          fontSize: 11,
          fontWeight: 700,
          textTransform: "uppercase",
        }}
      >
        {Icon ? <Icon size={13} strokeWidth={2} /> : null}
        {children}
      </div>
      {action}
    </div>
  );
}

function ToggleRow({
  label,
  hint,
  checked,
  disabled,
  icon: Icon,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  icon: LucideIcon;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      style={{
        ...s.settingToggle,
        alignItems: hint ? "flex-start" : "center",
        opacity: disabled ? 0.55 : 1,
        cursor: disabled ? "not-allowed" : "pointer",
      }}
    >
      <span style={{ display: "flex", minWidth: 0, gap: 10 }}>
        <Icon
          size={15}
          strokeWidth={1.9}
          color="var(--text-muted)"
          style={{ marginTop: 1, flexShrink: 0 }}
        />
        <span style={{ display: "flex", minWidth: 0, flexDirection: "column", gap: 3 }}>
          <span style={s.settingToggleLabel}>{label}</span>
          {hint ? (
            <span style={{ ...hintStyle, marginTop: 0, textAlign: "left" }}>{hint}</span>
          ) : null}
        </span>
      </span>
      <span
        aria-hidden="true"
        style={{
          ...s.settingToggleTrack,
          marginTop: hint ? 1 : 0,
          background: checked ? "var(--primary-action-bg)" : "var(--border-medium)",
        }}
      >
        <span
          style={{
            ...s.settingToggleKnob,
            transform: checked ? "translateX(16px)" : "translateX(0)",
          }}
        />
      </span>
    </button>
  );
}

function Metric({ icon: Icon, label, value }: { icon: LucideIcon; label: string; value: number }) {
  return (
    <div style={{ minWidth: 0, padding: "4px 10px", borderLeft: "1px solid var(--border-dim)" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 5,
          color: "var(--text-hint)",
          fontSize: 10.5,
        }}
      >
        <Icon size={12} strokeWidth={1.9} />
        <span>{label}</span>
      </div>
      <div
        style={{
          marginTop: 5,
          color: "var(--text-primary)",
          fontSize: 18,
          fontWeight: 700,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value.toLocaleString()}
      </div>
    </div>
  );
}

function NumberField({
  id,
  label,
  hint,
  value,
  min,
  max,
  allowZero = false,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  allowZero?: boolean;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  const invalid =
    !Number.isInteger(value) || (!(allowZero && value === 0) && (value < min || value > max));
  return (
    <div style={{ minWidth: 0 }}>
      <label htmlFor={id} style={labelStyle}>
        {label}
      </label>
      <input
        id={id}
        type="number"
        min={allowZero ? 0 : min}
        max={max}
        step={1}
        value={value}
        disabled={disabled}
        aria-invalid={invalid || undefined}
        aria-describedby={`${id}-hint`}
        style={{
          ...inputStyle,
          borderColor: invalid ? "var(--danger)" : "var(--border-medium)",
          opacity: disabled ? 0.6 : 1,
        }}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
      />
      <div
        id={`${id}-hint`}
        style={{ ...hintStyle, color: invalid ? "var(--danger)" : "var(--text-hint)" }}
      >
        {hint}
      </div>
    </div>
  );
}

function InlineBadge({
  children,
  color = "var(--text-secondary)",
}: {
  children: ReactNode;
  color?: string;
}) {
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        minHeight: 20,
        padding: "1px 6px",
        border: "1px solid color-mix(in srgb, currentColor 28%, transparent)",
        borderRadius: 999,
        color,
        background: "color-mix(in srgb, currentColor 8%, transparent)",
        fontSize: 10.5,
        fontWeight: 700,
        lineHeight: 1.2,
      }}
    >
      {children}
    </span>
  );
}

function TargetCard({
  target,
  queuePosition,
  settings,
  agentEnabled,
  action,
  disabled,
  t,
  onSwitch,
  onToggleQueue,
  onResetCircuit,
}: {
  target: LocalRouterTargetStatus;
  queuePosition: number | null;
  settings: LocalRouterAgentSettings;
  agentEnabled: boolean;
  action: string | null;
  disabled: boolean;
  t: Translate;
  onSwitch: () => void;
  onToggleQueue: () => void;
  onResetCircuit: () => void;
}) {
  const switchAction = `switch:${target.agent}:${target.target_id}`;
  const resetAction = `reset:${target.agent}:${target.target_id}`;
  const switching = action === switchAction;
  const resetting = action === resetAction;
  const isPrimary = queuePosition === 1;
  const inFailoverQueue = queuePosition !== null;
  const circuitColor =
    target.circuit.state === "open"
      ? "var(--danger)"
      : target.circuit.state === "half_open"
        ? "var(--color-warning)"
        : "var(--success)";
  const circuitLabel =
    target.circuit.state === "open"
      ? t("appSettings.localRouter.circuitOpen")
      : target.circuit.state === "half_open"
        ? t("appSettings.localRouter.circuitHalfOpen")
        : t("appSettings.localRouter.circuitClosed");
  const switchDisabled = settings.auto_failover_enabled ? isPrimary : target.active;

  return (
    <article
      aria-label={target.target_name}
      style={{
        ...insetPanelStyle,
        gap: 9,
        borderColor: inFailoverQueue
          ? "color-mix(in srgb, var(--success) 72%, var(--border-dim))"
          : target.active
            ? "color-mix(in srgb, var(--success) 46%, var(--border-dim))"
            : "var(--border-dim)",
        boxShadow: inFailoverQueue
          ? "0 0 0 1px color-mix(in srgb, var(--success) 18%, transparent)"
          : "none",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 10,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <strong
              style={{
                minWidth: 0,
                color: "var(--text-primary)",
                fontSize: 12.5,
                overflowWrap: "anywhere",
              }}
            >
              {target.target_name}
            </strong>
            {target.active ? (
              <InlineBadge color="var(--success)">
                {t("appSettings.localRouter.targetActive")}
              </InlineBadge>
            ) : null}
            {queuePosition ? (
              <InlineBadge color="var(--accent)">
                {t("appSettings.localRouter.targetPosition", { pos: queuePosition })}
              </InlineBadge>
            ) : null}
            {isPrimary ? (
              <InlineBadge color="var(--accent)">
                {t("appSettings.localRouter.targetPrimary")}
              </InlineBadge>
            ) : null}
          </div>
          <div
            title={target.base_url}
            style={{
              marginTop: 5,
              color: "var(--text-hint)",
              fontFamily: "var(--font-mono)",
              fontSize: 10.5,
              lineHeight: 1.4,
              overflowWrap: "anywhere",
            }}
          >
            {target.base_url}
          </div>
        </div>
        <InlineBadge color={circuitColor}>{circuitLabel}</InlineBadge>
      </div>

      <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
        {target.models.length ? (
          <InlineBadge>
            {t("appSettings.localRouter.targetModels", { count: target.models.length })}
          </InlineBadge>
        ) : (
          <InlineBadge>{t("appSettings.localRouter.targetRequestedModel")}</InlineBadge>
        )}
        {target.enable_1m_context ? (
          <InlineBadge color="var(--accent)">{t("appSettings.localRouter.targetOneM")}</InlineBadge>
        ) : null}
        {target.enable_chat_completions_proxy ? (
          <InlineBadge color="var(--accent)">
            {t("appSettings.localRouter.targetChatBridge")}
          </InlineBadge>
        ) : null}
      </div>

      {target.models.length ? (
        <div
          title={target.models.join(", ")}
          style={{
            color: "var(--text-hint)",
            fontFamily: "var(--font-mono)",
            fontSize: 10.5,
            lineHeight: 1.45,
            overflowWrap: "anywhere",
          }}
        >
          {target.models.join(", ")}
        </div>
      ) : null}

      {target.circuit.consecutive_failures > 0 || target.circuit.last_error ? (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 3,
            color: target.circuit.state === "open" ? "var(--danger)" : "var(--text-hint)",
            fontSize: 10.5,
            lineHeight: 1.4,
          }}
        >
          <span>
            {t("appSettings.localRouter.targetFailures", {
              count: target.circuit.consecutive_failures,
            })}
          </span>
          {target.circuit.last_error ? (
            <span title={target.circuit.last_error} style={{ overflowWrap: "anywhere" }}>
              {target.circuit.last_error}
            </span>
          ) : null}
        </div>
      ) : null}

      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
        <Button
          variant={switchDisabled ? "secondary" : "outline"}
          size="xs"
          disabled={disabled || !agentEnabled || switchDisabled || Boolean(action)}
          onClick={onSwitch}
        >
          {switching
            ? t("appSettings.localRouter.targetSwitching")
            : settings.auto_failover_enabled
              ? t("appSettings.localRouter.targetMakePrimary")
              : t("appSettings.localRouter.targetSwitch")}
        </Button>
        <Button
          variant={queuePosition ? "secondary" : "ghost"}
          size="xs"
          disabled={disabled || Boolean(action)}
          onClick={onToggleQueue}
        >
          {queuePosition ? (
            <Trash2 size={12} strokeWidth={2} />
          ) : (
            <Plus size={12} strokeWidth={2} />
          )}
          {queuePosition
            ? t("appSettings.localRouter.targetRemoveQueue")
            : t("appSettings.localRouter.targetAddQueue")}
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={t("appSettings.localRouter.targetResetCircuit", {
            target: target.target_name,
          })}
          title={t("appSettings.localRouter.targetResetCircuit", {
            target: target.target_name,
          })}
          disabled={disabled || Boolean(action)}
          onClick={onResetCircuit}
        >
          <RotateCcw
            size={12}
            strokeWidth={2}
            style={{ animation: resetting ? "spin 0.8s linear infinite" : undefined }}
          />
        </Button>
      </div>
    </article>
  );
}

function FailoverQueueEditor({
  agent,
  queue,
  targets,
  disabled,
  t,
  onInitialize,
  onMove,
  onRemove,
}: {
  agent: LocalRouterAgent;
  queue: string[];
  targets: LocalRouterTargetStatus[];
  disabled: boolean;
  t: Translate;
  onInitialize: () => void;
  onMove: (index: number, direction: -1 | 1) => void;
  onRemove: (targetId: string) => void;
}) {
  const targetById = new Map(targets.map((target) => [target.target_id, target]));

  if (!queue.length) {
    return (
      <div
        style={{
          ...insetPanelStyle,
          alignItems: "center",
          padding: "16px 12px",
          borderStyle: "dashed",
          color: "var(--text-hint)",
          textAlign: "center",
          fontSize: 11,
          lineHeight: 1.5,
        }}
      >
        <span>{t("appSettings.localRouter.failoverQueueEmpty")}</span>
        {targets.length ? (
          <Button variant="outline" size="xs" disabled={disabled} onClick={onInitialize}>
            <ListOrdered size={12} strokeWidth={2} />
            {t("appSettings.localRouter.failoverInitialize")}
          </Button>
        ) : null}
      </div>
    );
  }

  return (
    <div style={{ ...insetPanelStyle, gap: 6 }}>
      {queue.map((targetId, index) => {
        const target = targetById.get(targetId);
        const label = target?.target_name ?? targetId;
        return (
          <div
            key={targetId}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              minWidth: 0,
              padding: "6px 7px",
              border: "1px solid var(--border-dim)",
              borderRadius: "var(--radius-sm)",
              background: "var(--bg-input)",
            }}
          >
            <InlineBadge color="var(--accent)">
              {t("appSettings.localRouter.targetPosition", { pos: index + 1 })}
            </InlineBadge>
            <span
              title={label}
              style={{
                minWidth: 0,
                flex: 1,
                color: "var(--text-primary)",
                fontSize: 11.5,
                fontWeight: 600,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {label}
            </span>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={t("appSettings.localRouter.failoverMoveUp", { target: label })}
              title={t("appSettings.localRouter.failoverMoveUp", { target: label })}
              disabled={disabled || index === 0}
              onClick={() => onMove(index, -1)}
            >
              <ArrowUp size={12} strokeWidth={2} />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={t("appSettings.localRouter.failoverMoveDown", { target: label })}
              title={t("appSettings.localRouter.failoverMoveDown", { target: label })}
              disabled={disabled || index === queue.length - 1}
              onClick={() => onMove(index, 1)}
            >
              <ArrowDown size={12} strokeWidth={2} />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={t("appSettings.localRouter.failoverRemove", { target: label })}
              title={t("appSettings.localRouter.failoverRemove", { target: label })}
              disabled={disabled}
              onClick={() => onRemove(targetId)}
            >
              <Trash2 size={12} strokeWidth={2} />
            </Button>
          </div>
        );
      })}
      <div style={{ ...hintStyle, marginTop: 2 }}>
        {t("appSettings.localRouter.failoverQueueHint", {
          agent:
            agent === "claude"
              ? t("appSettings.localRouter.claude")
              : t("appSettings.localRouter.codex"),
        })}
      </div>
    </div>
  );
}

function RequestRecordRow({
  request,
  locale,
  t,
}: {
  request: LocalRouterRequestRecord;
  locale: string;
  t: Translate;
}) {
  const statusColor = request.success ? "var(--success)" : "var(--danger)";
  const target =
    request.targetName ?? request.targetId ?? t("appSettings.localRouter.requestNoTarget");
  const originalModel = request.model || t("appSettings.localRouter.requestUnknownModel");
  const mappedModel =
    request.outboundModel && request.outboundModel !== request.model
      ? t("appSettings.localRouter.requestModelMapping", {
          from: originalModel,
          to: request.outboundModel,
        })
      : originalModel;
  const completedAt = new Date(request.completedAt).toLocaleString(locale, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });

  return (
    <article
      role="listitem"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 6,
        padding: "9px 10px",
        border: "1px solid var(--border-dim)",
        borderRadius: "var(--radius-sm)",
        background: "color-mix(in srgb, var(--bg-input) 68%, transparent)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <span
          aria-hidden="true"
          style={{
            width: 7,
            height: 7,
            flexShrink: 0,
            borderRadius: 999,
            background: statusColor,
          }}
        />
        <strong style={{ color: "var(--text-primary)", fontSize: 11.5 }}>
          {request.agent === "claude"
            ? t("appSettings.localRouter.claude")
            : t("appSettings.localRouter.codex")}
        </strong>
        <span
          title={target}
          style={{
            minWidth: 0,
            flex: 1,
            color: "var(--text-secondary)",
            fontSize: 11,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {target}
        </span>
        <span
          style={{
            flexShrink: 0,
            color: "var(--text-hint)",
            fontFamily: "var(--font-mono)",
            fontSize: 10,
          }}
        >
          {completedAt}
        </span>
      </div>
      <div
        title={t("appSettings.localRouter.requestEndpointModel", {
          endpoint: request.endpoint,
          model: mappedModel,
        })}
        style={{
          color: "var(--text-hint)",
          fontFamily: "var(--font-mono)",
          fontSize: 10.5,
          lineHeight: 1.4,
          overflowWrap: "anywhere",
        }}
      >
        {t("appSettings.localRouter.requestEndpointModel", {
          endpoint: request.endpoint,
          model: mappedModel,
        })}
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
        <InlineBadge color={statusColor}>
          {t("appSettings.localRouter.requestStatus", { code: request.statusCode })}
        </InlineBadge>
        <InlineBadge>
          {t("appSettings.localRouter.requestAttempts", { count: request.attemptCount })}
        </InlineBadge>
        <InlineBadge>
          {t("appSettings.localRouter.requestLatencyMs", {
            ms: request.latencyMs.toLocaleString(),
          })}
        </InlineBadge>
        {request.isStreaming ? (
          <InlineBadge>{t("appSettings.localRouter.requestStreaming")}</InlineBadge>
        ) : null}
        <InlineBadge>
          {t("appSettings.localRouter.requestTokens", {
            input: request.inputTokens,
            output: request.outputTokens,
          })}
        </InlineBadge>
      </div>
      {request.errorSummary ? (
        <div
          title={request.errorSummary}
          style={{
            color: "var(--danger)",
            fontSize: 10.5,
            lineHeight: 1.45,
            overflowWrap: "anywhere",
          }}
        >
          {request.errorSummary}
        </div>
      ) : null}
    </article>
  );
}

export function LocalRouterPanel() {
  const { language, t } = useI18n();
  const [settings, setSettings] = useState<LocalRouterSettings>(DEFAULT_LOCAL_ROUTER_SETTINGS);
  const [originalSettings, setOriginalSettings] = useState<LocalRouterSettings>(
    DEFAULT_LOCAL_ROUTER_SETTINGS,
  );
  const [selectedAgent, setSelectedAgent] = useState<LocalRouterAgent>("claude");
  const [targetSearchQuery, setTargetSearchQuery] = useState("");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [portDraft, setPortDraft] = useState(String(DEFAULT_LOCAL_ROUTER_SETTINGS.listen_port));
  const [status, setStatus] = useState<LocalRouterStatus | null>(null);
  const [requests, setRequests] = useState<LocalRouterRequestRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [requestsLoading, setRequestsLoading] = useState(false);
  const [targetAction, setTargetAction] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [showAccessToken, setShowAccessToken] = useState(false);
  const [accessTokenCopied, setAccessTokenCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      const nextStatus = await invoke<LocalRouterStatus>("get_local_router_status");
      setStatus(nextStatus);
      return nextStatus;
    } catch (cause) {
      setError(String(cause));
      return null;
    }
  }, []);

  const refreshRequests = useCallback(async (showProgress = false) => {
    if (showProgress) setRequestsLoading(true);
    try {
      const nextRequests = await invoke<LocalRouterRequestRecord[]>("get_local_router_requests", {
        limit: REQUEST_LOG_LIMIT,
      });
      setRequests(nextRequests);
      return nextRequests;
    } catch (cause) {
      setError(String(cause));
      return null;
    } finally {
      if (showProgress) setRequestsLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;

    void Promise.allSettled([
      invoke<AppSettings>("load_app_settings"),
      invoke<LocalRouterStatus>("get_local_router_status"),
      invoke<LocalRouterRequestRecord[]>("get_local_router_requests", {
        limit: REQUEST_LOG_LIMIT,
      }),
    ]).then(([settingsResult, statusResult, requestsResult]) => {
      if (cancelled) return;

      if (settingsResult.status === "fulfilled") {
        const next = normalizeLocalRouterSettings(settingsResult.value.local_router_settings);
        setSettings(next);
        setOriginalSettings(next);
        setPortDraft(String(next.listen_port));
      } else {
        setError(String(settingsResult.reason));
      }

      if (statusResult.status === "fulfilled") {
        setStatus(statusResult.value);
      } else {
        setError(String(statusResult.reason));
      }

      if (requestsResult.status === "fulfilled") {
        setRequests(requestsResult.value);
      } else {
        setError(String(requestsResult.reason));
      }
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (document.visibilityState === "visible" && !toggling && !saving && !targetAction) {
        void refreshStatus();
      }
    }, STATUS_REFRESH_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [refreshStatus, saving, targetAction, toggling]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (document.visibilityState === "visible" && settings.record_usage && !saving) {
        void refreshRequests();
      }
    }, REQUEST_REFRESH_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [refreshRequests, saving, settings.record_usage]);

  const parsedPort = portDraft === "" ? Number.NaN : Number(portDraft);
  const portInvalid =
    !Number.isInteger(parsedPort) || parsedPort < MIN_ROUTER_PORT || parsedPort > MAX_ROUTER_PORT;
  const normalizedHost = normalizeRouterHost(settings.listen_host);
  const hostInvalid = !isValidRouterHost(normalizedHost);
  const nonLoopbackHost = !hostInvalid && !isLoopbackRouterHost(normalizedHost);
  const accessTokenInvalid =
    nonLoopbackHost && settings.access_token.trim().length < MIN_ACCESS_TOKEN_LENGTH;
  const targetsByAgent = useMemo(() => {
    const targets = status?.targets ?? [];
    return {
      claude: targets.filter((target) => target.agent === "claude"),
      codex: targets.filter((target) => target.agent === "codex"),
    };
  }, [status?.targets]);
  const agentConfigurationInvalid =
    !isAgentSettingsValid(settings.claude) || !isAgentSettingsValid(settings.codex);
  const failoverConfigurationInvalid =
    (settings.claude_enabled && !isFailoverQueueValid(settings.claude, targetsByAgent.claude)) ||
    (settings.codex_enabled && !isFailoverQueueValid(settings.codex, targetsByAgent.codex));
  const configurationInvalid =
    hostInvalid ||
    portInvalid ||
    accessTokenInvalid ||
    agentConfigurationInvalid ||
    failoverConfigurationInvalid;
  const settingsForComparison = useMemo(
    () => ({
      ...settings,
      listen_host: normalizedHost,
      listen_port: portInvalid ? settings.listen_port : parsedPort,
    }),
    [normalizedHost, parsedPort, portInvalid, settings],
  );
  const dirty =
    !routerSettingsEqual(settingsForComparison, originalSettings) ||
    portDraft !== String(originalSettings.listen_port);
  const busy = loading || saving || toggling;
  const desiredEnabled = status?.desired_enabled ?? settings.enabled;
  const selectedTargets = targetsByAgent[selectedAgent];
  const visibleTargets = useMemo(() => {
    const query = targetSearchQuery.trim().toLocaleLowerCase();
    if (!query) return selectedTargets;
    return selectedTargets.filter((target) =>
      `${target.target_name} ${target.target_id}`.toLocaleLowerCase().includes(query),
    );
  }, [selectedTargets, targetSearchQuery]);
  const selectedAgentSettings = settings[selectedAgent];
  const selectedAgentEnabled = isAgentEnabled(settings, selectedAgent);
  const selectedFailoverQueueInvalid =
    selectedAgentEnabled && !isFailoverQueueValid(selectedAgentSettings, selectedTargets);
  const selectedAgentLabel =
    selectedAgent === "claude"
      ? t("appSettings.localRouter.claude")
      : t("appSettings.localRouter.codex");

  let statusLabel = t("appSettings.localRouter.statusStopped");
  let statusColor = "var(--text-hint)";
  if (loading) {
    statusLabel = t("common.loadingEllipsis");
  } else if (status?.starting) {
    statusLabel = t("appSettings.localRouter.statusStarting");
    statusColor = "var(--color-warning)";
  } else if (status?.running) {
    statusLabel = status.listen_url
      ? t("appSettings.localRouter.statusRunningAt", { url: status.listen_url })
      : t("appSettings.localRouter.statusRunning");
    statusColor = "var(--success)";
  } else if (status?.desired_enabled && status.last_error) {
    statusLabel = t("appSettings.localRouter.statusFailed");
    statusColor = "var(--danger)";
  }

  const updateAgentSettings = useCallback(
    (
      agent: LocalRouterAgent,
      updater: (current: LocalRouterAgentSettings) => LocalRouterAgentSettings,
    ) => {
      setSettings((previous) => ({ ...previous, [agent]: updater(previous[agent]) }));
    },
    [],
  );

  function updatePersistedAgentSettings(
    agent: LocalRouterAgent,
    updater: (current: LocalRouterAgentSettings) => LocalRouterAgentSettings,
  ) {
    setOriginalSettings((previous) => ({ ...previous, [agent]: updater(previous[agent]) }));
  }

  async function handleRefreshAll() {
    setRefreshing(true);
    setError(null);
    await Promise.allSettled([refreshStatus(), refreshRequests()]);
    setRefreshing(false);
  }

  async function handleServiceToggle(enabled: boolean) {
    setToggling(true);
    setError(null);
    try {
      const nextStatus = await invoke<LocalRouterStatus>("set_local_router_enabled", { enabled });
      setStatus(nextStatus);
      setSettings((previous) => ({ ...previous, enabled: nextStatus.desired_enabled }));
      setOriginalSettings((previous) => ({
        ...previous,
        enabled: nextStatus.desired_enabled,
      }));
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } catch (cause) {
      setError(String(cause));
      await refreshStatus();
    } finally {
      setToggling(false);
    }
  }

  function handleAgentEnabledChange(agent: LocalRouterAgent, enabled: boolean) {
    setSettings((previous) =>
      agent === "claude"
        ? { ...previous, claude_enabled: enabled }
        : { ...previous, codex_enabled: enabled },
    );
  }

  function handleAutoFailoverChange(enabled: boolean) {
    updateAgentSettings(selectedAgent, (current) => {
      const initializedQueue =
        enabled && current.failover_queue.length === 0
          ? preferredTargetIds(selectedTargets)
          : current.failover_queue;
      return {
        ...current,
        auto_failover_enabled: enabled,
        failover_queue: initializedQueue,
        active_target:
          enabled && initializedQueue.length > 0
            ? initializedQueue[0]
            : current.active_target ||
              selectedTargets.find((target) => target.active)?.target_id ||
              "",
      };
    });
  }

  function initializeFailoverQueue() {
    updateAgentSettings(selectedAgent, (current) => {
      const failoverQueue = preferredTargetIds(selectedTargets);
      return {
        ...current,
        failover_queue: failoverQueue,
        active_target:
          current.auto_failover_enabled && failoverQueue.length > 0
            ? failoverQueue[0]
            : current.active_target,
      };
    });
  }

  function moveFailoverTarget(index: number, direction: -1 | 1) {
    updateAgentSettings(selectedAgent, (current) => {
      const nextIndex = index + direction;
      if (nextIndex < 0 || nextIndex >= current.failover_queue.length) return current;
      const failoverQueue = [...current.failover_queue];
      [failoverQueue[index], failoverQueue[nextIndex]] = [
        failoverQueue[nextIndex],
        failoverQueue[index],
      ];
      return {
        ...current,
        failover_queue: failoverQueue,
        active_target:
          current.auto_failover_enabled && failoverQueue.length > 0
            ? failoverQueue[0]
            : current.active_target,
      };
    });
  }

  function removeFailoverTarget(targetId: string) {
    updateAgentSettings(selectedAgent, (current) => {
      const failoverQueue = current.failover_queue.filter((id) => id !== targetId);
      return {
        ...current,
        failover_queue: failoverQueue,
        active_target:
          current.auto_failover_enabled && failoverQueue.length > 0
            ? failoverQueue[0]
            : current.active_target,
      };
    });
  }

  function toggleFailoverTarget(targetId: string) {
    updateAgentSettings(selectedAgent, (current) => {
      const contains = current.failover_queue.includes(targetId);
      const failoverQueue = contains
        ? current.failover_queue.filter((id) => id !== targetId)
        : [...current.failover_queue, targetId];
      return {
        ...current,
        failover_queue: failoverQueue,
        active_target:
          current.auto_failover_enabled && failoverQueue.length > 0
            ? failoverQueue[0]
            : current.active_target,
      };
    });
  }

  async function handleSwitchTarget(agent: LocalRouterAgent, targetId: string) {
    const action = `switch:${agent}:${targetId}`;
    setTargetAction(action);
    setError(null);
    try {
      const nextStatus = await invoke<LocalRouterStatus>("switch_local_router_target", {
        agent,
        targetId,
      });
      const promoteTarget = (current: LocalRouterAgentSettings): LocalRouterAgentSettings => ({
        ...current,
        active_target: targetId,
        failover_queue: current.auto_failover_enabled
          ? [targetId, ...current.failover_queue.filter((id) => id !== targetId)]
          : current.failover_queue,
      });
      setStatus(nextStatus);
      updateAgentSettings(agent, promoteTarget);
      updatePersistedAgentSettings(agent, promoteTarget);
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } catch (cause) {
      setError(String(cause));
      await refreshStatus();
    } finally {
      setTargetAction(null);
    }
  }

  async function handleResetCircuit(agent: LocalRouterAgent, targetId: string) {
    const action = `reset:${agent}:${targetId}`;
    setTargetAction(action);
    setError(null);
    try {
      const nextStatus = await invoke<LocalRouterStatus>("reset_local_router_circuit", {
        agent,
        targetId,
      });
      setStatus(nextStatus);
    } catch (cause) {
      setError(String(cause));
      await refreshStatus();
    } finally {
      setTargetAction(null);
    }
  }

  async function handleSave() {
    if (configurationInvalid) return;
    const nextSettings: LocalRouterSettings = {
      ...settings,
      enabled: desiredEnabled,
      listen_host: normalizedHost,
      listen_port: parsedPort,
      claude: sanitizeAgentSettings(settings.claude, targetsByAgent.claude),
      codex: sanitizeAgentSettings(settings.codex, targetsByAgent.codex),
    };

    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      const appSettings = await invoke<AppSettings>("update_local_router_settings", {
        settings: nextSettings,
      });
      const persisted = normalizeLocalRouterSettings(
        appSettings.local_router_settings ?? nextSettings,
      );
      setSettings(persisted);
      setOriginalSettings(persisted);
      setPortDraft(String(persisted.listen_port));
      await Promise.allSettled([refreshStatus(), refreshRequests()]);
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2_000);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  }

  const selectedAgentIcon = selectedAgent === "claude" ? Bot : Code2;
  const locale = language === "zh" ? "zh-CN" : "en-US";

  return (
    <>
      <div
        style={{
          ...s.settingsBody,
          display: "flex",
          flexDirection: "column",
          gap: 18,
          padding: "18px 20px 14px",
        }}
      >
        <section aria-label={t("appSettings.localRouter.serviceSection")}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 12,
              marginBottom: 10,
            }}
          >
            <div style={{ display: "flex", minWidth: 0, alignItems: "center", gap: 8 }}>
              <span
                aria-hidden="true"
                style={{
                  width: 8,
                  height: 8,
                  flexShrink: 0,
                  borderRadius: 999,
                  background: statusColor,
                  boxShadow: status?.running
                    ? `0 0 0 3px color-mix(in srgb, ${statusColor} 18%, transparent)`
                    : "none",
                }}
              />
              <span
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  color: statusColor,
                  fontFamily: status?.running && status.listen_url ? "var(--font-mono)" : undefined,
                  fontSize: 11.5,
                  fontWeight: 600,
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={statusLabel}
              >
                {statusLabel}
              </span>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t("appSettings.localRouter.refreshStatus")}
              title={t("appSettings.localRouter.refreshStatus")}
              disabled={loading || refreshing}
              onClick={() => void handleRefreshAll()}
            >
              <RefreshCw
                size={14}
                strokeWidth={2}
                style={{ animation: refreshing ? "spin 0.8s linear infinite" : undefined }}
              />
            </Button>
          </div>

          <ToggleRow
            icon={Power}
            label={t("appSettings.localRouter.serviceToggle")}
            hint={t("appSettings.localRouter.serviceToggleHint")}
            checked={desiredEnabled}
            disabled={busy || status?.starting}
            onChange={(enabled) => void handleServiceToggle(enabled)}
          />

          {error ? (
            <div
              role="alert"
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 7,
                marginTop: 9,
                color: "var(--danger)",
                fontSize: 11.5,
                lineHeight: 1.45,
              }}
            >
              <AlertCircle size={14} strokeWidth={2} style={{ marginTop: 1, flexShrink: 0 }} />
              <span>{t("appSettings.localRouter.operationFailed", { message: error })}</span>
            </div>
          ) : null}
          {!error && status?.last_error ? (
            <div
              role="alert"
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 7,
                marginTop: 9,
                color: "var(--danger)",
                fontSize: 11.5,
                lineHeight: 1.45,
              }}
            >
              <AlertCircle size={14} strokeWidth={2} style={{ marginTop: 1, flexShrink: 0 }} />
              <span>{t("appSettings.localRouter.lastError", { message: status.last_error })}</span>
            </div>
          ) : null}
        </section>

        <section aria-label={t("appSettings.localRouter.routingSection")}>
          <SectionHeading icon={Route}>
            {t("appSettings.localRouter.routingSection")}
          </SectionHeading>
          <AnimatedSelectionGroup
            value={selectedAgent}
            ariaLabel={t("appSettings.localRouter.agentTabs")}
            role="tablist"
            equalWidth
            style={{ width: "100%", marginBottom: 10, padding: 3 }}
            itemStyle={{ justifyContent: "center", minWidth: 0 }}
            options={ROUTER_AGENTS.map((agent) => ({
              value: agent,
              label: (
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                  {agent === "claude" ? (
                    <Bot size={13} strokeWidth={2} />
                  ) : (
                    <Code2 size={13} strokeWidth={2} />
                  )}
                  <span>
                    {agent === "claude"
                      ? t("appSettings.localRouter.claude")
                      : t("appSettings.localRouter.codex")}
                  </span>
                  <span style={{ color: "var(--text-hint)", fontSize: 10 }}>
                    {targetsByAgent[agent].length}
                  </span>
                </span>
              ),
            }))}
            onChange={setSelectedAgent}
          />

          <div
            role="tabpanel"
            aria-label={selectedAgentLabel}
            style={{ display: "flex", flexDirection: "column", gap: 12 }}
          >
            <ToggleRow
              icon={selectedAgentIcon}
              label={selectedAgentLabel}
              hint={t("appSettings.localRouter.agentHint")}
              checked={selectedAgentEnabled}
              disabled={busy}
              onChange={(enabled) => handleAgentEnabledChange(selectedAgent, enabled)}
            />

            <div>
              <SectionHeading
                icon={Network}
                action={
                  <label
                    style={{
                      position: "relative",
                      display: "flex",
                      minWidth: 0,
                      flex: "1 1 180px",
                      maxWidth: 260,
                      alignItems: "center",
                    }}
                  >
                    <Search
                      aria-hidden="true"
                      size={13}
                      strokeWidth={2}
                      style={{
                        position: "absolute",
                        left: 8,
                        color: "var(--text-hint)",
                        pointerEvents: "none",
                      }}
                    />
                    <input
                      type="search"
                      aria-label={t("appSettings.localRouter.targetSearch")}
                      placeholder={t("appSettings.localRouter.targetSearchPlaceholder")}
                      value={targetSearchQuery}
                      onChange={(event) => setTargetSearchQuery(event.currentTarget.value)}
                      style={{
                        ...inputStyle,
                        height: 28,
                        paddingLeft: 27,
                        fontFamily: "inherit",
                        fontSize: 11.5,
                      }}
                    />
                  </label>
                }
              >
                {t("appSettings.localRouter.targetsSection")}
              </SectionHeading>
              {!selectedAgentEnabled ? (
                <div
                  style={{
                    marginBottom: 8,
                    color: "var(--color-warning)",
                    fontSize: 11,
                    lineHeight: 1.45,
                  }}
                >
                  {t("appSettings.localRouter.agentDisabledHint", { agent: selectedAgentLabel })}
                </div>
              ) : null}
              {selectedTargets.length ? (
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  {visibleTargets.length ? (
                    visibleTargets.map((target) => {
                      const queueIndex = selectedAgentSettings.failover_queue.indexOf(
                        target.target_id,
                      );
                      return (
                        <TargetCard
                          key={target.target_id}
                          target={target}
                          queuePosition={queueIndex >= 0 ? queueIndex + 1 : null}
                          settings={selectedAgentSettings}
                          agentEnabled={selectedAgentEnabled}
                          action={targetAction}
                          disabled={busy}
                          t={t}
                          onSwitch={() => void handleSwitchTarget(selectedAgent, target.target_id)}
                          onToggleQueue={() => toggleFailoverTarget(target.target_id)}
                          onResetCircuit={() =>
                            void handleResetCircuit(selectedAgent, target.target_id)
                          }
                        />
                      );
                    })
                  ) : (
                    <div
                      style={{
                        ...insetPanelStyle,
                        borderStyle: "dashed",
                        color: "var(--text-hint)",
                        fontSize: 11,
                        lineHeight: 1.5,
                      }}
                    >
                      {t("appSettings.localRouter.targetsNoMatch")}
                    </div>
                  )}
                </div>
              ) : (
                <div
                  style={{
                    ...insetPanelStyle,
                    borderStyle: "dashed",
                    color: "var(--text-hint)",
                    fontSize: 11,
                    lineHeight: 1.5,
                  }}
                >
                  {t("appSettings.localRouter.targetsEmpty")}
                </div>
              )}
            </div>

            <div>
              <SectionHeading icon={Shuffle}>
                {t("appSettings.localRouter.failoverSection")}
              </SectionHeading>
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <ToggleRow
                  icon={Shuffle}
                  label={t("appSettings.localRouter.autoFailover")}
                  hint={t("appSettings.localRouter.autoFailoverHint")}
                  checked={selectedAgentSettings.auto_failover_enabled}
                  disabled={busy || !selectedAgentEnabled}
                  onChange={handleAutoFailoverChange}
                />
                {selectedFailoverQueueInvalid ? (
                  <div
                    role="alert"
                    style={{
                      display: "flex",
                      alignItems: "flex-start",
                      gap: 7,
                      color: "var(--danger)",
                      fontSize: 11,
                      lineHeight: 1.45,
                    }}
                  >
                    <AlertCircle size={13} strokeWidth={2} style={{ marginTop: 1 }} />
                    {t(
                      selectedAgentSettings.failover_queue.length === 0
                        ? "appSettings.localRouter.failoverQueueRequired"
                        : "appSettings.localRouter.failoverQueueInvalid",
                    )}
                  </div>
                ) : null}
                <FailoverQueueEditor
                  agent={selectedAgent}
                  queue={selectedAgentSettings.failover_queue}
                  targets={selectedTargets}
                  disabled={busy}
                  t={t}
                  onInitialize={initializeFailoverQueue}
                  onMove={moveFailoverTarget}
                  onRemove={removeFailoverTarget}
                />
              </div>
            </div>

            <div>
              <button
                type="button"
                aria-expanded={advancedOpen}
                onClick={() => setAdvancedOpen((open) => !open)}
                style={{
                  width: "100%",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 10,
                  padding: "9px 10px",
                  border: "1px solid var(--border-dim)",
                  borderRadius: "var(--radius-sm)",
                  background: "var(--bg-input)",
                  color: "var(--text-secondary)",
                  cursor: "pointer",
                  fontFamily: "inherit",
                  fontSize: 11.5,
                  fontWeight: 700,
                  textAlign: "left",
                }}
              >
                <span style={{ display: "inline-flex", alignItems: "center", gap: 7 }}>
                  <SlidersHorizontal size={13} strokeWidth={2} />
                  {t("appSettings.localRouter.advancedSection")}
                </span>
                {advancedOpen ? (
                  <ChevronDown size={14} strokeWidth={2} />
                ) : (
                  <ChevronRight size={14} strokeWidth={2} />
                )}
              </button>

              {advancedOpen ? (
                <div
                  style={{
                    display: "flex",
                    flexDirection: "column",
                    gap: 12,
                    marginTop: 8,
                  }}
                >
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    <ToggleRow
                      icon={Gauge}
                      label={t("appSettings.localRouter.modelMapping")}
                      hint={t("appSettings.localRouter.modelMappingHint")}
                      checked={selectedAgentSettings.model_mapping_enabled}
                      disabled={busy}
                      onChange={(model_mapping_enabled) =>
                        updateAgentSettings(selectedAgent, (current) => ({
                          ...current,
                          model_mapping_enabled,
                        }))
                      }
                    />
                    {selectedAgent === "claude" ? (
                      <>
                        <ToggleRow
                          icon={ShieldCheck}
                          label={t("appSettings.localRouter.rectifier")}
                          hint={t("appSettings.localRouter.rectifierHint")}
                          checked={selectedAgentSettings.rectifier_enabled}
                          disabled={busy}
                          onChange={(rectifier_enabled) =>
                            updateAgentSettings(selectedAgent, (current) => ({
                              ...current,
                              rectifier_enabled,
                            }))
                          }
                        />
                        <ToggleRow
                          icon={Brain}
                          label={t("appSettings.localRouter.thinkingOptimizer")}
                          hint={t("appSettings.localRouter.thinkingOptimizerHint")}
                          checked={selectedAgentSettings.thinking_optimizer_enabled}
                          disabled={busy}
                          onChange={(thinking_optimizer_enabled) =>
                            updateAgentSettings(selectedAgent, (current) => ({
                              ...current,
                              thinking_optimizer_enabled,
                            }))
                          }
                        />
                        <ToggleRow
                          icon={DatabaseZap}
                          label={t("appSettings.localRouter.cacheInjection")}
                          hint={t("appSettings.localRouter.cacheInjectionHint")}
                          checked={selectedAgentSettings.cache_injection_enabled}
                          disabled={busy}
                          onChange={(cache_injection_enabled) =>
                            updateAgentSettings(selectedAgent, (current) => ({
                              ...current,
                              cache_injection_enabled,
                            }))
                          }
                        />
                      </>
                    ) : null}
                  </div>

                  <div style={insetPanelStyle}>
                    <SectionHeading icon={Timer}>
                      {t("appSettings.localRouter.retryTimeoutSection")}
                    </SectionHeading>
                    <div
                      style={{
                        display: "grid",
                        gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
                        gap: 10,
                      }}
                    >
                      <NumberField
                        id={`${selectedAgent}-router-max-retries`}
                        label={t("appSettings.localRouter.maxRetries")}
                        hint={t("appSettings.localRouter.maxRetriesHint")}
                        value={selectedAgentSettings.max_retries}
                        min={0}
                        max={10}
                        allowZero
                        disabled={busy}
                        onChange={(max_retries) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            max_retries,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-first-byte-timeout`}
                        label={t("appSettings.localRouter.streamingFirstByte")}
                        hint={t("appSettings.localRouter.streamingFirstByteHint")}
                        value={selectedAgentSettings.streaming_first_byte_timeout}
                        min={1}
                        max={120}
                        disabled={busy}
                        onChange={(streaming_first_byte_timeout) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            streaming_first_byte_timeout,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-idle-timeout`}
                        label={t("appSettings.localRouter.streamingIdle")}
                        hint={t("appSettings.localRouter.streamingIdleHint")}
                        value={selectedAgentSettings.streaming_idle_timeout}
                        min={60}
                        max={600}
                        allowZero
                        disabled={busy}
                        onChange={(streaming_idle_timeout) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            streaming_idle_timeout,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-non-streaming-timeout`}
                        label={t("appSettings.localRouter.nonStreaming")}
                        hint={t("appSettings.localRouter.nonStreamingHint")}
                        value={selectedAgentSettings.non_streaming_timeout}
                        min={60}
                        max={1200}
                        disabled={busy}
                        onChange={(non_streaming_timeout) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            non_streaming_timeout,
                          }))
                        }
                      />
                    </div>
                  </div>

                  <div style={insetPanelStyle}>
                    <SectionHeading icon={ShieldCheck}>
                      {t("appSettings.localRouter.circuitSection")}
                    </SectionHeading>
                    <div
                      style={{
                        display: "grid",
                        gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
                        gap: 10,
                      }}
                    >
                      <NumberField
                        id={`${selectedAgent}-router-failure-threshold`}
                        label={t("appSettings.localRouter.failureThreshold")}
                        hint={t("appSettings.localRouter.failureThresholdHint")}
                        value={selectedAgentSettings.circuit_failure_threshold}
                        min={1}
                        max={20}
                        disabled={busy}
                        onChange={(circuit_failure_threshold) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            circuit_failure_threshold,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-success-threshold`}
                        label={t("appSettings.localRouter.successThreshold")}
                        hint={t("appSettings.localRouter.successThresholdHint")}
                        value={selectedAgentSettings.circuit_success_threshold}
                        min={1}
                        max={10}
                        disabled={busy}
                        onChange={(circuit_success_threshold) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            circuit_success_threshold,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-circuit-timeout`}
                        label={t("appSettings.localRouter.circuitTimeout")}
                        hint={t("appSettings.localRouter.circuitTimeoutHint")}
                        value={selectedAgentSettings.circuit_timeout_seconds}
                        min={0}
                        max={300}
                        allowZero
                        disabled={busy}
                        onChange={(circuit_timeout_seconds) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            circuit_timeout_seconds,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-error-rate`}
                        label={t("appSettings.localRouter.errorRate")}
                        hint={t("appSettings.localRouter.errorRateHint")}
                        value={selectedAgentSettings.circuit_error_rate_percent}
                        min={0}
                        max={100}
                        allowZero
                        disabled={busy}
                        onChange={(circuit_error_rate_percent) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            circuit_error_rate_percent,
                          }))
                        }
                      />
                      <NumberField
                        id={`${selectedAgent}-router-min-requests`}
                        label={t("appSettings.localRouter.minRequests")}
                        hint={t("appSettings.localRouter.minRequestsHint")}
                        value={selectedAgentSettings.circuit_min_requests}
                        min={5}
                        max={100}
                        disabled={busy}
                        onChange={(circuit_min_requests) =>
                          updateAgentSettings(selectedAgent, (current) => ({
                            ...current,
                            circuit_min_requests,
                          }))
                        }
                      />
                    </div>
                  </div>

                  {!isAgentSettingsValid(selectedAgentSettings) ? (
                    <div role="alert" style={{ color: "var(--danger)", fontSize: 11 }}>
                      {t("appSettings.localRouter.agentConfigInvalid")}
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.listenSection")}>
          <SectionHeading icon={Network}>
            {t("appSettings.localRouter.listenSection")}
          </SectionHeading>
          <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 116px", gap: 10 }}>
            <div>
              <label htmlFor="local-router-host" style={labelStyle}>
                {t("appSettings.localRouter.host")}
              </label>
              <input
                id="local-router-host"
                style={{
                  ...inputStyle,
                  borderColor: hostInvalid ? "var(--danger)" : "var(--border-medium)",
                  opacity: busy ? 0.6 : 1,
                }}
                value={settings.listen_host}
                disabled={busy}
                aria-invalid={hostInvalid || undefined}
                aria-describedby="local-router-host-hint"
                spellCheck={false}
                onChange={(event) => {
                  const listen_host = event.currentTarget.value;
                  setSettings((previous) => ({ ...previous, listen_host }));
                }}
              />
            </div>
            <div>
              <label htmlFor="local-router-port" style={labelStyle}>
                {t("appSettings.localRouter.port")}
              </label>
              <input
                id="local-router-port"
                style={{
                  ...inputStyle,
                  borderColor: portInvalid ? "var(--danger)" : "var(--border-medium)",
                  opacity: busy ? 0.6 : 1,
                }}
                value={portDraft}
                disabled={busy}
                inputMode="numeric"
                maxLength={5}
                aria-invalid={portInvalid || undefined}
                aria-describedby="local-router-port-hint"
                onChange={(event) => setPortDraft(event.currentTarget.value.replace(/[^0-9]/g, ""))}
              />
            </div>
          </div>
          <div
            id="local-router-host-hint"
            style={{ ...hintStyle, color: hostInvalid ? "var(--danger)" : "var(--text-hint)" }}
          >
            {hostInvalid
              ? t("appSettings.localRouter.hostInvalid")
              : t("appSettings.localRouter.hostHint")}
          </div>
          {portInvalid ? (
            <div id="local-router-port-hint" style={{ ...hintStyle, color: "var(--danger)" }}>
              {t("appSettings.localRouter.portInvalid")}
            </div>
          ) : null}
          {nonLoopbackHost ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 8 }}>
              <div
                role="alert"
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 7,
                  padding: "8px 9px",
                  border: "1px solid color-mix(in srgb, var(--color-warning) 35%, transparent)",
                  borderRadius: "var(--radius-sm)",
                  color: "var(--color-warning)",
                  background: "color-mix(in srgb, var(--color-warning) 8%, transparent)",
                  fontSize: 11,
                  lineHeight: 1.45,
                }}
              >
                <AlertTriangle size={14} strokeWidth={2} style={{ marginTop: 1, flexShrink: 0 }} />
                <span>{t("appSettings.localRouter.nonLoopbackWarning")}</span>
              </div>
              <div>
                <label htmlFor="local-router-access-token" style={labelStyle}>
                  {t("appSettings.localRouter.accessToken")}
                </label>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <input
                    id="local-router-access-token"
                    type={showAccessToken ? "text" : "password"}
                    style={{
                      ...inputStyle,
                      minWidth: 0,
                      borderColor: accessTokenInvalid ? "var(--danger)" : "var(--border-medium)",
                      opacity: busy ? 0.6 : 1,
                    }}
                    value={settings.access_token}
                    disabled={busy}
                    aria-invalid={accessTokenInvalid || undefined}
                    aria-describedby="local-router-access-token-hint"
                    autoComplete="new-password"
                    spellCheck={false}
                    onChange={(event) => {
                      const access_token = event.currentTarget.value;
                      setSettings((previous) => ({ ...previous, access_token }));
                    }}
                  />
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={
                      showAccessToken
                        ? t("appSettings.localRouter.hideAccessToken")
                        : t("appSettings.localRouter.showAccessToken")
                    }
                    title={
                      showAccessToken
                        ? t("appSettings.localRouter.hideAccessToken")
                        : t("appSettings.localRouter.showAccessToken")
                    }
                    disabled={busy}
                    onClick={() => setShowAccessToken((visible) => !visible)}
                  >
                    {showAccessToken ? <EyeOff size={14} /> : <Eye size={14} />}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("appSettings.localRouter.copyAccessToken")}
                    title={t("appSettings.localRouter.copyAccessToken")}
                    disabled={busy || !settings.access_token}
                    onClick={() => {
                      void writeClipboardText(settings.access_token)
                        .then(() => {
                          setAccessTokenCopied(true);
                          window.setTimeout(() => setAccessTokenCopied(false), 2_000);
                        })
                        .catch((cause) => setError(String(cause)));
                    }}
                  >
                    {accessTokenCopied ? <Check size={14} /> : <Copy size={14} />}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("appSettings.localRouter.regenerateAccessToken")}
                    title={t("appSettings.localRouter.regenerateAccessToken")}
                    disabled={busy}
                    onClick={() => {
                      try {
                        const access_token = generateRouterAccessToken();
                        setSettings((previous) => ({ ...previous, access_token }));
                      } catch (cause) {
                        setError(String(cause));
                      }
                    }}
                  >
                    <RefreshCw size={14} />
                  </Button>
                </div>
                <div
                  id="local-router-access-token-hint"
                  style={{
                    ...hintStyle,
                    color: accessTokenInvalid ? "var(--danger)" : "var(--text-hint)",
                  }}
                >
                  {accessTokenInvalid
                    ? t("appSettings.localRouter.accessTokenInvalid")
                    : t("appSettings.localRouter.accessTokenHint")}
                </div>
              </div>
            </div>
          ) : null}
        </section>

        <section aria-label={t("appSettings.localRouter.optionsSection")}>
          <SectionHeading icon={SlidersHorizontal}>
            {t("appSettings.localRouter.optionsSection")}
          </SectionHeading>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <ToggleRow
              icon={House}
              label={t("appSettings.localRouter.showOnHome")}
              checked={settings.show_on_home}
              disabled={busy}
              onChange={(show_on_home) =>
                setSettings((previous) => ({ ...previous, show_on_home }))
              }
            />
            <ToggleRow
              icon={Activity}
              label={t("appSettings.localRouter.recordUsage")}
              hint={t("appSettings.localRouter.recordUsageHint")}
              checked={settings.record_usage}
              disabled={busy}
              onChange={(record_usage) =>
                setSettings((previous) => ({ ...previous, record_usage }))
              }
            />
            <ToggleRow
              icon={Globe2}
              label={t("appSettings.localRouter.useGlobalProxy")}
              hint={t("appSettings.localRouter.useGlobalProxyHint")}
              checked={settings.use_global_proxy}
              disabled={busy}
              onChange={(use_global_proxy) =>
                setSettings((previous) => ({ ...previous, use_global_proxy }))
              }
            />
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.requestStats")}>
          <SectionHeading icon={Gauge}>{t("appSettings.localRouter.requestStats")}</SectionHeading>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
              padding: "7px 0",
              borderTop: "1px solid var(--border-dim)",
              borderBottom: "1px solid var(--border-dim)",
            }}
          >
            <Metric
              icon={Route}
              label={t("appSettings.localRouter.totalRequests")}
              value={status?.total_requests ?? 0}
            />
            <Metric
              icon={CheckCircle2}
              label={t("appSettings.localRouter.successfulRequests")}
              value={status?.successful_requests ?? 0}
            />
            <Metric
              icon={XCircle}
              label={t("appSettings.localRouter.failedRequests")}
              value={status?.failed_requests ?? 0}
            />
            <Metric
              icon={Activity}
              label={t("appSettings.localRouter.activeRequests")}
              value={status?.active_requests ?? 0}
            />
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.tokenUsage")}>
          <SectionHeading icon={DatabaseZap}>
            {t("appSettings.localRouter.tokenUsage")}
          </SectionHeading>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
              padding: "7px 0",
              borderTop: "1px solid var(--border-dim)",
              borderBottom: "1px solid var(--border-dim)",
            }}
          >
            <Metric
              icon={Bot}
              label={t("appSettings.localRouter.inputTokens")}
              value={status?.input_tokens ?? 0}
            />
            <Metric
              icon={Code2}
              label={t("appSettings.localRouter.outputTokens")}
              value={status?.output_tokens ?? 0}
            />
            <Metric
              icon={Activity}
              label={t("appSettings.localRouter.cacheReadTokens")}
              value={status?.cache_read_tokens ?? 0}
            />
            <Metric
              icon={DatabaseZap}
              label={t("appSettings.localRouter.cacheCreationTokens")}
              value={status?.cache_creation_tokens ?? 0}
            />
          </div>
        </section>

        <section aria-label={t("appSettings.localRouter.requestsSection")}>
          <SectionHeading
            icon={ScrollText}
            action={
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label={t("appSettings.localRouter.refreshRequests")}
                title={t("appSettings.localRouter.refreshRequests")}
                disabled={requestsLoading}
                onClick={() => void refreshRequests(true)}
              >
                <RefreshCw
                  size={12}
                  strokeWidth={2}
                  style={{ animation: requestsLoading ? "spin 0.8s linear infinite" : undefined }}
                />
              </Button>
            }
          >
            {t("appSettings.localRouter.requestsSection")}
          </SectionHeading>
          <div style={{ ...hintStyle, marginTop: -4, marginBottom: 8 }}>
            {t("appSettings.localRouter.requestsHint")}
          </div>
          {requestsLoading && !requests.length ? (
            <div style={{ color: "var(--text-hint)", fontSize: 11 }}>
              {t("common.loadingEllipsis")}
            </div>
          ) : requests.length ? (
            <div role="list" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {requests.map((request) => (
                <RequestRecordRow key={request.requestId} request={request} locale={locale} t={t} />
              ))}
            </div>
          ) : (
            <div
              style={{
                ...insetPanelStyle,
                borderStyle: "dashed",
                color: "var(--text-hint)",
                fontSize: 11,
                textAlign: "center",
              }}
            >
              {t("appSettings.localRouter.requestsEmpty")}
            </div>
          )}
        </section>
      </div>

      <div style={s.settingsFooter}>
        {configurationInvalid ? (
          <span style={{ color: "var(--danger)", fontSize: 11 }}>
            {t("appSettings.localRouter.configurationInvalid")}
          </span>
        ) : saved ? (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              color: "var(--success)",
              fontSize: 12,
            }}
          >
            <Check size={12} /> {t("common.saved")}
          </span>
        ) : null}
        <Button
          variant="default"
          size="sm"
          disabled={busy || !dirty || configurationInvalid}
          onClick={() => void handleSave()}
        >
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </>
  );
}
