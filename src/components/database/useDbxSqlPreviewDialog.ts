/**
 * 「预览 SQL」对话框的状态。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与同目录的 `useTableImportDialog.ts` 等一致。
 *
 * 这一簇的特点是**记录与展示分离**:改单元格 / 保存网格改动 / 删行三条链路在弹
 * confirm 之前先把后端算出的语句 `record()` 下来,对话框本身要等用户点工具栏上的
 * 「预览 SQL」才显示。所以 `visible` 与那三份内容是各自独立的 state ——
 * 关掉对话框不清语句,下次还能翻回来看同一份。
 */

import { useCallback, useState } from "react";

export interface DbxSqlPreviewRecord {
  statements: string[];
  rollbackStatements: string[];
}

export interface DbxSqlPreviewDialogState {
  visible: boolean;
  statements: string[];
  rollback: string[];
  description: string;
  /** 工具栏那颗「预览 SQL」按钮只在录到过语句之后才渲染。 */
  hasStatements: boolean;
  /** 记录最近一次预览结果,不打开对话框。 */
  record: (preview: DbxSqlPreviewRecord, description: string) => void;
  show: () => void;
  close: () => void;
}

export function useDbxSqlPreviewDialog(): DbxSqlPreviewDialogState {
  const [visible, setVisible] = useState(false);
  const [statements, setStatements] = useState<string[]>([]);
  const [rollback, setRollback] = useState<string[]>([]);
  const [description, setDescription] = useState("");

  const record = useCallback((preview: DbxSqlPreviewRecord, nextDescription: string) => {
    setStatements(preview.statements);
    setRollback(preview.rollbackStatements);
    setDescription(nextDescription);
  }, []);

  const show = useCallback(() => setVisible(true), []);
  const close = useCallback(() => setVisible(false), []);

  return {
    visible,
    statements,
    rollback,
    description,
    hasStatements: statements.length > 0,
    record,
    show,
    close,
  };
}
