import { describe, expect, it } from "vitest";
import {
  readModelReasoningSpeed,
  setModelReasoningSpeed,
} from "../components/app-settings/reasoningSpeed";

describe("Agent reasoning speed config", () => {
  it("reads a supported speed from the root TOML config", () => {
    expect(readModelReasoningSpeed('model = "gpt-5"\nmodel_reasoning_speed = "fast"\n')).toBe(
      "fast",
    );
    expect(readModelReasoningSpeed('model_reasoning_speed = "turbo"\n')).toBeNull();
    // A speed nested under a table header is ignored.
    expect(
      readModelReasoningSpeed('[profiles.work]\nmodel_reasoning_speed = "fast"\n'),
    ).toBeNull();
  });

  it("reads a speed from a root JSON config", () => {
    expect(
      readModelReasoningSpeed(
        '{\n  "model": "claude",\n  "model_reasoning_speed": "fast"\n}\n',
      ),
    ).toBe("fast");
    expect(readModelReasoningSpeed('{"model_reasoning_speed": "standard"}')).toBe("standard");
    expect(readModelReasoningSpeed('{"model_reasoning_speed": 5}')).toBeNull();
  });

  it("updates the speed while preserving comments and other settings (TOML)", () => {
    const content =
      '# personal config\nmodel = "gpt-5"\nmodel_reasoning_speed = "standard" # keep\n\n[features]\nweb_search = true\n';

    expect(setModelReasoningSpeed(content, "fast")).toBe(
      '# personal config\nmodel = "gpt-5"\nmodel_reasoning_speed = "fast" # keep\n\n[features]\nweb_search = true\n',
    );
  });

  it("adds and removes the root speed setting (TOML)", () => {
    const content = 'model = "gpt-5"\n';
    expect(setModelReasoningSpeed(content, "fast")).toBe(
      'model_reasoning_speed = "fast"\nmodel = "gpt-5"\n',
    );
    expect(
      setModelReasoningSpeed('model_reasoning_speed = "fast"\nmodel = "gpt-5"\n', null),
    ).toBe('model = "gpt-5"\n');
  });

  it("writes and removes the speed in a JSON config", () => {
    const content = '{\n  "model": "claude"\n}\n';
    expect(setModelReasoningSpeed(content, "fast")).toBe(
      '{\n  "model": "claude",\n  "model_reasoning_speed": "fast"\n}\n',
    );
    expect(
      setModelReasoningSpeed(
        '{\n  "model": "claude",\n  "model_reasoning_speed": "fast"\n}\n',
        null,
      ),
    ).toBe('{\n  "model": "claude"\n}\n');
  });
});
