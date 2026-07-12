import { describe, expect, it } from "vitest";
import {
  readModelReasoningEffort,
  setModelReasoningEffort,
} from "../components/app-settings/reasoningEffort";

describe("Codex reasoning effort config", () => {
  it("reads a supported effort from the root config", () => {
    expect(readModelReasoningEffort('model = "gpt-5"\nmodel_reasoning_effort = "high"\n')).toBe(
      "high",
    );
    expect(readModelReasoningEffort('model_reasoning_effort = "unsupported"\n')).toBeNull();
    expect(
      readModelReasoningEffort('[profiles.work]\nmodel_reasoning_effort = "high"\n'),
    ).toBeNull();
  });

  it("updates the effort while preserving comments and other settings", () => {
    const content =
      '# personal config\nmodel = "gpt-5"\nmodel_reasoning_effort = "medium" # keep\n\n[features]\nweb_search = true\n';

    expect(setModelReasoningEffort(content, "high")).toBe(
      '# personal config\nmodel = "gpt-5"\nmodel_reasoning_effort = "high" # keep\n\n[features]\nweb_search = true\n',
    );
  });

  it("adds and removes the root effort setting", () => {
    const content = 'model = "gpt-5"\n';
    expect(setModelReasoningEffort(content, "high")).toBe(
      'model_reasoning_effort = "high"\nmodel = "gpt-5"\n',
    );
    expect(
      setModelReasoningEffort('model_reasoning_effort = "high"\nmodel = "gpt-5"\n', null),
    ).toBe('model = "gpt-5"\n');
  });

  it("does not replace a similarly named setting inside a table", () => {
    const content = '[profiles.work]\nmodel_reasoning_effort = "low"\n';
    expect(setModelReasoningEffort(content, "high")).toBe(
      'model_reasoning_effort = "high"\n[profiles.work]\nmodel_reasoning_effort = "low"\n',
    );
  });
});
