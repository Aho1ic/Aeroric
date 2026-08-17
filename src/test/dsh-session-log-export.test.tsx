import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { DshSessionLogExportButton } from "../components/DshSessionLogExport";
import {
  dshCommandName,
  dshSessionLogFilename,
  formatDshExportSize,
  readDshCommandOutcome,
  requestsDshSessionLogExport,
} from "../dshSessionLogExport";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const saveMock = vi.mocked(save);

function renderButton(sessionId = "session-1") {
  return render(
    <I18nProvider>
      <ToastProvider>
        <DshSessionLogExportButton sessionId={sessionId} />
      </ToastProvider>
    </I18nProvider>,
  );
}

describe("DSH session log export", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    saveMock.mockReset();
  });

  it("proposes the archive filename the Harness endpoint would attach", () => {
    // Mirrors `sessionLogZipFilename`, so the native dialog and a browser
    // download of the same session agree on the file name.
    expect(dshSessionLogFilename("session-01HXYZ_9")).toBe("dsh-session-session-01HXYZ_9.zip");
    expect(dshSessionLogFilename("a/b c:d.e")).toBe("dsh-session-a_b_c_d_e.zip");
  });

  it("starts a download only for an accepted /export line", () => {
    const accepted = { accepted: true, command: { kind: "success", text: "requested" } };
    expect(requestsDshSessionLogExport("/export", accepted)).toBe(true);
    expect(requestsDshSessionLogExport("  /export  ", accepted)).toBe(true);
    // `/export <path>` is rejected by the Harness command itself; the client
    // must not download anything the Harness refused.
    expect(
      requestsDshSessionLogExport("/export /tmp/out.zip", {
        accepted: true,
        command: { kind: "error", text: "The Web /export command does not accept a path." },
      }),
    ).toBe(false);
    // An ordinary prompt carries no command result at all.
    expect(requestsDshSessionLogExport("summarize the diff", { accepted: true })).toBe(false);
    expect(requestsDshSessionLogExport("/exportx", accepted)).toBe(false);
  });

  it("reads the command name and outcome the prompt result carries", () => {
    expect(dshCommandName("/goal set ship it")).toBe("goal");
    expect(dshCommandName("no slash")).toBeUndefined();
    expect(readDshCommandOutcome({ command: { kind: "success" } })).toEqual({ kind: "success" });
    expect(readDshCommandOutcome({ command: { kind: "error", text: "no" } })).toEqual({
      kind: "error",
      text: "no",
    });
    expect(readDshCommandOutcome({ command: { kind: "queued" } })).toBeUndefined();
    expect(readDshCommandOutcome(null)).toBeUndefined();
  });

  it("formats the written archive size", () => {
    expect(formatDshExportSize(512)).toBe("512 B");
    expect(formatDshExportSize(2_048)).toBe("2.0 KB");
    expect(formatDshExportSize(5 * 1_024 * 1_024)).toBe("5.0 MB");
    expect(formatDshExportSize(-1)).toBe("0 B");
  });

  it("streams the archive to the chosen path and reports it", async () => {
    saveMock.mockResolvedValue("/tmp/dsh-session-session-1.zip");
    invokeMock.mockResolvedValue({ path: "/tmp/dsh-session-session-1.zip", bytes: 4_096 });
    renderButton();
    await userEvent.click(screen.getByRole("button", { name: /Session log/ }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("export_dsh_session_log", {
        sessionId: "session-1",
        outputPath: "/tmp/dsh-session-session-1.zip",
        includeDescendants: true,
      });
    });
    expect(saveMock.mock.calls[0][0]).toMatchObject({
      defaultPath: "dsh-session-session-1.zip",
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    expect(await screen.findByText(/Session log saved to/)).toBeInTheDocument();
  });

  it("writes nothing when the save dialog is dismissed", async () => {
    saveMock.mockResolvedValue(null);
    renderButton();
    await userEvent.click(screen.getByRole("button", { name: /Session log/ }));
    await waitFor(() => expect(saveMock).toHaveBeenCalled());
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("surfaces a rejected export instead of failing silently", async () => {
    saveMock.mockResolvedValue("/tmp/out.zip");
    invokeMock.mockRejectedValue(
      "Session log export returned HTTP 404 Not Found: session not found",
    );
    renderButton();
    await userEvent.click(screen.getByRole("button", { name: /Session log/ }));
    expect(await screen.findByText(/Session export failed: .*404/)).toBeInTheDocument();
  });
});
