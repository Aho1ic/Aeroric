/**
 * Stable ordering for notebook-facing labels.
 *
 * The default `String#localeCompare` inherits the host's locale.  That made
 * the same vault render in a different order on an en-US desktop, a Chinese
 * desktop, and a Node test worker.  Notebook views intentionally use one
 * explicit Chinese pinyin collator, then a code-point tie-breaker so equal
 * collator weights never fall back to input order.  The linguistic portion is
 * still supplied by the WebView/Node ICU data; the tie-breaker makes each
 * individual runtime's result a total order, but cannot make different ICU
 * data sets produce identical pinyin weights.
 */
const NOTEBOOK_COLLATOR = new Intl.Collator("zh-Hans-u-co-pinyin", {
  numeric: true,
  sensitivity: "base",
});

function compareCodePoints(left: string, right: string): number {
  const leftPoints = [...left];
  const rightPoints = [...right];
  const length = Math.min(leftPoints.length, rightPoints.length);
  for (let index = 0; index < length; index += 1) {
    const a = leftPoints[index]!;
    const b = rightPoints[index]!;
    if (a === b) continue;
    return a < b ? -1 : 1;
  }
  return leftPoints.length - rightPoints.length;
}

/** Compare user-visible notebook text with the notebook's fixed locale policy. */
export function compareNotebookText(left: string, right: string): number {
  const collated = NOTEBOOK_COLLATOR.compare(left, right);
  return collated === 0 ? compareCodePoints(left, right) : collated;
}

/** Compare paths without applying linguistic collation. */
export function compareNotebookPath(left: string, right: string): number {
  return compareCodePoints(left, right);
}
