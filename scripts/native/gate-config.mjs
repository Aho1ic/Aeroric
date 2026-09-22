// Shared by scripts/native/check.mjs (the gate runner) and its tests.
//
// check.mjs is a script with top-level side effects and cannot be imported without
// running it, so the one rule that decides whether a module counts as a real gate
// lives here instead of being duplicated.
/**
 * One module is "ready" only if it is registered **and** declares at least one
 * runnable case with a usable shape.
 *
 * Measured reason this exists: `native:check protocol` with `"cases": []` used to
 * print "0 case(s) passed" and **exit 0** — a green gate that certified nothing,
 * reachable the moment P2-01 registers its module. `all` had the same hole, and its
 * error text claimed to test for exactly that while only testing whether the module
 * key existed.
 */
/**
 * A scan that "found nothing" is only a pass if it could have found something.
 *
 * Measured reason: with `tauri::AppHandle` sitting in `core/crates/aeroric-core/src/`,
 * adding one line to a `.gitignore` in that same directory made the INV-02 guard exit 1
 * with empty output — i.e. report *no forbidden matches* while the violation was on
 * disk. An empty directory behaves the same way. Each `absent` case therefore declares
 * `controls`: one probe per scanned path that must find content, or the case fails.
 */
export function absenceControlFailure(name, controlArgv, exitCode) {
  if (exitCode === 0) return null;
  const target = controlArgv[controlArgv.length - 1] ?? controlArgv.join(" ");
  return `${name}: control probe on "${target}" found no content (exit ${exitCode}) — refusing to read an unreadable or empty corpus as absence`;
}

/** `forbid`-style purity checks need a positive control too: the output must be the
 *  output of a real resolution, not an empty or truncated one. */
export function missingOutputTokens(testCase, haystack) {
  return (testCase.mustContain ?? []).filter((token) => !String(haystack).includes(String(token)));
}

export function moduleGateProblem(name, entry) {
  if (!entry) return `${name}: not registered in checks.json`;
  if (!Array.isArray(entry.cases)) return `${name}: "cases" is missing or not an array`;
  if (entry.cases.length === 0) return `${name}: registered with zero cases — a gate that runs nothing cannot certify anything`;
  const malformed = entry.cases.filter(
    (c) => !c || typeof c !== "object" || typeof c.name !== "string" || !c.name
      || !Array.isArray(c.argv) || c.argv.length === 0 || typeof c.argv[0] !== "string",
  );
  if (malformed.length) return `${name}: ${malformed.length} case(s) without a name or a non-empty string argv`;
  // A new absence guard that could pass without ever reading content must not be
  // registerable — that is how INV-02's scan got defeated by one ignore file.
  const uncontrolled = entry.cases.filter(
    (c) => c.kind === "absent"
      && (!Array.isArray(c.controls) || c.controls.length === 0
        || c.controls.some((a) => !Array.isArray(a) || a.length === 0 || typeof a[0] !== "string")),
  );
  if (uncontrolled.length) {
    return `${name}: ${uncontrolled.length} "absent" case(s) declare no usable controls (${uncontrolled.map((c) => c.name).join(", ")}) — they cannot prove the corpus was read`;
  }
  const unassertive = entry.cases.filter((c) => c.kind === "forbid" && (!Array.isArray(c.mustContain) || c.mustContain.length === 0));
  if (unassertive.length) {
    return `${name}: ${unassertive.length} "forbid" case(s) declare no mustContain (${unassertive.map((c) => c.name).join(", ")}) — an empty listing would pass them`;
  }
  return null;
}
