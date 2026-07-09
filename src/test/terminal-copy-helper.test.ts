import { describe, expect, it, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import { attachSmartCopy, handleTerminalContextMenu } from "../components/terminalCopyHelper";

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
  it("suppresses repeated printable terminal keys through the shared custom key handler", () => {
    let handler: ((event: KeyboardEvent) => boolean) | undefined;
    const terminal = {
      attachCustomKeyEventHandler: vi.fn((nextHandler: (event: KeyboardEvent) => boolean) => {
        handler = nextHandler;
      }),
      hasSelection: vi.fn(() => false),
      element: document.createElement("div"),
    } as unknown as Terminal;

    const dispose = attachSmartCopy(terminal);

    expect(handler?.(new KeyboardEvent("keydown", { key: "s", repeat: true }))).toBe(false);
    expect(handler?.(new KeyboardEvent("keydown", { key: "ArrowLeft", repeat: true }))).toBe(true);
    expect(handler?.(new KeyboardEvent("keydown", { key: "s", repeat: false }))).toBe(true);
    const imeRepeat = new KeyboardEvent("keydown", { key: "Process", repeat: true });
    Object.defineProperty(imeRepeat, "keyCode", { value: 229 });
    expect(handler?.(imeRepeat)).toBe(false);

    dispose();
  });

  it("pastes clipboard text on right click even when terminal text is selected", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const readText = vi.fn().mockResolvedValue("pasted");
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText, readText },
      configurable: true,
    });
    const event = new MouseEvent("contextmenu");
    const preventDefault = vi.spyOn(event, "preventDefault");
    const onPaste = vi.fn();

    await handleTerminalContextMenu(selectedTerminal("selected text"), { onPaste }, event, {
      copyInProgress: false,
      pasteInProgress: false,
    });

    expect(preventDefault).toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
    expect(readText).toHaveBeenCalled();
    expect(onPaste).toHaveBeenCalledWith("pasted");
  });

  it("keeps right-click copy behavior when no paste handler is configured", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText, readText: vi.fn() },
      configurable: true,
    });
    const event = new MouseEvent("contextmenu");

    await handleTerminalContextMenu(selectedTerminal("selected text"), undefined, event, {
      copyInProgress: false,
      pasteInProgress: false,
    });

    expect(writeText).toHaveBeenCalledWith("selected text");
  });
});
