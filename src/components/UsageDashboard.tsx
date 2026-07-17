import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ChartNoAxesCombined,
  CircleGauge,
  Coins,
  DatabaseZap,
  Info,
  Layers3,
  RefreshCw,
  Sigma,
  Sparkles,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { useUsageStatistics } from "../hooks/useUsageStatistics";
import { useI18n, type AppLanguage } from "../i18n";
import s from "../styles";
import type {
  UsageStatisticsAgent,
  UsageStatisticsDay,
  UsageStatisticsRange,
  UsageStatisticsTotals,
} from "../types";
import { Button } from "./ui/Button";

const RANGE_OPTIONS: UsageStatisticsRange[] = [1, 7, 14, 30];
const AGENT_OPTIONS: UsageStatisticsAgent[] = ["all", "codex", "claude"];

// Format numbers/currency/dates against the user's chosen UI language rather
// than the OS locale, so an English UI never renders "US$0.12" / localized
// dates just because the host machine is set to another region.
function localeForLanguage(language: AppLanguage): string {
  return language === "zh" ? "zh-CN" : "en-US";
}

function formatInteger(locale: string, value: number): string {
  return new Intl.NumberFormat(locale, {
    notation: "standard",
    maximumFractionDigits: 0,
    useGrouping: true,
  }).format(value);
}

function formatCost(locale: string, value: number): string {
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: value > 0 && value < 1 ? 3 : 2,
    maximumFractionDigits: value > 0 && value < 1 ? 4 : 2,
  }).format(value);
}

function formatDate(locale: string, value: string, includeYear = false): string {
  const date = new Date(`${value}T00:00:00`);
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    year: includeYear ? "numeric" : undefined,
  }).format(date);
}

