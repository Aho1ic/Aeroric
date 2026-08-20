import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import {
  collectDshImageAttachments,
  dshImageFit,
  parseDshImageAttachment,
  type DshImageAttachmentRef,
} from "../dshImageAttachments";
import { projectDshSessionEvents } from "../dshSessionFeatures";
import { DshImageGallery, DshMessageImage } from "../components/DshImageGallery";
import { DshSessionInsights } from "../components/DshSessionInsights";
import { DshTrajectoryHost } from "../components/DshTrajectoryHost";
import { DshTrajectoryOverlay } from "../components/DshTrajectoryOverlay";
import { useDshImageLoader, type DshImageLoader } from "../hooks/useDshImageLoader";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const attachment: DshImageAttachmentRef = {
  attachmentId: `sha256:${"a".repeat(64)}`,
  mediaType: "image/png",
  bytes: 68,
  width: 640,
  height: 320,
  name: "history.png",
};

function imageBlock(ref: DshImageAttachmentRef = attachment) {
  return { type: "image", attachment: ref };
}

function renderImage(
  ref: DshImageAttachmentRef,
  load: DshImageLoader,
  variant: "single" | "tile" = "single",
) {
  return render(
    <I18nProvider>
      <DshMessageImage attachment={ref} load={load} variant={variant} />
    </I18nProvider>,
  );
}

describe("DSH image attachment references", () => {
  it("reads a durable reference and rejects one it could not lay out", () => {
    expect(parseDshImageAttachment(imageBlock().attachment)).toEqual(attachment);
    const { name: _name, ...unnamed } = attachment;
    expect(parseDshImageAttachment(unnamed)).toEqual(unnamed);
    // An unknown media type cannot be decoded and a missing intrinsic size
    // cannot be bounded without reflowing once the bytes land.
    expect(parseDshImageAttachment({ ...attachment, mediaType: "image/tiff" })).toBeUndefined();
    expect(parseDshImageAttachment({ ...attachment, width: 0 })).toBeUndefined();
    expect(parseDshImageAttachment({ ...attachment, height: 12.5 })).toBeUndefined();
    expect(parseDshImageAttachment({ ...attachment, bytes: -1 })).toBeUndefined();
    expect(parseDshImageAttachment({ ...attachment, attachmentId: "" })).toBeUndefined();
    expect(parseDshImageAttachment(null)).toBeUndefined();
  });

  it("collects the image blocks of one content array in block order", () => {
    const second = { ...attachment, attachmentId: "sha256:second", name: "second.png" };
    expect(
      collectDshImageAttachments([
        { type: "text", text: "look" },
        imageBlock(),
        { type: "image" },
        { type: "image", attachment: { attachmentId: "sha256:bad" } },
        imageBlock(second),
      ]),
    ).toEqual([attachment, second]);
    expect(collectDshImageAttachments("not an array")).toEqual([]);
  });

  it("bounds a lone image and anchors the crop toward the informative edge", () => {
    expect(dshImageFit(attachment)).toEqual({
      width: 240,
      height: 120,
      objectPosition: "center",
    });
    expect(dshImageFit({ ...attachment, width: 100, height: 2_000 })).toEqual({
      width: 60,
      height: 240,
      objectPosition: "center top",
    });
    expect(dshImageFit({ ...attachment, width: 4_000, height: 100 })).toEqual({
      width: 240,
      height: 60,
      objectPosition: "left center",
    });
    // Never upscaled past the image's natural size.
    expect(dshImageFit({ ...attachment, width: 100, height: 100 })).toEqual({
      width: 100,
      height: 100,
      objectPosition: "center",
    });
  });

  it("projects the images a history event carries onto its trajectory entry", () => {
    const { trajectory } = projectDshSessionEvents([
      { type: "user/message", seq: 1, data: { content: [{ type: "text", text: "hi" }] } },
      {
        type: "user/message",
        seq: 2,
        data: { content: [imageBlock(), { type: "text", text: "x" }] },
      },
      { type: "tool/result", seq: 3, data: { message: { content: [imageBlock()] } } },
    ]);
    expect(trajectory[0].images).toBeUndefined();
    expect(trajectory[1].images).toEqual([attachment]);
    // A tool that answers with an image (`read_image`) reaches the same walk.
    expect(trajectory[2].images).toEqual([attachment]);
  });

  it("counts an event carrying both content shapes once", () => {
    const { trajectory } = projectDshSessionEvents([
      {
        type: "user/message",
        seq: 1,
        data: { content: [imageBlock()], message: { content: [imageBlock()] } },
      },
    ]);
    expect(trajectory[0].images).toEqual([attachment]);
  });
});

