/**
 * 额度胶囊:紧凑记数、百分比、进度条宽度与配色。
 *
 * 用户看到的那串 `US$5,114,182.00 / US$13,954,113.00` 是本文件的起点 —— 大额下
 * 必须换紧凑记数,同时精确值不能丢(它进 `title` 与 `aria-valuetext`)。
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AgentBalanceMeter } from "../components/app-settings/AgentBalanceMeter";
import {
  agentBalanceUsedPercent,
  formatAgentBalanceDisplay,
  type AgentBalance,
} from "../components/app-settings/types";
import { I18nProvider } from "../i18n";

function renderMeter(balance: AgentBalance) {
  return render(
    <I18nProvider>
      <AgentBalanceMeter balance={balance} />
    </I18nProvider>,
  );
}

/** 轨道是 progressbar,填充是它唯一的子元素。 */
function barFillWidth(): string {
  const track = screen.getByRole("progressbar");
  const fill = track.firstElementChild as HTMLElement | null;
  if (!fill) throw new Error("进度条没有填充子元素");
  return fill.style.width;
}

describe("formatAgentBalanceDisplay", () => {
  it("keeps sub-million amounts exact", () => {
    expect(formatAgentBalanceDisplay({ used: 57.25, total: 100 }, "en")).toEqual({
      used: "$57.25",
      total: "$100.00",
    });
  });

  /** 用户报的就是这一档:七位数下逐位显示既排不下也读不出量级。 */
  it("switches to compact notation once either side reaches a million", () => {
    expect(formatAgentBalanceDisplay({ used: 5_114_182, total: 13_954_113 }, "en")).toEqual({
      used: "$5.11M",
      total: "$13.95M",
    });
  });

  /** 量级词是分语言的,所以不能自己除 1e6 再拼 "M"。 */
  it("uses the locale's own magnitude words", () => {
    expect(formatAgentBalanceDisplay({ used: 5_114_182, total: 13_954_113 }, "zh")).toEqual({
      used: "US$511.42万",
      total: "US$1395.41万",
    });
  });

  /**
   * 门槛看 used 与 total 的较大者:已用很小但总额上百万时两边要同档,
   * 否则会出现 `$1.00 / $13.95M` 这种一边精确一边紧凑的错位。
   */
  it("picks the notation from the larger side so both sides match", () => {
    expect(formatAgentBalanceDisplay({ used: 1, total: 13_954_113 }, "en")).toEqual({
      used: "$1.00",
      total: "$13.95M",
    });
  });

  it("renders an unlimited total in the current language", () => {
    expect(formatAgentBalanceDisplay({ used: 12, total: null }, "en").total).toBe("Unlimited");
    expect(formatAgentBalanceDisplay({ used: 12, total: null }, "zh").total).toBe("无限制");
  });
});

describe("agentBalanceUsedPercent", () => {
  it("computes the used share", () => {
    expect(agentBalanceUsedPercent({ used: 25, total: 100 })).toBe(25);
    expect(agentBalanceUsedPercent({ used: 5_114_182, total: 13_954_113 })).toBeCloseTo(36.65, 2);
  });

  /** 没有上限就没有「占比」这回事,调用方据此不画条。 */
  it("returns null when there is nothing to divide by", () => {
    expect(agentBalanceUsedPercent({ used: 12, total: null })).toBeNull();
    expect(agentBalanceUsedPercent({ used: 12, total: 0 })).toBeNull();
    expect(agentBalanceUsedPercent({ used: 12, total: -5 })).toBeNull();
    expect(agentBalanceUsedPercent({ used: Number.NaN, total: 100 })).toBeNull();
    expect(agentBalanceUsedPercent({ used: -1, total: 100 })).toBeNull();
  });

  /** 超额不截:105% 是真实状态,截成 100% 会把「已经超了」这件事藏起来。 */
  it("reports overage above 100", () => {
    expect(agentBalanceUsedPercent({ used: 120, total: 100 })).toBe(120);
  });
});

describe("AgentBalanceMeter", () => {
  it("shows the compact pair, the percent, and keeps the exact amount reachable", () => {
    const { container } = renderMeter({ used: 5_114_182, total: 13_954_113 });

    expect(screen.getByText(/\$5\.11M/)).toBeInTheDocument();
    expect(screen.getByText(/\$13\.95M/)).toBeInTheDocument();
    expect(screen.getByText("36.6% used")).toBeInTheDocument();

    // 精确值走 title 与 aria-valuetext,紧凑记数不该让准数无处可查。
    const pill = container.querySelector('[role="status"]') as HTMLElement;
    expect(pill.title).toContain("$5,114,182.00 / $13,954,113.00");
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuetext",
      "$5,114,182.00 / $13,954,113.00",
    );
  });

  it("sizes the fill to the used share", () => {
    renderMeter({ used: 25, total: 100 });
    expect(barFillWidth()).toBe("25%");
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "25");
  });

  /** 条不能画到轨道外面,但文字要照实说超了多少。 */
  it("caps the fill at full while still reporting the overage", () => {
    renderMeter({ used: 120, total: 100 });
    expect(barFillWidth()).toBe("100%");
    expect(screen.getByText("120.0% used")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "100");
  });

  /** 无上限时百分比无意义 —— 不画条,但金额照显示。 */
  it("omits the bar when the key has no limit", () => {
    renderMeter({ used: 57.25, total: null });
    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(screen.getByText(/Unlimited/)).toBeInTheDocument();
    expect(screen.getByText(/\$57\.25/)).toBeInTheDocument();
  });

  /**
   * 配色沿用 `getUsageColor`(吃剩余百分比)的三档。这里锁的是「越接近用尽越告警」
   * 这个方向,以及三档确实是三个不同的值 —— 具体色值由 getUsageColor 自己的约定决定。
   */
  it("moves through the usage color tiers as the key gets consumed", () => {
    const colorFor = (used: number) => {
      const { unmount } = renderMeter({ used, total: 100 });
      const color = (screen.getByRole("progressbar").firstElementChild as HTMLElement).style
        .background;
      unmount();
      return color;
    };

    const plenty = colorFor(10); // 剩 90% → good
    const middle = colorFor(50); // 剩 50% → warn
    const nearly = colorFor(95); // 剩 5%  → danger

    expect(plenty).toContain("--usage-good");
    expect(middle).toContain("--usage-warn");
    expect(nearly).toContain("--usage-danger");
  });

  /** 99.9x% 不能四舍五入成 100% —— 「快用完」和「已用完」是两种处置。 */
  it("keeps an extra digit just below full so it never reads as exhausted", () => {
    renderMeter({ used: 99.94, total: 100 });
    expect(screen.getByText("99.94% used")).toBeInTheDocument();
  });
});
