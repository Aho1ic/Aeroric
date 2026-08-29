import { describe, expect, it } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { useNoteLayoutTier } from "../components/notebook/useNoteLayoutTier";
import { triggerResize } from "./resizeObserverStub";

/** 当前宽度。测试直接改它,再 triggerResize 让 hook 重新量。 */
let width = 0;

/**
 * 量宽度的次数。判断「有没有 disconnect」只能看这个 —— 卸载后再 setState,
 * React 18 既不抛也不告警,`not.toThrow()` 那种断言是钉不住东西的。
 * 观察还挂着的话,triggerResize 会走到 measure,这个数就会涨。
 */
let measureCount = 0;

function Probe() {
  const { ref, tier } = useNoteLayoutTier<HTMLDivElement>();
  return (
    <div
      ref={(element) => {
        ref.current = element;
        if (element) {
          element.getBoundingClientRect = () => {
            measureCount += 1;
            return { width, height: 100 } as DOMRect;
          };
        }
      }}
      data-testid="probe"
    >
      {tier}
    </div>
  );
}

const tier = () => screen.getByTestId("probe").textContent;

describe("useNoteLayoutTier", () => {
  it("挂载时就量一次,不等第一次 resize", () => {
    // 等 ResizeObserver 首次回调的话,首帧会闪一下 standard 的布局。
    width = 400;
    render(<Probe />);
    expect(tier()).toBe("compact");
  });

  it("宽度变化跟着换档", () => {
    width = 400;
    render(<Probe />);
    expect(tier()).toBe("compact");

    width = 1200;
    act(() => triggerResize());
    expect(tier()).toBe("wide");

    width = 700;
    act(() => triggerResize());
    expect(tier()).toBe("standard");
  });

  it("回差范围内的抖动不换档", () => {
    width = 400;
    render(<Probe />);
    // 570 已经过了 560,但没过 560+24 —— 还该留在 compact。
    width = 570;
    act(() => triggerResize());
    expect(tier()).toBe("compact");
  });

  it("卸载后断开观察", () => {
    width = 400;
    const view = render(<Probe />);
    // 挂载时量过一次,从这里开始数。
    const baseline = measureCount;
    view.unmount();
    act(() => triggerResize());
    // 观察没断开的话,回调还握着那个已卸载的元素,会再量一次。
    expect(measureCount).toBe(baseline);
  });
});
