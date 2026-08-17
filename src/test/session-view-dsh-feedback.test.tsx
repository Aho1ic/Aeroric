import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionView } from "../components/SessionView";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const message = {
  role: "assistant" as const,
  messageId: "message-1",
  content: [{ type: "text" as const, text: "A durable DSH answer" }],
};

const feedback = (rating: "positive" | "negative", version: string, note?: string) => ({
  messageId: message.messageId,
  rating,
  ...(note === undefined ? {} : { note }),
  version,
  createdAt: 1,
  updatedAt: 2,
});

function historyPage() {
  return { messages: [message], nextCursor: null, hasMore: false };
}

function renderView() {
  return render(
    <I18nProvider>
      <SessionView
        sessionPath="/tmp/dsh/session.jsonl"
        projectPath="/tmp/project"
        isCodex={false}
        family="dsh"
        sessionId="session-1"
      />
    </I18nProvider>,
  );
}

describe("SessionView DSH message feedback", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
  });

  it("creates feedback with a null CAS version and retracts the same rating with its returned version", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_session_message_page") return Promise.resolve(historyPage());
      if (command === "list_dsh_message_feedback") {
        return Promise.resolve({ ok: true, value: { items: [] } });
      }
      if (command === "put_dsh_message_feedback") {
        return Promise.resolve({ ok: true, value: feedback("positive", "version-1") });
      }
      if (command === "delete_dsh_message_feedback") {
        return Promise.resolve({ ok: true, value: { absent: true } });
      }
      return Promise.resolve({});
    });

    renderView();
    const helpful = await screen.findByRole("button", { name: "Helpful" });
    fireEvent.click(helpful);

    await waitFor(() => expect(helpful).toHaveAttribute("aria-pressed", "true"));
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("put_dsh_message_feedback", {
      sessionId: "session-1",
      messageId: "message-1",
      rating: "positive",
      ifVersion: null,
    });

    fireEvent.click(helpful);
    await waitFor(() => expect(helpful).toHaveAttribute("aria-pressed", "false"));
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("delete_dsh_message_feedback", {
      sessionId: "session-1",
      messageId: "message-1",
      ifVersion: "version-1",
    });
  });

  it("edits a note using the currently observed feedback version", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "read_session_message_page") return Promise.resolve(historyPage());
      if (command === "list_dsh_message_feedback") {
        return Promise.resolve({
          ok: true,
          value: { items: [feedback("negative", "version-7", "Old note")] },
        });
      }
      if (command === "put_dsh_message_feedback") {
        const note = (args as { note?: string }).note;
        return Promise.resolve({ ok: true, value: feedback("negative", "version-8", note) });
      }
      return Promise.resolve({});
    });

    renderView();
    fireEvent.click(await screen.findByRole("button", { name: "Old note" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Feedback note" }), {
      target: { value: "More precise note" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "More precise note" })).toBeInTheDocument(),
    );
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("put_dsh_message_feedback", {
      sessionId: "session-1",
      messageId: "message-1",
      rating: "negative",
      note: "More precise note",
      ifVersion: "version-7",
    });
  });

  it("refreshes authoritative feedback after a CAS conflict", async () => {
    let listCalls = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_session_message_page") return Promise.resolve(historyPage());
      if (command === "list_dsh_message_feedback") {
        listCalls += 1;
        const item =
          listCalls === 1 ? feedback("positive", "version-1") : feedback("negative", "version-2");
        return Promise.resolve({ ok: true, value: { items: [item] } });
      }
      if (command === "put_dsh_message_feedback") {
        return Promise.resolve({ ok: false, error: { code: "version-conflict" } });
      }
      return Promise.resolve({});
    });

    renderView();
    const notHelpful = await screen.findByRole("button", { name: "Not helpful" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Helpful" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    fireEvent.click(notHelpful);

    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Feedback changed elsewhere");
      expect(notHelpful).toHaveAttribute("aria-pressed", "true");
    });
    expect(listCalls).toBe(2);
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("put_dsh_message_feedback", {
      sessionId: "session-1",
      messageId: "message-1",
      rating: "negative",
      ifVersion: "version-1",
    });
  });
});