describe("DSH image gallery", () => {
  it("renders nothing without images and an aligned wrapping group with them", async () => {
    const load = vi.fn<DshImageLoader>().mockResolvedValue("blob:gallery");
    const empty = render(
      <I18nProvider>
        <DshImageGallery images={[]} load={load} align="start" />
      </I18nProvider>,
    );
    expect(empty.container.firstChild).toBeNull();
    empty.unmount();
    const view = render(
      <I18nProvider>
        <DshImageGallery images={[attachment, attachment]} load={load} align="end" />
      </I18nProvider>,
    );
    expect(view.container.querySelector('[data-align="end"]')).not.toBeNull();
    await waitFor(() => expect(view.getAllByAltText("history.png")).toHaveLength(2));
  });

  it("renders a lone image large and several images as square tiles", () => {
    const load = vi.fn<DshImageLoader>(() => new Promise<string>(() => {}));
    const lone = render(
      <I18nProvider>
        <DshImageGallery images={[attachment]} load={load} align="start" />
      </I18nProvider>,
    );
    expect(lone.container.querySelectorAll('[data-variant="single"]')).toHaveLength(1);
    lone.unmount();
    const several = render(
      <I18nProvider>
        <DshImageGallery images={[attachment, attachment, attachment]} load={load} align="end" />
      </I18nProvider>,
    );
    expect(several.container.querySelectorAll('[data-variant="tile"]')).toHaveLength(3);
  });
});

