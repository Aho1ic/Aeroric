import { describe, expect, it } from "vitest";
import { compactTerminalLabel, formatTerminalTabLabel } from "../components/terminalTabLabel";

describe("terminal tab labels", () => {
  it("shortens Windows PowerShell while preserving the session number", () => {
    expect(formatTerminalTabLabel("Windows PowerShell", 0)).toBe("PowerShell 1");
    expect(formatTerminalTabLabel("Windows PowerShell", 1)).toBe("PowerShell 2");
  });

  it("keeps other shell names and handles blank labels", () => {
    expect(compactTerminalLabel("Command Prompt")).toBe("Command Prompt");
    expect(formatTerminalTabLabel("", 2)).toBe("Shell 3");
    expect(formatTerminalTabLabel("SSH")).toBe("SSH");
  });
});
