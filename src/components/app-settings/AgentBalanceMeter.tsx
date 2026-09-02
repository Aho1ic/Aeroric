/**
 * API key 额度的展示胶囊:一行「已用 / 总额」加一条进度条。
 *
 * 「获取可用模型」成功后,三处(新增面板 / 配置面板 / 详情弹窗)都要显示同一份额度,
 * 原先各自内联一套一模一样的样式、只有 `minHeight` 与 `marginTop` 有差。收成一个组件,
 * 那两个差异留成 props。
 *
 * 颜色沿用仓库既有的 `getUsageColor` —— 它吃的是**剩余**百分比,不是已用,所以这里传
 * `100 - 已用%`。不另起一套阈值是有意的:用量色在别处(任务栏、用量面板)已经是这三档,
 * 额度条再自定义一套会让同一个绿色在两处表示不同的宽裕程度。
 */

import type { CSSProperties } from "react";

import { useI18n } from "../../i18n";
import { getUsageColor } from "../../utils";
import {
  agentBalanceUsedPercent,
  formatAgentBalance,
  formatAgentBalanceDisplay,
  type AgentBalance,
} from "./types";

/** 条子留得下刻度的最小宽度;比这更窄时百分比和条会挤在一起。 */
const METER_MIN_WIDTH = 208;

export function AgentBalanceMeter({
  balance,
  style,
}: {
  balance: AgentBalance;
  /** 各调用点的位置微调(`marginTop` / `minHeight` 之类),其余样式不开放。 */
  style?: CSSProperties;
}) {
  // 语言取自 `useI18n()` 而不是 prop:调用方的 `language` 本来就来自同一个 hook,
  // 多一个 prop 只是多一处能与 `t()` 不一致的地方。
  const { t, language } = useI18n();
  const display = formatAgentBalanceDisplay(balance, language);
  // 精确值只进 title 与 aria:紧凑记数会把 $5,114,182.00 显示成 $5.11M,
  // 想看准数的人得有地方看到。
  const exact = formatAgentBalance(balance, language);
  const usedPercent = agentBalanceUsedPercent(balance);
  const label = t("appSettings.keyBalanceLabel");

  return (
    <div
      role="status"
      // 标点交给文案表:中文用全角冒号,在组件里拼 `${label}: ` 会得到半角。
      title={t("appSettings.keyBalanceExact", { amount: exact })}
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 5,
        minWidth: METER_MIN_WIDTH,
        padding: "6px 9px",
        border: "1px solid color-mix(in srgb, var(--success) 26%, var(--border-medium))",
        borderRadius: "var(--radius-sm)",
        background: "color-mix(in srgb, var(--success) 7%, transparent)",
        fontSize: 11.5,
        fontVariantNumeric: "tabular-nums",
        ...style,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ color: "var(--text-secondary)", whiteSpace: "nowrap" }}>{label}</span>
        <span
          style={{
            marginLeft: "auto",
            color: "var(--text-primary)",
            fontWeight: 600,
            whiteSpace: "nowrap",
          }}
        >
          {display.used} <span style={{ color: "var(--text-hint)", fontWeight: 400 }}>/</span>{" "}
          {display.total}
        </span>
      </div>
      {usedPercent !== null && <BalanceBar usedPercent={usedPercent} exact={exact} label={label} />}
    </div>
  );
}

function BalanceBar({
  usedPercent,
  exact,
  label,
}: {
  usedPercent: number;
  exact: string;
  label: string;
}) {
  const { t } = useI18n();
  // 条宽截到 [0, 100];超额时百分比文字照实显示(见 `agentBalanceUsedPercent`),
  // 但条不能画出轨道外面去。取两位小数只是别让内联样式里出现 36.64999702954964%,
  // 渲染上没有区别。
  const width = Number(Math.min(100, Math.max(0, usedPercent)).toFixed(2));
  const color = getUsageColor(100 - usedPercent);
  // 小数位跟着量级走:99.94% 与 100% 的区别有意义,3% 与 3.2% 的没有。
  const percentText = t("appSettings.keyBalancePercentUsed", {
    percent:
      usedPercent >= 99.9 && usedPercent < 100 ? usedPercent.toFixed(2) : usedPercent.toFixed(1),
  });

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <div
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={width}
        aria-valuetext={exact}
        style={{
          flex: 1,
          height: 4,
          overflow: "hidden",
          borderRadius: 2,
          background: "var(--border-dim)",
        }}
      >
        <div
          style={{
            width: `${width}%`,
            height: "100%",
            background: color,
            borderRadius: 2,
            transition: "width 160ms ease",
          }}
        />
      </div>
      <span style={{ color, fontWeight: 600, whiteSpace: "nowrap" }}>{percentText}</span>
    </div>
  );
}
