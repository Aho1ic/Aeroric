import { describe, expect, it, vi } from "vitest";
import { render, waitFor } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { I18nProvider } from "../i18n";
import { DshLiveBars } from "../components/DshLiveBars";
import { useDshLiveSessions } from "../hooks/useDshLiveSessions";
import type { DshProjectionFrame, DshJobsFrame, DshQueueFrame } from "../types";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const listenMock = listen as unknown as ReturnType<typeof vi.fn>;

/** A recorder that captures the frame handlers `useDshLiveSessions` registered. */
function captureHandlers() {
  const handlers: Record<string, (e: { payload: unknown }) => void> = {};
  listenMock.mockImplementation((event: string, cb: (e: { payload: unknown }) => void) => {
    handlers[event] = cb;
    return Promise.resolve(() => {});
  });
  return handlers;
}

function Wrapper({ children }: { children: React.ReactNode }) {
  return <I18nProvider>{children}</I18nProvider>;
}

describe("useDshLiveSessions — projection/jobs/queue consumption", () => {
  it("applies a goal projection frame to live state", async () => {
    const handlers = captureHandlers();
    let sessions: Record<string, unknown> = {};
    function Probe() {
      const api = useDshLiveSessions();
      sessions = api.sessions;
      return null;
    }
    const { unmount } = render(<Probe />, { wrapper: Wrapper });

    const frame: DshProjectionFrame = {
      type: "session/projection",
      sessionId: "s1",
      key: "goal",
      value: { goal: { id: "g1", title: "Ship parity", revision: 1, phase: "active" } },
      seq: 5,
    };
    handlers["dsh-session-projection"]({ payload: frame });

    await waitFor(() => {
      expect((sessions as { s1?: { goal?: { title?: string } } }).s1?.goal?.title).toBe(
        "Ship parity",
      );
    });
    unmount();
  });

  it("drops a stale projection frame under higher-seq-wins", async () => {
    const handlers = captureHandlers();
    let sessions: Record<string, unknown> = {};
    function Probe() {
      const api = useDshLiveSessions();
      sessions = api.sessions;
      return null;
    }
    const { unmount } = render(<Probe />, { wrapper: Wrapper });

    const hi: DshProjectionFrame = {
      type: "session/projection",
      sessionId: "s1",
      key: "plan",
      value: { active: true },
      seq: 10,
    };
    const lo: DshProjectionFrame = {
      type: "session/projection",
      sessionId: "s1",
      key: "plan",
      value: { active: false },
      seq: 3,
    };
    handlers["dsh-session-projection"]({ payload: hi });
    handlers["dsh-session-projection"]({ payload: lo });

    await waitFor(() => {
      expect((sessions as { s1?: { planMode?: boolean } }).s1?.planMode).toBe(true);
    });
    unmount();
  });

  it("applies jobs and queue frames", async () => {
    const handlers = captureHandlers();
    let sessions: Record<string, unknown> = {};
    function Probe() {
      const api = useDshLiveSessions();
      sessions = api.sessions;
      return null;
    }
    const { unmount } = render(<Probe />, { wrapper: Wrapper });

    const jobs: DshJobsFrame = {
      type: "session/jobs",
      sessionId: "s1",
      jobs: [{ id: "j1", kind: "bash", status: "running", label: "lint" }],
    };
    const queue: DshQueueFrame = {
      type: "session/queue",
      sessionId: "s1",
      items: [{ itemId: "q1", text: "then do X" }],
    };
    handlers["dsh-session-jobs"]({ payload: jobs });
    handlers["dsh-session-queue"]({ payload: queue });

    await waitFor(() => {
      const s = sessions as { s1?: { jobs?: unknown[]; queue?: unknown[] } };
      expect(s.s1?.jobs?.length).toBe(1);
      expect(s.s1?.queue?.length).toBe(1);
    });
    unmount();
  });
});

describe("DshLiveBars rendering", () => {
  it("renders the goal bar when a goal projection is present", () => {
    const { getByText } = render(
      <Wrapper>
        <DshLiveBars
          sessionId="s1"
          live={{
            goal: { id: "g1", title: "Ship parity", revision: 1, phase: "active" },
          }}
        />
      </Wrapper>,
    );
    expect(getByText("Ship parity")).toBeInTheDocument();
  });

  it("renders nothing when there is no live state", () => {
    const { container } = render(
      <Wrapper>
        <DshLiveBars sessionId="s1" live={undefined} />
      </Wrapper>,
    );
    expect(container.firstChild).toBeNull();
  });
});