describe("DSH history image thumbnail", () => {
  it("bounds the thumbnail, loads the URL, and clicks into the original", async () => {
    const load = vi.fn<DshImageLoader>().mockResolvedValue("blob:history");
    renderImage(attachment, load);
    const frame = screen.getByRole("button", { name: "history.png, click to view original" });
    expect(frame.getAttribute("style")).toContain("width: 240px");
    expect(frame.getAttribute("style")).toContain("height: 120px");
    expect(frame).toHaveAttribute("title", "View original");
    await waitFor(() => expect(screen.getByAltText("history.png")).toBeInTheDocument());
    expect(load).toHaveBeenCalledWith(attachment);
    expect(screen.getByAltText("history.png").style.objectPosition).toBe("center");
    await userEvent.click(frame);
    expect(screen.getByRole("dialog", { name: "Original image preview" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close original image preview" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes the original preview on Escape and restores focus to the opener", async () => {
    const load = vi.fn<DshImageLoader>().mockResolvedValue("blob:escape");
    renderImage(attachment, load);
    const frame = screen.getByRole("button", { name: "history.png, click to view original" });
    await waitFor(() => expect(screen.getByAltText("history.png")).toBeInTheDocument());
    await userEvent.click(frame);
    expect(screen.getByRole("button", { name: "Close original image preview" })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(frame).toHaveFocus();
  });

  it("ignores a click while the thumbnail is still loading", async () => {
    const load = vi.fn<DshImageLoader>(() => new Promise<string>(() => {}));
    renderImage(attachment, load);
    expect(screen.getByText("Loading image…")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "history.png, click to view original" }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("falls back to the generic image label for an unnamed attachment", async () => {
    const { name: _name, ...unnamed } = attachment;
    const load = vi.fn<DshImageLoader>().mockResolvedValue("blob:unnamed");
    renderImage(unnamed, load);
    await waitFor(() => expect(screen.getByAltText("Image")).toBeInTheDocument());
    expect(
      screen.getByRole("button", { name: "Image, click to view original" }),
    ).toBeInTheDocument();
  });

  it("surfaces a retry control when durable bytes cannot be read", async () => {
    const load = vi
      .fn<DshImageLoader>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockRejectedValueOnce(new Error("still offline"))
      .mockResolvedValueOnce("blob:retry");
    renderImage(attachment, load);
    const retry = await screen.findByRole("button", {
      name: "Image failed to load; click to retry",
    });
    await userEvent.click(retry);
    await userEvent.click(
      await screen.findByRole("button", { name: "Image failed to load; click to retry" }),
    );
    await waitFor(() => expect(screen.getByAltText("history.png")).toBeInTheDocument());
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("keeps the fixed square on a tile and on its failed-load retry control", async () => {
    const pending = vi.fn<DshImageLoader>(() => new Promise<string>(() => {}));
    const tile = renderImage(attachment, pending, "tile");
    const frame = screen.getByRole("button", { name: "history.png, click to view original" });
    expect(frame).toHaveAttribute("data-variant", "tile");
    expect(frame.getAttribute("style")).toBeNull();
    tile.unmount();
    const failing = vi.fn<DshImageLoader>().mockRejectedValue(new Error("offline"));
    renderImage(attachment, failing, "tile");
    const retry = await screen.findByRole("button", {
      name: "Image failed to load; click to retry",
    });
    expect(retry).toHaveAttribute("data-variant", "tile");
  });

  it("ignores a load settling after unmount", async () => {
    let resolve: ((url: string) => void) | undefined;
    const load = vi.fn<DshImageLoader>(
      () =>
        new Promise<string>((settle) => {
          resolve = settle;
        }),
    );
    renderImage(attachment, load).unmount();
    resolve?.("blob:late");
    await Promise.resolve();
    expect(screen.queryByAltText("history.png")).not.toBeInTheDocument();
  });
});

function LoaderHarness({ sessionId }: { sessionId: string }) {
  const load = useDshImageLoader(sessionId);
  return (
    <>
      <DshMessageImage attachment={attachment} load={load} variant="tile" />
      <DshMessageImage attachment={attachment} load={load} variant="tile" />
    </>
  );
}

describe("DSH session image loader", () => {
  const originalCreate = URL.createObjectURL;
  const originalRevoke = URL.revokeObjectURL;

  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    URL.createObjectURL = originalCreate;
    URL.revokeObjectURL = originalRevoke;
  });

  it("fetches each attachment once and revokes its object URL on release", async () => {
    const create = vi.fn(() => "blob:loaded");
    const revoke = vi.fn();
    URL.createObjectURL = create as unknown as typeof URL.createObjectURL;
    URL.revokeObjectURL = revoke;
    invokeMock.mockResolvedValue({ attachment, data: "aGVsbG8=" });
    const view = render(
      <I18nProvider>
        <LoaderHarness sessionId="session-1" />
      </I18nProvider>,
    );
    await waitFor(() => expect(screen.getAllByAltText("history.png")).toHaveLength(2));
    // Two thumbnails of the same attachment share one fetch and one URL.
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_dsh_session_attachment", {
      sessionId: "session-1",
      attachmentId: attachment.attachmentId,
    });
    expect(create).toHaveBeenCalledTimes(1);
    view.unmount();
    expect(revoke).toHaveBeenCalledWith("blob:loaded");
  });

  it("falls back to the base64 the Harness sent when object URLs are unavailable", async () => {
    URL.createObjectURL = undefined as unknown as typeof URL.createObjectURL;
    invokeMock.mockResolvedValue({ attachment, data: "aGVsbG8=" });
    render(
      <I18nProvider>
        <LoaderHarness sessionId="session-1" />
      </I18nProvider>,
    );
    await waitFor(() => expect(screen.getAllByAltText("history.png")).toHaveLength(2));
    expect(screen.getAllByAltText("history.png")[0]).toHaveAttribute(
      "src",
      "data:image/png;base64,aGVsbG8=",
    );
  });

  it("reports a response without bytes as a failed load", async () => {
    invokeMock.mockResolvedValue({ attachment });
    render(
      <I18nProvider>
        <LoaderHarness sessionId="session-1" />
      </I18nProvider>,
    );
    expect(
      await screen.findAllByRole("button", { name: "Image failed to load; click to retry" }),
    ).toHaveLength(2);
  });
});

describe("DSH insights trajectory images", () => {
  const originalCreate = URL.createObjectURL;

  beforeEach(() => {
    invokeMock.mockReset();
    URL.createObjectURL = undefined as unknown as typeof URL.createObjectURL;
  });

  afterEach(() => {
    URL.createObjectURL = originalCreate;
  });

  it("renders the images of a replayed history event inside the trajectory", async () => {
    invokeMock.mockImplementation((command) => {
      if (command === "get_dsh_session_history") {
        return Promise.resolve({
          events: [
            {
              event: {
                type: "user/message",
                seq: 7,
                time: 1_700_000_000_000,
                data: { content: [imageBlock(), { type: "text", text: "review this" }] },
              },
            },
          ],
          hasMore: false,
          projections: { values: {} },
        });
      }
      if (command === "get_dsh_session_attachment") {
        return Promise.resolve({ attachment, data: "aGVsbG8=" });
      }
      return Promise.resolve(null);
    });
    render(
      <I18nProvider>
        <DshTrajectoryHost sessionId="session-1">
          <DshSessionInsights />
          <DshTrajectoryOverlay />
        </DshTrajectoryHost>
      </I18nProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: /Trajectory/ }));
    const thumbnail = await screen.findByRole("button", {
      name: "history.png, click to view original",
    });
    // A user's own image aligns to the trailing edge, as it does in the Web half.
    expect(thumbnail.closest(".dsh-image-gallery")).toHaveAttribute("data-align", "end");
    await waitFor(() => expect(screen.getByAltText("history.png")).toBeInTheDocument());
    await userEvent.click(thumbnail);
    // The preview portals to the body so the insights dialog cannot clip it.
    const dialog = screen.getByRole("dialog", { name: "Original image preview" });
    expect(dialog.parentElement).toBe(document.body);
  });
});
