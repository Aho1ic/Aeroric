import type { Terminal } from "@xterm/xterm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { attachMouseReleaseStrandGuard } from "../components/terminalShared";

/**
 * xterm 6.0.0 的 `CoreBrowserTerminal.bindMouse` 记账复刻。
 *
 * 只保留与「释放报文丢失」相关的那几行，逐字对齐上游语义：
 * - `mouseup` / `mousedrag` 这两个 document 级监听**只在 mousedown 里**挂上；
 * - `onProtocolChange` 会摘掉它们，重新被请求时只把函数放回 `requestedEvents`，
 *   不再往 document 上挂。
 *
 * 于是「按下 → 关一次上报 → 再开 → 松手」这条路上，松手没有任何监听者。
 */
const CORE_MOUSE_EVENT_UP = 2;
const CORE_MOUSE_EVENT_DRAG = 4;

interface FakeXterm {
  term: Terminal;
  element: HTMLElement;
  /** 已发出的报文，"M" 结尾是按下/拖动，"m" 结尾是释放。 */
  reports: string[];
  /** 模拟程序改上报模式（DECSET/DECRST）。 */
  setProtocol: (events: number) => void;
  dispose: () => void;
}

function fakeXterm(initialEvents: number): FakeXterm {
  const element = document.createElement("div");
  element.className = "xterm";
  document.body.appendChild(element);

  const reports: string[] = [];
  const protocolListeners: Array<(events: number) => void> = [];
  const requested: {
    mouseup: EventListener | null;
    mousedrag: EventListener | null;
  } = { mouseup: null, mousedrag: null };

  const sendEvent = (event: MouseEvent, kind: "M" | "m") => {
    reports.push(`\u001b[<0;1;1${kind}`);
    void event;
  };

  const listeners = {
    mouseup: ((event: Event) => {
      const mouse = event as MouseEvent;
      sendEvent(mouse, "m");
      if (!mouse.buttons) {
        document.removeEventListener("mouseup", requested.mouseup!);
        if (requested.mousedrag) {
          document.removeEventListener("mousemove", requested.mousedrag);
        }
      }
    }) satisfies EventListener,
    mousedrag: ((event: Event) => {
      const mouse = event as MouseEvent;
      if (mouse.buttons) sendEvent(mouse, "M");
    }) satisfies EventListener,
  };

  // 上游：element 级 mousedown 里才把 document 监听挂上。
  element.addEventListener("mousedown", (event) => {
    sendEvent(event, "M");
    if (requested.mouseup) document.addEventListener("mouseup", requested.mouseup);
    if (requested.mousedrag) document.addEventListener("mousemove", requested.mousedrag);
    // 上游 `cancel(ev)` 会 stopPropagation() —— 还原点不能挂在 document 上。
    event.stopPropagation();
  });

  const applyProtocol = (events: number) => {
    if (events & CORE_MOUSE_EVENT_UP) {
      // 上游这里只重新赋值，不 addEventListener —— 正是这处漏账。
      if (!requested.mouseup) requested.mouseup = listeners.mouseup;
    } else {
      document.removeEventListener("mouseup", requested.mouseup!);
      requested.mouseup = null;
    }
    if (events & CORE_MOUSE_EVENT_DRAG) {
      if (!requested.mousedrag) requested.mousedrag = listeners.mousedrag;
    } else {
      document.removeEventListener("mousemove", requested.mousedrag!);
      requested.mousedrag = null;
    }
    for (const listener of protocolListeners) listener(events);
  };
  applyProtocol(initialEvents);

  const term = {
    element,
    _core: {
      coreMouseService: {
        onProtocolChange: (listener: (events: number) => void) => {
          protocolListeners.push(listener);
          return {
            dispose: () => {
              const index = protocolListeners.indexOf(listener);
              if (index >= 0) protocolListeners.splice(index, 1);
            },
          };
        },
      },
    },
  } as unknown as Terminal;

  return {
    term,
    element,
    reports,
    setProtocol: applyProtocol,
    dispose: () => {
      if (requested.mouseup) document.removeEventListener("mouseup", requested.mouseup);
      if (requested.mousedrag) document.removeEventListener("mousemove", requested.mousedrag);
      element.remove();
    },
  };
}

function press(element: HTMLElement): void {
  element.dispatchEvent(new MouseEvent("mousedown", { button: 0, buttons: 1, bubbles: true }));
}
function release(): void {
  document.dispatchEvent(new MouseEvent("mouseup", { button: 0, buttons: 0, bubbles: true }));
}
function drag(): void {
  document.dispatchEvent(new MouseEvent("mousemove", { buttons: 1, bubbles: true }));
}

