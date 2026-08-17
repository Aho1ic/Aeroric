import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { DshComposer } from "../components/DshComposer";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

const invokeMock = vi.mocked(invoke);

function renderComposer() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <DshComposer taskId="task-1" sessionId="session-1" />
      </ToastProvider>
    </I18nProvider>,
  );
}

/** Attach one draft image through the hidden file input and wait for its tile. */
async function attachDraft(container: HTMLElement) {
  const input = container.querySelector('input[type="file"]');
  if (!(input instanceof HTMLInputElement)) throw new Error("expected the attachment input");
  const file = new File([new Uint8Array([1, 2, 3])], "shot.png", { type: "image/png" });
  fireEvent.change(input, { target: { files: [file] } });
  return await screen.findByRole("button", { name: /click to view original/ });
}

describe("DSH composer draft images", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("opens a draft image in the original-size lightbox and closes it again", async () => {
    const { container } = renderComposer();
    const thumbnail = await attachDraft(container);
    await userEvent.click(thumbnail);
    const dialog = screen.getByRole("dialog", { name: "Original image preview" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByAltText("Original image")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close original image preview" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes the preview by itself when its draft is removed", async () => {
    // The lightbox reads the live draft rather than a copied URL, so dropping
    // the image cannot leave a preview of bytes the composer no longer holds.
    const { container } = renderComposer();
    const thumbnail = await attachDraft(container);
    await userEvent.click(thumbnail);
    expect(screen.getByRole("dialog", { name: "Original image preview" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Remove image" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(
      screen.queryByRole("button", { name: /click to view original/ }),
    ).not.toBeInTheDocument();
  });

  it("keeps the draft image out of the way of an ordinary send", async () => {
    invokeMock.mockResolvedValue({ accepted: true });
    const { container } = renderComposer();
    await attachDraft(container);
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    // The composer also pulls the `/` and `@` trigger catalogs, so the prompt
    // call is picked out by name rather than by call count.
    const promptCalls = () =>
      invokeMock.mock.calls.filter(([command]) => command === "prompt_dsh_task");
    await waitFor(() => expect(promptCalls()).toHaveLength(1));
    const [, payload] = promptCalls()[0] as [string, { images: string[] }];
    expect(payload.images).toHaveLength(1);
    expect(payload.images[0]).toMatch(/^data:image\/png;base64,/);
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /click to view original/ }),
      ).not.toBeInTheDocument(),
    );
  });
});
