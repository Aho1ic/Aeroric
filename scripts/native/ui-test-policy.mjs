import { readFileSync } from "node:fs";

export const UI_TEST_POLICY_EXIT_CODE = 78;

// Version 1 is deny-only. Resumption needs reviewed source AND policy changes,
// not an environment variable, CLI argument, token or an "allowed" JSON value.
const expectedPolicy = Object.freeze({
  version: 1,
  date: "2026-10-19",
  realUITests: "paused",
  vmProvisioning: "forbidden",
  resumesOnlyBy: "explicit new user authorization and reviewed policy update",
});

function denied(reason) {
  const error = new Error(
    `PAUSED: real UI tests NOT EXECUTED / NOT PASSED; VM provisioning FORBIDDEN. ${reason} ` +
    "See .agents/notes/implemented/process/2026-10-19-ui-automation-pause.md.",
  );
  error.code = "ERR_AERORIC_UI_TEST_POLICY";
  error.exitCode = UI_TEST_POLICY_EXIT_CODE;
  return error;
}

export function readUITestPolicy() {
  let policy;
  try {
    policy = JSON.parse(readFileSync(new URL("./ui-test-policy.json", import.meta.url), "utf8"));
  } catch {
    // Never expose filesystem paths, raw policy content or parser diagnostics.
    throw denied("Policy missing, unreadable or invalid; fail-closed.");
  }
  if (policy === null || typeof policy !== "object" || Array.isArray(policy)
      || Object.keys(policy).length !== Object.keys(expectedPolicy).length
      || !Object.keys(expectedPolicy).every((key) => policy[key] === expectedPolicy[key])) {
    throw denied("Unsupported policy schema or state; fail-closed.");
  }
  return Object.freeze(policy);
}

export function assertRealUITestsAllowed() {
  readUITestPolicy();
  throw denied("Explicit new user authorization and reviewed policy/source updates are required to resume.");
}

// A reusable entry-point check, not an OS-level interceptor for arbitrary VM tools.
export function assertVMProvisioningAllowed() {
  readUITestPolicy();
  throw denied("VM provisioning is forbidden by project policy.");
}