describe("mouse release strand guard", () => {
  const cleanups: Array<() => void> = [];
  beforeEach(() => {
    // 守卫用 setTimeout(0) 兜底还原 document.addEventListener；用假时钟推进它，
    // 不要真等一拍。
    vi.useFakeTimers();
  });
  afterEach(() => {
    while (cleanups.length) cleanups.pop()!();
    vi.useRealTimers();
  });

  it("reproduces the upstream strand without the guard", () => {
    const fake = fakeXterm(CORE_MOUSE_EVENT_UP);
    cleanups.push(fake.dispose);

    press(fake.element);
    // 程序在按下期间重发整段模式串：先关再开。
    fake.setProtocol(0);
    fake.setProtocol(CORE_MOUSE_EVENT_UP);
    release();

    // 按下发出去了，释放永远不发 —— 程序那边就停在「左键按住」。
    expect(fake.reports).toEqual(["\u001b[<0;1;1M"]);
  });

  it("still reports the release when the protocol is re-emitted mid-press", () => {
    const fake = fakeXterm(CORE_MOUSE_EVENT_UP);
    cleanups.push(fake.dispose);
    cleanups.push(attachMouseReleaseStrandGuard(fake.term));

    press(fake.element);
    fake.setProtocol(0);
    fake.setProtocol(CORE_MOUSE_EVENT_UP);
    release();

    expect(fake.reports).toEqual(["\u001b[<0;1;1M", "\u001b[<0;1;1m"]);
    vi.advanceTimersByTime(0);
  });

  it("restores drag reporting too, so the app keeps tracking the held button", () => {
    const fake = fakeXterm(CORE_MOUSE_EVENT_UP | CORE_MOUSE_EVENT_DRAG);
    cleanups.push(fake.dispose);
    cleanups.push(attachMouseReleaseStrandGuard(fake.term));

    press(fake.element);
    fake.setProtocol(0);
    fake.setProtocol(CORE_MOUSE_EVENT_UP | CORE_MOUSE_EVENT_DRAG);
    drag();
    release();

    expect(fake.reports).toEqual(["\u001b[<0;1;1M", "\u001b[<0;1;1M", "\u001b[<0;1;1m"]);
    vi.advanceTimersByTime(0);
  });

  it("does not duplicate reports for an ordinary click", () => {
    const fake = fakeXterm(CORE_MOUSE_EVENT_UP);
    cleanups.push(fake.dispose);
    cleanups.push(attachMouseReleaseStrandGuard(fake.term));

    press(fake.element);
    release();
    press(fake.element);
    release();

    expect(fake.reports).toEqual([
      "\u001b[<0;1;1M",
      "\u001b[<0;1;1m",
      "\u001b[<0;1;1M",
      "\u001b[<0;1;1m",
    ]);
    vi.advanceTimersByTime(0);
  });

  it("ignores a protocol change while no button is held", () => {
    const fake = fakeXterm(CORE_MOUSE_EVENT_UP);
    cleanups.push(fake.dispose);
    cleanups.push(attachMouseReleaseStrandGuard(fake.term));

    // 一次完整点击，抓到引用；随后空转的协议变化不该凭空挂上监听。
    press(fake.element);
    release();
    fake.setProtocol(0);
    fake.setProtocol(CORE_MOUSE_EVENT_UP);
    release();

    expect(fake.reports).toEqual(["\u001b[<0;1;1M", "\u001b[<0;1;1m"]);
    vi.advanceTimersByTime(0);
  });

  it("leaves document.addEventListener unpatched after the gesture", () => {
    const fake = fakeXterm(CORE_MOUSE_EVENT_UP);
    cleanups.push(fake.dispose);
    cleanups.push(attachMouseReleaseStrandGuard(fake.term));

    const before = document.addEventListener;
    press(fake.element);
    release();
    vi.advanceTimersByTime(0);

    expect(document.addEventListener).toBe(before);
  });

  it("no-ops when the terminal has no element or core mouse service", () => {
    expect(attachMouseReleaseStrandGuard({} as Terminal)).toBeTypeOf("function");
    const orphan = { element: document.createElement("div") } as unknown as Terminal;
    expect(attachMouseReleaseStrandGuard(orphan)).toBeTypeOf("function");
  });
});
