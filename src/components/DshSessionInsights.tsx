/**
 * The trigger that opens the DeepSeek Harness trajectory panel.
 *
 * It sits in the live bars above the terminal while the panel it opens is
 * mounted inside the terminal's box, so both read the open flag and the event
 * count from `DshTrajectoryHost` rather than from each other.
 */

import { Activity } from "lucide-react";
import { useI18n } from "../i18n";
import { useDshTrajectory } from "./DshTrajectoryHost";

export function DshSessionInsights() {
  const { t } = useI18n();
  const { history, openAt } = useDshTrajectory();
  const count = history.features.events.length;
  return (
    <button
      type="button"
      className="dsh-view-trigger dsh-insights-trigger"
      title={t("dsh.insights.open")}
      // The tab now outlives a close, so this trigger names the view it opens
      // rather than reopening whichever one was last looked at.
      onClick={() => openAt("trajectory")}
    >
      <Activity size={13} />
      <span>{t("dsh.insights.open")}</span>
      {count > 0 && <small>{count}</small>}
    </button>
  );
}