function formatUpdatedTime(locale: string, value: number): string {
  return new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

function Segment<T extends string | number>({
  ariaLabel,
  value,
  options,
  label,
  onChange,
}: {
  ariaLabel: string;
  value: T;
  options: readonly T[];
  label: (option: T) => string;
  onChange: (option: T) => void;
}) {
  return (
    <div role="group" aria-label={ariaLabel} style={s.usageSegment}>
      {options.map((option) => (
        <Button
          key={option}
          size="xs"
          variant="ghost"
          active={option === value}
          aria-pressed={option === value}
          onClick={() => onChange(option)}
          style={{ minWidth: option === "claude" ? 58 : 42 }}
        >
          {label(option)}
        </Button>
      ))}
    </div>
  );
}

function MetricCard({
  label,
  value,
  icon,
  color,
  index,
}: {
  label: string;
  value: string;
  icon: React.ReactNode;
  color: string;
  index: number;
}) {
  const valueFontSize =
    value.length > 18 ? 14 : value.length > 14 ? 16 : value.length > 10 ? 18 : 21;
  return (
    <section
      className="usage-metric-card"
      style={{ ...s.usageMetricCard, animationDelay: `${index * 35}ms` }}
    >
      <div style={s.usageMetricHead}>
        <span style={s.usageMetricLabel}>{label}</span>
        <span
          style={{
            ...s.usageMetricIcon,
            color,
            background: `color-mix(in srgb, ${color} 10%, transparent)`,
          }}
        >
          {icon}
        </span>
      </div>
      <div style={{ ...s.usageMetricValue, fontSize: valueFontSize }} title={value}>
        {value}
      </div>
    </section>
  );
}

function ChartLegend({ label, color }: { label: string; color: string }) {
  return (
    <span style={s.usageLegendItem}>
      <span style={{ ...s.usageLegendSwatch, background: color }} />
      {label}
    </span>
  );
}

function UsageChart({
  series,
  labels,
  locale,
}: {
  series: UsageStatisticsDay[];
  labels: {
    total: string;
    input: string;
    output: string;
    cacheCreation: string;
    cacheRead: string;
  };
  locale: string;
}) {
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const max = Math.max(1, ...series.map((day) => day.totalTokens));
  const labelEvery = series.length <= 7 ? 1 : series.length <= 14 ? 2 : 5;
  const compactBars = series.length <= 7;
  const barMaxWidth = series.length === 1 ? 32 : compactBars ? 28 : 24;
  const columnWidth = series.length === 1 ? 72 : compactBars ? 52 : undefined;
  const minPlotWidth = compactBars ? 260 : Math.max(360, series.length * 25);
  const activeDay = activeIndex === null ? null : series[activeIndex];

  return (
    <div style={s.usageChartViewport}>
      <div style={{ ...s.usageChart, minWidth: minPlotWidth + 96 }}>
        <div style={s.usageChartAxis} aria-hidden="true">
          <span>{formatInteger(locale, max)}</span>
          <span>{formatInteger(locale, Math.round(max * 0.75))}</span>
          <span>{formatInteger(locale, Math.round(max * 0.5))}</span>
          <span>{formatInteger(locale, Math.round(max * 0.25))}</span>
          <span>0</span>
        </div>
        <div
          style={{
            ...s.usageChartPlot,
            minWidth: minPlotWidth,
            justifyContent: compactBars ? "center" : "stretch",
          }}
        >
          {series.map((day, index) => {
            const height = day.totalTokens === 0 ? 0 : Math.max(2, (day.totalTokens / max) * 192);
            const title = [
              formatDate(locale, day.date, true),
              `${labels.input}: ${formatInteger(locale, day.inputTokens)}`,
              `${labels.output}: ${formatInteger(locale, day.outputTokens)}`,
              `${labels.cacheCreation}: ${formatInteger(locale, day.cacheCreationTokens)}`,
              `${labels.cacheRead}: ${formatInteger(locale, day.cacheReadTokens)}`,
            ].join("\n");
            const segment = (value: number, color: string): CSSProperties => ({
              height: day.totalTokens === 0 ? 0 : `${(value / day.totalTokens) * 100}%`,
              minHeight: value > 0 ? 1 : 0,
              background: color,
            });

            return (
              <div
                key={day.date}
                className="usage-chart-column"
                style={{
                  ...s.usageChartColumn,
                  flex: columnWidth ? `0 0 ${columnWidth}px` : "1 1 0",
                  maxWidth: columnWidth,
                }}
                tabIndex={0}
                aria-label={title.split("\n").join(", ")}
                onMouseEnter={() => setActiveIndex(index)}
                onMouseLeave={() => setActiveIndex(null)}
                onFocus={() => setActiveIndex(index)}
                onBlur={() => setActiveIndex(null)}
              >
                <div
                  className="usage-chart-bar"
                  style={{
                    ...s.usageChartBar,
                    width: "100%",
                    maxWidth: barMaxWidth,
                    height,
                  }}
                >
                  <span style={segment(day.inputTokens, "var(--accent)")} />
                  <span style={segment(day.outputTokens, "var(--success)")} />
                  <span style={segment(day.cacheCreationTokens, "var(--warning)")} />
                  <span style={segment(day.cacheReadTokens, "var(--icon-file-ts)")} />
                </div>
                <div style={s.usageChartLabel}>
                  {index % labelEvery === 0 || index === series.length - 1
                    ? formatDate(locale, day.date)
                    : ""}
                </div>
              </div>
            );
          })}
        </div>
      </div>
      {activeDay && (
        <div style={s.usageChartTooltip} role="status">
          <div style={s.usageChartTooltipHead}>
            <strong>{formatDate(locale, activeDay.date, true)}</strong>
            <span>{formatInteger(locale, activeDay.totalTokens)}</span>
          </div>
          {[
            { label: labels.input, value: activeDay.inputTokens, color: "var(--accent)" },
            { label: labels.output, value: activeDay.outputTokens, color: "var(--success)" },
            {
              label: labels.cacheCreation,
              value: activeDay.cacheCreationTokens,
              color: "var(--warning)",
            },
            {
              label: labels.cacheRead,
              value: activeDay.cacheReadTokens,
              color: "var(--icon-file-ts)",
            },
          ].map((item) => (
            <div key={item.label} style={s.usageChartTooltipRow}>
              <span style={{ ...s.usageLegendSwatch, background: item.color }} />
              <span>{item.label}</span>
              <strong>{formatInteger(locale, item.value)}</strong>
            </div>
          ))}
          <div style={s.usageChartTooltipTotal}>
            <span>{labels.total}</span>
            <strong>{formatInteger(locale, activeDay.totalTokens)}</strong>
          </div>
        </div>
      )}
    </div>
  );
}

function SourceSummary({
  codex,
  claude,
  tokenLabel,
  requestLabel,
  title,
  locale,
}: {
  codex: UsageStatisticsTotals;
  claude: UsageStatisticsTotals;
  tokenLabel: string;
  requestLabel: string;
  title: string;
  locale: string;
}) {
  const max = Math.max(1, codex.totalTokens, claude.totalTokens);
  return (
    <section className="usage-panel" style={s.usageSourceSummary}>
      <div style={s.usageSectionTitle}>{title}</div>
      <div style={s.usageSourceList}>
        {[
          { name: "Codex", totals: codex, color: "var(--accent)" },
          { name: "Claude", totals: claude, color: "var(--success)" },
        ].map((item) => (
          <div key={item.name} style={s.usageSourceRow}>
            <strong>{item.name}</strong>
            <div style={s.usageSourceTrack}>
              <div
                className="usage-source-fill"
                style={{
                  ...s.usageSourceFill,
                  width: `${(item.totals.totalTokens / max) * 100}%`,
                  background: item.color,
                }}
              />
            </div>
            <span style={s.usageSourceValue}>
              {formatInteger(locale, item.totals.totalTokens)} {tokenLabel}
            </span>
            <span style={s.usageSourceValue}>
              {formatInteger(locale, item.totals.requestCount)} {requestLabel}
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}

export function UsageDashboard({ embedded = false }: { embedded?: boolean }) {
  const { t, language } = useI18n();
  const locale = localeForLanguage(language);
  const [rangeDays, setRangeDays] = useState<UsageStatisticsRange>(7);
  const [agent, setAgent] = useState<UsageStatisticsAgent>("all");
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(900);
  const { statistics, loading, refreshing, error, refetch } = useUsageStatistics(rangeDays, agent);

  useEffect(() => {
    const root = rootRef.current;
    if (!root || typeof ResizeObserver === "undefined") return;
    const update = () => setWidth(root.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  const columns = width < 520 ? 1 : width < 860 ? 2 : 4;
  const metrics = useMemo(() => {
    const totals = statistics?.totals;
    return [
      {
        label: t("usageStats.totalTokens"),
        value: formatInteger(locale, totals?.totalTokens ?? 0),
        icon: <Sigma size={14} />,
        color: "var(--accent)",
      },
      {
        label: t("usageStats.inputTokens"),
        value: formatInteger(locale, totals?.inputTokens ?? 0),
        icon: <ArrowDownToLine size={14} />,
        color: "var(--icon-file-ts)",
      },
      {
        label: t("usageStats.outputTokens"),
        value: formatInteger(locale, totals?.outputTokens ?? 0),
        icon: <ArrowUpFromLine size={14} />,
        color: "var(--success)",
      },
      {
        label: t("usageStats.cacheCreation"),
        value: formatInteger(locale, totals?.cacheCreationTokens ?? 0),
        icon: <DatabaseZap size={14} />,
        color: "var(--warning)",
      },
      {
        label: t("usageStats.cacheRead"),
        value: formatInteger(locale, totals?.cacheReadTokens ?? 0),
        icon: <Layers3 size={14} />,
        color: "var(--usage-codex)",
      },
      {
        label: t("usageStats.cacheHitRate"),
        value: `${((totals?.cacheHitRate ?? 0) * 100).toFixed(1)}%`,
        icon: <CircleGauge size={14} />,
        color: "var(--success)",
      },
      {
        label: t("usageStats.requests"),
        value: formatInteger(locale, totals?.requestCount ?? 0),
        icon: <Sparkles size={14} />,
        color: "var(--accent)",
      },
      {
        label: t("usageStats.estimatedCost"),
        value: formatCost(locale, totals?.totalCost ?? 0),
        icon: <Coins size={14} />,
        color: "var(--warning)",
      },
    ];
  }, [statistics?.totals, t, locale]);

  const dateRange = statistics
    ? `${formatDate(locale, statistics.from, true)} – ${formatDate(locale, statistics.to, true)}`
    : t("usageStats.rangePending");
  const updateLabel =
    statistics?.updatedAt && statistics.updatedAt > 0
      ? t("usageStats.updatedAt", { time: formatUpdatedTime(locale, statistics.updatedAt) })
      : t("usageStats.indexing");

  return (
    <div ref={rootRef} style={s.usageDashboard}>
      <header
        style={{
          ...s.usageDashboardHeader,
          flexDirection: width < 700 ? "column" : "row",
          padding: embedded ? "14px 20px" : s.usageDashboardHeader.padding,
        }}
      >
        {embedded ? (
          <div style={s.usageDashboardRange}>{dateRange}</div>
        ) : (
          <div>
            <h1 style={s.usageDashboardTitle}>{t("usageStats.title")}</h1>
            <div style={s.usageDashboardRange}>{dateRange}</div>
          </div>
        )}
        <div style={s.usageHeaderRight}>
          <div style={s.usageLiveStatus}>
            <span className="usage-live-dot" />
            <span style={s.usageLiveText}>
              <strong>{t("usageStats.live")}</strong>
              <span>{updateLabel}</span>
            </span>
          </div>
          <div
            style={{
              ...s.usageDashboardControls,
              justifyContent: width < 700 ? "flex-start" : "flex-end",
            }}
          >
            <Segment
              ariaLabel={t("usageStats.rangeFilter")}
              value={rangeDays}
              options={RANGE_OPTIONS}
              label={(option) =>
                option === 1 ? t("usageStats.today") : t("usageStats.days", { days: option })
              }
              onChange={setRangeDays}
            />
            <Segment
              ariaLabel={t("usageStats.agentFilter")}
              value={agent}
              options={AGENT_OPTIONS}
              label={(option) => t(`usageStats.agent.${option}`)}
              onChange={setAgent}
            />
            <Button
              size="icon-sm"
              variant="outline"
              aria-label={t("common.refresh")}
              title={t("common.refresh")}
              disabled={refreshing}
              onClick={() => void refetch()}
            >
              <RefreshCw className={refreshing ? "spin" : undefined} size={13} />
            </Button>
          </div>
        </div>
      </header>

      <div style={s.usageDashboardScroll}>
        {error && !statistics ? (
          <div style={s.usageState}>
            <ChartNoAxesCombined size={28} strokeWidth={1.5} />
            <span>{t("usageStats.failed", { error })}</span>
            <Button size="sm" variant="outline" onClick={() => void refetch()}>
              {t("common.retry")}
            </Button>
          </div>
        ) : loading && !statistics ? (
          <div style={s.usageState}>
            <RefreshCw size={24} strokeWidth={1.5} />
            <span>{t("usageStats.loading")}</span>
          </div>
        ) : statistics ? (
          <>
            <div
              style={{
                ...s.usageMetricGrid,
                gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                opacity: loading ? 0.72 : 1,
              }}
            >
              {metrics.map((metric, index) => (
                <MetricCard key={metric.label} {...metric} index={index} />
              ))}
            </div>

            <section className="usage-panel" style={s.usageChartSection}>
              <div
                style={{
                  ...s.usageSectionHead,
                  alignItems: width < 600 ? "flex-start" : "center",
                  flexDirection: width < 600 ? "column" : "row",
                }}
              >
                <div style={s.usageSectionTitle}>{t("usageStats.dailyUsage")}</div>
                <div style={s.usageLegend}>
                  <ChartLegend label={t("usageStats.inputTokens")} color="var(--accent)" />
                  <ChartLegend label={t("usageStats.outputTokens")} color="var(--success)" />
                  <ChartLegend label={t("usageStats.cacheCreation")} color="var(--warning)" />
                  <ChartLegend label={t("usageStats.cacheRead")} color="var(--icon-file-ts)" />
                </div>
              </div>

              {statistics.totals.requestCount === 0 ? (
                <div style={{ ...s.usageState, minHeight: 190 }}>
                  <ChartNoAxesCombined size={28} strokeWidth={1.5} />
                  <span>{t("usageStats.empty")}</span>
                </div>
              ) : (
                <UsageChart
                  series={statistics.series}
                  labels={{
                    total: t("usageStats.totalTokens"),
                    input: t("usageStats.inputTokens"),
                    output: t("usageStats.outputTokens"),
                    cacheCreation: t("usageStats.cacheCreation"),
                    cacheRead: t("usageStats.cacheRead"),
                  }}
                  locale={locale}
                />
              )}
            </section>

            {agent === "all" && (
              <SourceSummary
                codex={statistics.breakdown.codex}
                claude={statistics.breakdown.claude}
                tokenLabel={t("usageStats.tokensShort")}
                requestLabel={t("usageStats.requestsShort")}
                title={t("usageStats.sourceBreakdown")}
                locale={locale}
              />
            )}

            <div style={s.usageCostNote}>
              <Info size={13} style={{ flexShrink: 0, marginTop: 1 }} />
              <span>
                {t("usageStats.costNote")}
                {statistics.totals.unpricedRequestCount > 0
                  ? ` ${t("usageStats.unpricedNote", {
                      count: statistics.totals.unpricedRequestCount,
                    })}`
                  : ""}
              </span>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
