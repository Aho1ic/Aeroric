import { describe, expect, it, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import { handleTerminalContextMenu } from "../components/terminalCopyHelper";

function selectedTerminal(text: string): Terminal {
  return {
    hasSelection: () => true,
    getSelection: () => text,
    focus: vi.fn(),
    buffer: {
      active: {
        getLine: () => null,
      },
    },
  } as unknown as Terminal;
}

describe("terminal context menu", () => {
  it("pastes selected terminal text into the input line on right click", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const readText = vi.fn().mockResolvedValue("pasted");
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText, readText },
      configurable: true,
    });
    const event = new MouseEvent("contextmenu");
    const preventDefault = vi.spyOn(event, "preventDefault");
    const onPaste = vi.fn();

    await handleTerminalContextMenu(
      selectedTerminal("selected text"),
      { onPaste },
      event,
      { copyInProgress: false, pasteInProgress: false },
    );

    expect(preventDefault).toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
    expect(readText).not.toHaveBeenCalled();
    expect(onPaste).toHaveBeenCalledWith("selected text");
  });

  it("keeps right-click copy behavior when no paste handler is configured", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText, readText: vi.fn() },
      configurable: true,
    });
    const event = new MouseEvent("contextmenu");

    await handleTerminalContextMenu(
      selectedTerminal("selected text"),
      undefined,
      event,
      { copyInProgress: false, pasteInProgress: false },
    );

    expect(writeText).toHaveBeenCalledWith("selected text");
  });
});
