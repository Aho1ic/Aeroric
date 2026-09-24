import { invoke } from "../lib/api/invoke";
import type { StartupDegradation } from "../types";

/**
 * ADR-027 decision 4: a refused writer lease is a state that lasts the whole session, so it
 * needs an indicator that does not expire with a toast. The ADR says plainly that reusing
 * `StartupDegradation` is *not* the same as satisfying "continuously visible".
 *
 * There is deliberately no cached promise here. A module-level cache made the startup-toast
 * tests feed each other's rows (three of them went red for exactly that reason), and the only
 * thing worth sharing between the two consumers is the *meaning* of a record, not the fetch —
 * the backend answer cannot change within a launch either way.
 */
export function readStartupDegradations(): Promise<StartupDegradation[]> {
  return invoke<StartupDegradation[]>("list_startup_degradations")
    .then((rows) => (Array.isArray(rows) ? rows : []))
    .catch((error: unknown) => {
      // A diagnostics read must not take the app down, but it must not look like "no
      // degradations" either: say so on the console, the way the toast hook did before.
      console.error("list_startup_degradations failed", error);
      return [];
    });
}

/**
 * The read-only case, named by the component the adapter records: `CoreMode::ReadOnlyBusy`
 * writes `storage-lease`, while a cloud-synced or unwritable directory writes
 * `storage-location`. The second is a degraded *writer*, not a read-only instance, so it must
 * not light the badge — a badge that fires on the wrong record would tell the user their edits
 * are lost when they are being saved.
 */
export function readOnlyStorageDegradation(
  rows: StartupDegradation[],
): StartupDegradation | null {
  return rows.find((row) => row.component === "storage-lease") ?? null;
}
