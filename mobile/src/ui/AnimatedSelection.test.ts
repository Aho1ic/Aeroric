import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const pressHandlers = vi.hoisted((): Array<() => void> => []);

vi.mock("react-native", async () => {
  const { createElement } = await import("react");
  const Host = ({ children }: { children?: ReactNode }) => createElement("div", null, children);

  return {
    Animated: {
      Value: class {
        setValue() {}
      },
      View: Host,
      parallel: () => ({ start() {} }),
      timing: () => ({}),
    },
    ScrollView: Host,
    StyleSheet: {
      create: <T>(styles: T) => styles,
      hairlineWidth: 1,
    },
    Text: Host,
    View: Host,
  };
});

vi.mock("./AnimatedPressable", async () => {
  const { createElement } = await import("react");
  return {
    AnimatedPressable: ({
      children,
      onPress,
      style,
    }: {
      children?: ReactNode;
      onPress: () => void;
      style?: unknown;
    }) => {
      const flattened = (
        Array.isArray(style) ? style.flat(Number.POSITIVE_INFINITY) : [style]
      ).filter(Boolean);
      const hasDivider = flattened.some(
        (entry) => typeof entry === "object" && entry !== null && "borderLeftWidth" in entry,
      );
      pressHandlers.push(onPress);
      return createElement("button", { "data-divider": String(hasDivider) }, children);
    },
  };
});

import { AnimatedSelection } from "./AnimatedSelection";

describe("AnimatedSelection", () => {
  beforeEach(() => {
    pressHandlers.length = 0;
  });

  it("renders two dividers for three providers and keeps selection callbacks", () => {
    const changes: string[] = [];
    const html = renderToStaticMarkup(
      createElement(AnimatedSelection, {
        value: "anthropic",
        options: [
          { value: "anthropic", label: "Anthropic" },
          { value: "openai", label: "OpenAI" },
          { value: "deepseek", label: "DeepSeek" },
        ],
        onChange: (value) => changes.push(value),
        showDividers: true,
      }),
    );

    expect(html.match(/data-divider="true"/g)).toHaveLength(2);
    expect(html.match(/data-divider="false"/g)).toHaveLength(1);

    pressHandlers[1]?.();
    expect(changes).toEqual(["openai"]);
  });
});
