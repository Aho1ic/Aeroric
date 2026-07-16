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
    from: "2026-07-10",
    to: "2026-07-16",
    agent,
    totals,
    series: Array.from({ length: rangeDays }, (_, index) => ({
      date: `2026-07-${String(16 - (rangeDays - index - 1)).padStart(2, "0")}`,
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
});
