import { useEffect, useMemo, useState } from "react";
import { Columns2, FileCode, Rows3, X } from "lucide-react";
import { DiffFileBlock } from "./git-diff/DiffFileBlock";
import { parseDiff } from "./git-diff/parse";
import type { DiffViewMode } from "./git-diff/types";
import { load, save } from "../utils";
import { useI18n } from "../i18n";
import {
  formatInvokeError,
  invokeWithTimeout,
  remoteInvokeOptions,
} from "../hooks/useCancellableInvoke";
import s from "../styles";
import type { RemoteProjectTarget } from "../types";
import { AnimatedSelectionGroup } from "./ui/AnimatedSelection";
import { GIT_MIRRORS } from "../lib/api/git";
import { invokeProjectFor, resolveCommand } from "../lib/invokeFacade";
import { resolveInvokeTarget } from "../lib/target";

const VIEW_MODE_KEY = "aeroric.diffViewMode";

interface Props {
  projectPath: string;
  // "commit" = full commit diff, "file" = working-tree file diff, "commit-file" = single file in a commit
  mode: "commit" | "file" | "commit-file";
  commitHash?: string;
  filePath?: string;
  staged?: boolean;
  title: string;
  onClose: () => void;
  remote?: RemoteProjectTarget;
}

export function GitDiffViewer({
  projectPath,
  mode,
  commitHash,
  filePath,
  staged,
  title,
  onClose,
  remote,
}: Props) {
  const { t } = useI18n();
  const [diff, setDiff] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<DiffViewMode>(() =>
    load<DiffViewMode>(VIEW_MODE_KEY, "unified"),
  );

  useEffect(() => {
    save(VIEW_MODE_KEY, viewMode);
  }, [viewMode]);

  useEffect(() => {
    setLoading(true);
    setError(null);

    const loadDiff = async () => {
      try {
        const target = resolveInvokeTarget(projectPath, remote);
        const isRemote = Boolean(remote);
        let result: string;
        if (mode === "commit" && commitHash) {
          const command = resolveCommand(GIT_MIRRORS.showCommitDiff, target);
          const promise = invokeProjectFor<string>(target, GIT_MIRRORS.showCommitDiff, {
            commitHash,
          });
          result = await (isRemote
            ? invokeWithTimeout(promise, command, remoteInvokeOptions())
            : promise);
        } else if (mode === "commit-file" && commitHash && filePath !== undefined) {
          const command = resolveCommand(GIT_MIRRORS.showFileDiff, target);
          const promise = invokeProjectFor<string>(target, GIT_MIRRORS.showFileDiff, {
            commitHash,
            filePath,
          });
          result = await (isRemote
            ? invokeWithTimeout(promise, command, remoteInvokeOptions())
            : promise);
        } else if (mode === "file" && filePath !== undefined) {
          const command = resolveCommand(GIT_MIRRORS.fileDiff, target);
          const promise = invokeProjectFor<string>(target, GIT_MIRRORS.fileDiff, {
            filePath,
            staged: staged ?? false,
          });
          result = await (isRemote
            ? invokeWithTimeout(promise, command, remoteInvokeOptions())
            : promise);
        } else {
          result = "";
        }
        setDiff(result);
      } catch (e) {
        setError(formatInvokeError(e));
      } finally {
        setLoading(false);
      }
    };

    loadDiff();
  }, [projectPath, mode, commitHash, filePath, staged, remote]);

  const { parsedFiles, totalAdditions, totalDeletions } = useMemo(() => {
    const files = parseDiff(diff, projectPath);
    let add = 0;
    let del = 0;
    for (const f of files) {
      add += f.additions;
      del += f.deletions;
    }
    return { parsedFiles: files, totalAdditions: add, totalDeletions: del };
  }, [diff, projectPath]);

  return (
    <div style={s.diffViewer}>
      <div style={s.diffHeader}>
        <FileCode size={15} color="var(--text-muted)" />
        <div style={s.diffHeaderTitleWrap}>
          <div style={s.diffHeaderTitle}>{title}</div>
          <div style={s.diffHeaderMeta}>
            <span>
              {t(parsedFiles.length === 1 ? "common.fileChanged" : "common.filesChanged", {
                count: parsedFiles.length,
              })}
            </span>
            <span style={s.diffAddCount}>+{totalAdditions}</span>
            <span style={s.diffDeleteCount}>-{totalDeletions}</span>
          </div>
        </div>

        <AnimatedSelectionGroup
          value={viewMode}
          onChange={setViewMode}
          ariaLabel={t("git.diffViewMode")}
          options={[
            {
              value: "unified",
              label: <Rows3 size={15} />,
              title: t("git.singleColumnDiff"),
              ariaLabel: t("git.singleColumnDiff"),
            },
            {
              value: "split",
              label: <Columns2 size={15} />,
              title: t("git.twoColumnDiff"),
              ariaLabel: t("git.twoColumnDiff"),
            },
          ]}
          style={s.diffViewToggle}
          itemStyle={{ width: 28, minHeight: 24, padding: 0 }}
        />

        <button
          type="button"
          onClick={onClose}
          title={t("git.closeDiff")}
          aria-label={t("git.closeDiff")}
          style={s.diffCloseBtn}
        >
          <X size={15} />
        </button>
      </div>

      <div style={s.diffContent}>
        {loading ? (
          <div style={s.diffStateMessage}>{t("git.loadingDiff")}</div>
        ) : error ? (
          <div style={s.diffStateError}>{error}</div>
        ) : diff.trim() === "" ? (
          <div style={s.diffStateMessage}>{t("git.noChanges")}</div>
        ) : (
          <div style={s.diffFileList}>
            {parsedFiles.map((file, index) => (
              <DiffFileBlock key={`${file.displayPath}-${index}`} file={file} viewMode={viewMode} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
