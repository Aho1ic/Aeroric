export interface RequestSequence {
  next: () => number;
  invalidate: () => number;
  isCurrent: (sequence: number) => boolean;
}

/**
 * Monotonic request token used to ignore late responses when a user switches
 * database connections or workspace objects before a request completes.
 */
export function createRequestSequence(): RequestSequence {
  let current = 0;
  return {
    next: () => {
      current += 1;
      return current;
    },
    invalidate: () => {
      current += 1;
      return current;
    },
    isCurrent: (sequence) => sequence === current,
  };
}
