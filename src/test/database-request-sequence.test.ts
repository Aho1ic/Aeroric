import { describe, expect, it } from "vitest";
import { createRequestSequence } from "../components/database/requestSequence";

describe("createRequestSequence", () => {
  it("only accepts the newest request token", () => {
    const sequence = createRequestSequence();
    const first = sequence.next();
    const second = sequence.next();

    expect(sequence.isCurrent(first)).toBe(false);
    expect(sequence.isCurrent(second)).toBe(true);
  });

  it("invalidates an in-flight request without starting another request", () => {
    const sequence = createRequestSequence();
    const request = sequence.next();

    sequence.invalidate();

    expect(sequence.isCurrent(request)).toBe(false);
    expect(sequence.next()).toBe(3);
  });
});
