import { Fragment, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import { segmentDshProse } from "../dshDeliverables";

/**
 * Message prose with the produced files it names turned into openers.
 *
 * Clicking a reference goes through `open_dsh_host_path`, the same Host opener
 * the produced-files panel uses, with the full path as the button's title — the
 * disambiguator when a turn produced two files that share a basename. Prose
 * with nothing to resolve renders as the plain text it was.
 *
 * @param prose - The message text, as the Harness delivered it.
 * @param paths - Produced paths this message may reference.
 * @param className - Class of the wrapper, so callers keep their own layout.
 */
export function DshMentionProse({
  prose,
  paths,
  className,
}: {
  prose: string;
  paths: readonly string[];
  className?: string;
}) {
  const { t } = useI18n();
  const segments = useMemo(() => segmentDshProse(prose, paths), [paths, prose]);
  return (
    <div className={className}>
      {segments.map((segment, index) =>
        segment.kind === "text" ? (
          <Fragment key={index}>{segment.text}</Fragment>
        ) : (
          <button
            key={index}
            type="button"
            className="dsh-file-mention"
            title={segment.path}
            aria-label={t("dsh.deliverables.open", { path: segment.path })}
            onClick={() => {
              // Opening is best-effort: the Host answers or it does not, and the
              // prose stays readable either way.
              void invoke("open_dsh_host_path", { path: segment.path }).catch(() => {});
            }}
          >
            {segment.token}
          </button>
        ),
      )}
    </div>
  );
}
