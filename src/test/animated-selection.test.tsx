import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AnimatedSelectionGroup, AnimatedSelectionTrack } from "../components/ui/AnimatedSelection";

function SegmentedFixture() {
  const [value, setValue] = useState("first");
  return (
    <AnimatedSelectionGroup
      value={value}
      onChange={setValue}
      ariaLabel="Example selection"
      options={[
        { value: "first", label: "First" },
        { value: "second", label: "Second" },
        { value: "third", label: "Third" },
      ]}
    />
  );
}

describe("AnimatedSelection", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("uses pressed state and supports cyclic arrow-key navigation", async () => {
    const user = userEvent.setup();
    render(<SegmentedFixture />);

    const first = screen.getByRole("button", { name: "First" });
    const second = screen.getByRole("button", { name: "Second" });
    expect(first).toHaveAttribute("aria-pressed", "true");

    first.focus();
    await user.keyboard("{ArrowRight}");
    expect(second).toHaveAttribute("aria-pressed", "true");

    await user.keyboard("{End}");
    expect(screen.getByRole("button", { name: "Third" })).toHaveAttribute("aria-pressed", "true");
  });

  it("supports vertical tablists and dynamically added custom tabs", async () => {
    const user = userEvent.setup();
    function Fixture() {
      const [value, setValue] = useState("one");
      const [extra, setExtra] = useState(false);
      return (
        <>
          <AnimatedSelectionTrack
            value={value}
            ariaLabel="Vertical tabs"
            role="tablist"
            orientation="vertical"
          >
            {["one", "two", ...(extra ? ["three"] : [])].map((item) => (
              <button
                key={item}
                type="button"
                role="tab"
                aria-selected={value === item}
                data-animated-selection-item
                data-selection-value={item}
                onClick={() => setValue(item)}
              >
                {item}
              </button>
            ))}
          </AnimatedSelectionTrack>
          <button type="button" onClick={() => setExtra(true)}>
            Add tab
          </button>
        </>
      );
    }

    render(<Fixture />);
    const tablist = screen.getByRole("tablist", { name: "Vertical tabs" });
    expect(tablist).toHaveAttribute("aria-orientation", "vertical");
    await user.click(screen.getByRole("button", { name: "Add tab" }));
    await user.click(screen.getByRole("tab", { name: "three" }));
    expect(screen.getByRole("tab", { name: "three" })).toHaveAttribute("aria-selected", "true");
  });

  it("remeasures the indicator after resize and scrolling", async () => {
    let activeLeft = 40;
    const resize = { callback: null as ResizeObserverCallback | null };
    class ResizeObserverMock {
      constructor(callback: ResizeObserverCallback) {
        resize.callback = callback;
      }
      observe() {}
      disconnect() {}
      unobserve() {}
    }
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      if (this.getAttribute("aria-label") === "Measured selection") {
        return {
          x: 10,
          y: 20,
          left: 10,
          top: 20,
          right: 210,
          bottom: 60,
          width: 200,
          height: 40,
          toJSON: () => ({}),
        };
      }
      if (this.textContent === "Second") {
        return {
          x: activeLeft,
          y: 24,
          left: activeLeft,
          top: 24,
          right: activeLeft + 60,
          bottom: 52,
          width: 60,
          height: 28,
          toJSON: () => ({}),
        };
      }
      return {
        x: 12,
        y: 24,
        left: 12,
        top: 24,
        right: 32,
        bottom: 52,
        width: 20,
        height: 28,
        toJSON: () => ({}),
      };
    });

    render(
      <AnimatedSelectionGroup
        value="second"
        onChange={() => {}}
        ariaLabel="Measured selection"
        options={[
          { value: "first", label: "First" },
          { value: "second", label: "Second" },
        ]}
      />,
    );

    const track = screen.getByRole("group", { name: "Measured selection" });
    const indicator = track.querySelector<HTMLElement>(".animated-selection__indicator");
    await waitFor(() => expect(indicator?.style.transform).toContain("translate3d(30px, 4px, 0)"));

    activeLeft = 70;
    resize.callback?.([], {} as ResizeObserver);
    await waitFor(() => expect(indicator?.style.transform).toContain("translate3d(60px, 4px, 0)"));

    Object.defineProperty(track, "scrollLeft", { value: 18, configurable: true });
    fireEvent.scroll(track);
    await waitFor(() => expect(indicator?.style.transform).toContain("translate3d(78px, 4px, 0)"));
  });
});
