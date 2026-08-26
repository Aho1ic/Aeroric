/**
 * 数据库工作区顶栏的展示层 —— 一排「新建连接 / 新建查询 / …」的入口按钮。
 *
 * 从 `DatabaseView.tsx` 抽出时保持按钮顺序、图标与文案 key 逐字不变,以免影响已有的
 * `database-view-*` 用例。这里不持有任何状态,动作全部由 `DatabaseView` 传进来。
 */

import {
  Database,
  FileCode,
  FilePlus,
  GitCompare,
  GitMerge,
  Network,
  Search,
  SlidersHorizontal,
  Table2,
  UsersRound,
  Wrench,
} from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button as DbxButton } from "../ui/Button";
import type { DatabaseAdvancedToolMode } from "./DatabaseAdvancedTools";

/** 顶栏动作里有几个是 async 的,原来就是「点了不等」,所以统一允许返回 Promise。 */
type ToolbarAction = () => void | Promise<void>;

export interface DatabaseTopToolbarProps {
  /** 当前选中的连接能不能跑 SQL;不能就只禁用「新建查询」。 */
  sqlCapable: boolean;
  /** DatabaseView 的全局忙碌态,只挡「执行 SQL 文件」。 */
  busy: boolean;
  onNewConnection: ToolbarAction;
  onNewQuery: ToolbarAction;
  onExecuteSqlFile: ToolbarAction;
  onOpenDriverManager: ToolbarAction;
  onOpenAdvancedTool: (mode: DatabaseAdvancedToolMode) => void;
  onOpenUserAdmin: ToolbarAction;
  onOpenErDiagram: ToolbarAction;
  onOpenDatabaseSearch: ToolbarAction;
  onOpenTableStructure: ToolbarAction;
}

export function DatabaseTopToolbar({
  sqlCapable,
  busy,
  onNewConnection,
  onNewQuery,
  onExecuteSqlFile,
  onOpenDriverManager,
  onOpenAdvancedTool,
  onOpenUserAdmin,
  onOpenErDiagram,
  onOpenDatabaseSearch,
  onOpenTableStructure,
}: DatabaseTopToolbarProps) {
  const { t } = useI18n();

  return (
    <div style={s.databaseTopToolbar}>
      <DbxButton variant="ghost" size="sm" icon={Database} onClick={onNewConnection}>
        {t("database.newConnection")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={FilePlus}
        onClick={onNewQuery}
        disabled={!sqlCapable}
      >
        {t("database.newQuery")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={FileCode}
        onClick={onExecuteSqlFile}
        disabled={busy}
      >
        {t("database.executeSqlFile")}
      </DbxButton>
      <DbxButton variant="ghost" size="sm" icon={Wrench} onClick={onOpenDriverManager}>
        {t("database.driverManager")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={GitMerge}
        onClick={() => onOpenAdvancedTool("transfer")}
      >
        {t("database.dataTransfer")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={GitCompare}
        onClick={() => onOpenAdvancedTool("schema-diff")}
      >
        {t("database.schemaDiff")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={Network}
        onClick={() => onOpenAdvancedTool("data-compare")}
      >
        {t("database.dataCompare")}
      </DbxButton>
      <DbxButton variant="ghost" size="sm" icon={UsersRound} onClick={onOpenUserAdmin}>
        {t("database.userAdmin")}
      </DbxButton>
      <DbxButton variant="ghost" size="sm" icon={Table2} onClick={() => void onOpenErDiagram()}>
        {t("database.erDiagram")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={Search}
        onClick={() => void onOpenDatabaseSearch()}
      >
        {t("database.databaseSearch")}
      </DbxButton>
      <DbxButton
        variant="ghost"
        size="sm"
        icon={SlidersHorizontal}
        onClick={() => void onOpenTableStructure()}
      >
        {t("database.tableStructure")}
      </DbxButton>
    </div>
  );
}
