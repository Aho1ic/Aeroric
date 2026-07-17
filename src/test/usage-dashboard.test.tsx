import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "../components/UsageDashboard";
import { I18nProvider } from "../i18n";
import type {
  UsageStatistics,
  UsageStatisticsAgent,
  UsageStatisticsRange,
  UsageStatisticsTotals,
} from "../types";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const totals: UsageStatisticsTotals = {
  totalTokens: 1500,
  inputTokens: 500,
  outputTokens: 300,
  cacheCreationTokens: 200,
  cacheReadTokens: 500,
  cacheHitRate: 5 / 12,
  requestCount: 12,
  totalCost: 0.1234,
  pricedRequestCount: 10,
  unpricedRequestCount: 2,
};

function result(rangeDays: UsageStatisticsRange, agent: UsageStatisticsAgent): UsageStatistics {
  return {
    rangeDays,
    from: "2026-07-11",
    to: "2026-07-17",
    agent,
    updatedAt: Date.UTC(2026, 6, 17, 12, 0, 0),
    totals,
    series: Array.from({ length: rangeDays }, (_, index) => ({
      date: `2026-07-${String(17 - (rangeDays - index - 1)).padStart(2, "0")}`,
      ...totals,
    })),
    breakdown: {
      codex: { ...totals, totalTokens: 1000, requestCount: 8 },
      claude: { ...totals, totalTokens: 500, requestCount: 4 },
    },
  };
}

describe("UsageDashboard", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invokeMock.mockReset();
    invokeMock.mockImplementation(
      async (
        command: string,
        args: { rangeDays: UsageStatisticsRange; agent: UsageStatisticsAgent },
      ) => {
        if (command === "refresh_usage_statistics_index") return false;
        expect(command).toBe("read_usage_statistics");
        return result(args.rangeDays, args.agent);
      },
    );
  });

  it("renders usage metrics and updates range and agent filters", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider>
        <UsageDashboard />
      </I18nProvider>,
    );

    expect(await screen.findByRole("heading", { name: "Usage statistics" })).toBeInTheDocument();
    expect(screen.getAllByText("1,500").length).toBeGreaterThan(0);
    expect(screen.queryByText("1.5K")).not.toBeInTheDocument();
    expect(screen.getByText("41.7%")).toBeInTheDocument();
    expect(screen.getByText("$0.1234")).toBeInTheDocument();
    expect(screen.getByText("Daily usage")).toBeInTheDocument();

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("read_usage_statistics", {
        rangeDays: 7,
        agent: "all",
      });
    });

    await user.click(screen.getByRole("button", { name: "14d" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("read_usage_statistics", {
        rangeDays: 14,
        agent: "all",
      });
    });

    await user.click(screen.getByRole("button", { name: "Codex" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("read_usage_statistics", {
        rangeDays: 14,
        agent: "codex",
      });
    });
  });

  it("formats currency against the selected UI language, not the OS locale", async () => {
    localStorage.setItem("aeroric:language", "zh");
    render(
      <I18nProvider>
        <UsageDashboard />
      </I18nProvider>,
    );

    // zh-CN renders USD as "US$0.1234"; en-US renders "$0.1234". Asserting the
    // zh form guards against formatters falling back to the host OS locale
    // (the previous bug rendered "US$" even when the UI was English).
    expect(await screen.findByText("US$0.1234")).toBeInTheDocument();
    expect(screen.queryByText("$0.1234")).not.toBeInTheDocument();
  });

  it("keeps the date range on one line and limits bars for short ranges", async () => {
    localStorage.setItem("aeroric:language", "zh");
    const user = userEvent.setup();
    const { container } = render(
      <I18nProvider>
        <UsageDashboard />
      </I18nProvider>,
    );

    const dateRange = await screen.findByText("2026年7月11日 – 2026年7月17日");
    expect(dateRange).toHaveStyle({ whiteSpace: "nowrap" });

    const bars = Array.from(container.querySelectorAll<HTMLElement>(".usage-chart-bar"));
    expect(bars).toHaveLength(7);
    expect(bars.every((bar) => bar.style.maxWidth === "28px")).toBe(true);

    await user.click(screen.getByRole("button", { name: "当天" }));
    await waitFor(() => {
      const todayBars = Array.from(container.querySelectorAll<HTMLElement>(".usage-chart-bar"));
      expect(todayBars).toHaveLength(1);
      expect(todayBars[0]).toHaveStyle({ maxWidth: "32px" });
    });
  });
});
