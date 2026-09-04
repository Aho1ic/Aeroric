import { describe, expect, it } from "vitest";
import { chatCompletionsProxyPayload } from "./agent-config-payload";

describe("mobile agent configuration payload", () => {
  it("sends the bridge field for Codex profiles, including an explicit disable", () => {
    expect(chatCompletionsProxyPayload("codex", true)).toEqual({
      enableChatCompletionsProxy: true,
    });
    expect(chatCompletionsProxyPayload("codex", false)).toEqual({
      enableChatCompletionsProxy: false,
    });
  });

  it("omits the Codex-only bridge field for Claude and DSH profiles", () => {
    expect(chatCompletionsProxyPayload("claude_code", false)).toEqual({});
    expect(chatCompletionsProxyPayload("dsh", false)).toEqual({});
  });
});
