import { describe, expect, it } from "vitest";
import { formatAgentBalance } from "../components/app-settings/types";

describe("formatAgentBalance", () => {
  it("formats limited and unlimited API key balances in USD", () => {
    expect(formatAgentBalance({ used: 57.25, total: 100 }, "en")).toBe("$57.25 / $100.00");
    expect(formatAgentBalance({ used: 57.25, total: null }, "en")).toBe("$57.25 / Unlimited");
    expect(formatAgentBalance({ used: 57.25, total: null }, "zh")).toBe("US$57.25 / 无限制");
  });
});
