import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nProvider } from "../i18n";
import { DshSlashPalette } from "../components/DshSlashPalette";
import {
  DSH_SLASH_COMMANDS,
  detectDshSlashCommand,
  dshSlashCommandsAvailable,
} from "../dshSlashCommands";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({ groups: [] }),
}));

function Wrapper({ children }: { children: React.ReactNode }) {
  return <I18nProvider>{children}</I18nProvider>;
}

describe("dshSlashCommands catalog", () => {
  it("includes the six server-registered commands plus popup selects", () => {
    const names = DSH_SLASH_COMMANDS.map((c) => c.name);
    expect(names).toEqual(
      expect.arrayContaining([
        "compact",
        "feedback",
        "goal",
        "plan",
        "permission",
        "export",
        "model",
        "skill",
        "subagent",
      ]),
    );
  });

  it("detects a leading slash command with an argument", () => {
    const got = detectDshSlashCommand("/feedback good work");
    expect(got?.command.name).toBe("feedback");
    expect(got?.arg).toBe("good work");
  });

  it("detects a bare slash command with no argument", () => {
    const got = detectDshSlashCommand("/compact");
    expect(got?.command.name).toBe("compact");
    expect(got?.arg).toBe("");
  });

  it("returns null for an unknown command", () => {
    expect(detectDshSlashCommand("/nope")).toBeNull();
  });

  it("returns null when the slash is not at the start", () => {
    expect(detectDshSlashCommand("then /compact")).toBeNull();
  });

  it("is only available for dsh-family agents", () => {
    expect(dshSlashCommandsAvailable("dsh")).toBe(true);
    expect(dshSlashCommandsAvailable("claude")).toBe(false);
  });
});

describe("DshSlashPalette", () => {
  it("lists commands and inserts the chosen name via the editor inserter", async () => {
    const user = userEvent.setup();
    const insert = vi.fn().mockReturnValue(true);
    const onDismiss = vi.fn();
    render(
      <Wrapper>
        <DshSlashPalette editorInsert={insert} onDismiss={onDismiss} />
      </Wrapper>,
    );
    // Filter to compact and pick it.
    const input = screen.getByPlaceholderText("Slash commands");
    await user.type(input, "comp");
    expect(screen.getByText(/compact/)).toBeInTheDocument();
    await user.keyboard("{Enter}");
    expect(insert).toHaveBeenCalledWith("compact");
    expect(onDismiss).toHaveBeenCalled();
  });
});
