import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { DshComposer } from "../components/DshComposer";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const saveMock = vi.mocked(save);

function renderComposer() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <DshComposer taskId="task-1" sessionId="session-1" />
      </ToastProvider>
    </I18nProvider>,
  );
}

async function submitLine(line: string) {
  const box = screen.getByRole("textbox");
  await userEvent.click(box);
  await userEvent.paste(line);
  // A pasted `/export` sits under the caret as a trigger token, so the
  // completion menu is open; Escape dismisses it and hands Enter to the send.
  await userEvent.keyboard("{Escape}");
  await userEvent.keyboard("{Enter}");
}

/** Only the prompt calls; the composer also pulls the trigger catalogs. */
function promptCalls() {
  return invokeMock.mock.calls.filter(([command]) => command === "prompt_dsh_task");
}

describe("DSH composer /export", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    saveMock.mockReset();
  });

  it("downloads the session archive when the Harness accepts /export", async () => {
    invokeMock.mockImplementation((command) =>
      command === "prompt_dsh_task"
        ? Promise.resolve({ accepted: true, command: { kind: "success", text: "requested" } })
        : Promise.resolve({ path: "/tmp/dsh-session-session-1.zip", bytes: 1_024 }),
    );
    saveMock.mockResolvedValue("/tmp/dsh-session-session-1.zip");
    renderComposer();
    await submitLine("/export");
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("export_dsh_session_log", {
        sessionId: "session-1",
        outputPath: "/tmp/dsh-session-session-1.zip",
        includeDescendants: true,
      }),
    );
    expect(await screen.findByText(/Session log saved to/)).toBeInTheDocument();
  });

  it("leaves a rejected /export to the Harness message alone", async () => {
    // `/export <path>` is refused by the Harness command, so no archive is
    // requested and no save dialog opens.
    invokeMock.mockResolvedValue({
      accepted: true,
      command: { kind: "error", text: "The Web /export command does not accept a path." },
    });
    renderComposer();
    await submitLine("/export /tmp/out.zip");
    await waitFor(() => expect(promptCalls()).toHaveLength(1));
    expect(saveMock).not.toHaveBeenCalled();
    expect(invokeMock).not.toHaveBeenCalledWith("export_dsh_session_log", expect.anything());
  });

  it("sends an ordinary prompt without touching the export path", async () => {
    invokeMock.mockResolvedValue({ accepted: true });
    renderComposer();
    await submitLine("summarize the diff");
    await waitFor(() => expect(promptCalls()).toHaveLength(1));
    expect(saveMock).not.toHaveBeenCalled();
  });
});
