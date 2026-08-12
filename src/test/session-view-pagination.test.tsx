import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionView } from "../components/SessionView";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("SessionView paged history", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("loads the newest page first and prepends every earlier message", async () => {
    vi.mocked(invoke).mockImplementation((_command, args) => {
      const cursor = (args as { cursor?: number | null })?.cursor;
      if (cursor == null) {
        return Promise.resolve({
          messages: [
            {
              role: "assistant",
              content: [
                { type: "thinking", thinking: "complete reasoning" },
                { type: "text", text: "newest assistant answer" },
                { type: "tool_use", id: "tool-1", name: "Read", input: "full input" },
              ],
            },
          ],
          nextCursor: 100,
          hasMore: true,
        });
      }
      return Promise.resolve({
        messages: [
          {
            role: "user",
            content: [
              { type: "text", text: "oldest user request" },
              {
                type: "attachment",
                name: "context.txt",
                mediaType: "text/plain",
                source: "/tmp/context.txt",
              },
            ],
          },
        ],
        nextCursor: null,
        hasMore: false,
      });
    });

    render(
      <I18nProvider>
        <SessionView sessionPath="/tmp/session.jsonl" projectPath="/tmp" isCodex />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(screen.getByText("oldest user request")).toBeInTheDocument();
      expect(screen.getByText("newest assistant answer")).toBeInTheDocument();
    });
    expect(screen.getByText("context.txt · text/plain")).toBeInTheDocument();
    expect(screen.getByText("Read")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledTimes(2);
    expect(document.querySelectorAll("[data-session-role='user']")).toHaveLength(1);
    expect(document.querySelectorAll("[data-session-role='assistant']")).toHaveLength(1);
  });

  it("merges Codex records split across a page boundary without duplicating the user bubble", async () => {
    vi.mocked(invoke).mockImplementation((_command, args) => {
      const cursor = (args as { cursor?: number | null })?.cursor;
      if (cursor == null) {
        return Promise.resolve({
          messages: [
            {
              role: "user",
              content: [
                { type: "text", text: "same request" },
                {
                  type: "attachment",
                  name: "reference.png",
                  mediaType: "image/png",
                  source: "/tmp/reference.png",
                },
              ],
            },
            { role: "assistant", content: [{ type: "text", text: "answer" }] },
          ],
          nextCursor: 100,
          hasMore: true,
        });
      }
      return Promise.resolve({
        messages: [{ role: "user", content: [{ type: "text", text: "same request" }] }],
        nextCursor: null,
        hasMore: false,
      });
    });

    render(
      <I18nProvider>
        <SessionView sessionPath="/tmp/session.jsonl" projectPath="/tmp" isCodex />
      </I18nProvider>,
    );

    await waitFor(() => expect(screen.getByText("answer")).toBeInTheDocument());
    expect(screen.getAllByText("same request")).toHaveLength(1);
    expect(screen.getByText("reference.png · image/png")).toBeInTheDocument();
    expect(document.querySelectorAll("[data-session-role='user']")).toHaveLength(1);
  });
});
