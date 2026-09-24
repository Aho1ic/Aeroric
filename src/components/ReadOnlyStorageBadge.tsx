import { useEffect, useState } from "react";
import { Lock } from "lucide-react";
import { useI18n } from "../i18n";
import {
  readOnlyStorageDegradation,
  readStartupDegradations,
} from "../state/startupDiagnostics";
import type { StartupDegradation } from "../types";

/**
 * The persistent half of ADR-027 decision 4. The startup toast disappears; a second instance
 * over the same data directory stays read-only for the whole session, and every write it
 * refuses is refused silently by design (it must not steal the lease and must not fall back to
 * a temp directory). Without a standing indicator the user's next surprise is that nothing they
 * did was saved — which is the outcome the ADR calls out as unacceptable.
 *
 * Deliberately plain text + icon in the sidebar footer's existing size/color idiom: ADR-027
 * says to reuse the existing banner/status form rather than invent a control.
 */
export function ReadOnlyStorageBadge() {
  const { t } = useI18n();
  const [degradation, setDegradation] = useState<StartupDegradation | null>(null);

  useEffect(() => {
    let cancelled = false;
    readStartupDegradations().then((rows) => {
      if (cancelled) return;
      setDegradation(readOnlyStorageDegradation(rows));
    });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!degradation) return null;

  return (
    <span
      data-testid="read-only-storage-badge"
      title={t("app.readOnlyStorageHint", { dir: degradation.fallback })}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        color: "var(--warning)",
        fontSize: 12,
      }}
    >
      <Lock size={12} aria-hidden />
      {t("app.readOnlyStorageBadge")}
    </span>
  );
}
