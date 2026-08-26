/**
 * 「执行 SQL 文件」面板的表单状态。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与同目录的 `useTableImportDialog.ts` 等一致。
 *
 * 这里只管表单三件套(路径 / 预览 / 超时)和挑文件。真正的执行留在
 * `DatabaseView` 的 `executeSqlFileFromPanel`:它要写 `sqlResult`、`queryResult`、
 * `sql` 这些跨面板共享的结果态,搬进来只会把整张工作区的 state 拖进这个 hook。
 */

import { useCallback, useState } from "react";

import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { databaseApi } from "../../lib/databaseApi";

/** 预览只留开头一段:整个脚本可能有几十兆,塞进 <pre> 会把界面卡住。 */
const PREVIEW_LIMIT = 8000;

export interface SqlFilePanelDeps {
  /** 文件选择对话框的起始目录;没打开项目时为 undefined。 */
  projectRoot: string | undefined;
}

export interface SqlFilePanelState {
  path: string;
  preview: string;
  timeoutSecs: string;
  setPath: (path: string) => void;
  setTimeoutSecs: (timeoutSecs: string) => void;
  /** 弹系统文件框选一个 .sql,顺手拉一段预览。 */
  chooseFile: () => Promise<void>;
}

export function useSqlFilePanel({ projectRoot }: SqlFilePanelDeps): SqlFilePanelState {
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState("");
  const [timeoutSecs, setTimeoutSecs] = useState("60");

  const chooseFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQL", extensions: ["sql"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected !== "string") return;
    setPath(selected);
    try {
      setPreview((await databaseApi.readSqlFile(selected)).slice(0, PREVIEW_LIMIT));
    } catch {
      // 读不出来不算错:路径已经填上了,执行时会再报一次真正的错。
      setPreview("");
    }
  }, [projectRoot]);

  return { path, preview, timeoutSecs, setPath, setTimeoutSecs, chooseFile };
}
