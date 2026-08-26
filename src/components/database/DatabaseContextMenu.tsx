/**
 * 数据库侧栏 / 网格的右键菜单展示层。
 *
 * 从 `DatabaseView.tsx` 抽出。原来是一条按 `contextMenu.kind` 分支的十几层三元
 * 表达式,每一支自己拼一份 `[action, labelKey]` 列表再 `.map` 出结构完全一样的
 * `<button role="menuitem">`。这里把「按钮长什么样」收进 `ContextMenuItem`,分支
 * 本身保持原样逐字照搬 —— 包括每一支各自的图标集合(比如连接级菜单的 `refresh`
 * 一直是没有图标的,不能顺手套用别处的 action→图标映射)、disabled 条件和
 * `menuitemcheckbox` 那一支的 grid 布局。
 *
 * 动作分发仍按 kind 拆成独立回调(`actions.*`),这样每一支 `.map` 里的 action
 * 还是字面量联合类型,不用往 `runXxx` 里塞 cast。
 */

import type { ComponentType, CSSProperties, ReactNode } from "react";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Copy,
  Database,
  Eraser,
  Eye,
  FilePlus,
  Pin,
  Play,
  Plus,
  RefreshCcw,
  Search,
  Trash2,
  UsersRound,
} from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import type { AeroricDbConnectionConfig, DbQueryResult } from "../../types";
import {
  canCreateDatabaseForConnection,
  dbxConnectionFinalProxyPort,
  dbxConnectionLocalFilePath,
  dbxDatabaseContextMenuItems,
  dbxObjectContextMenuItems,
  dbxSchemaContextMenuItems,
  dbxTableChildDropLabelKey,
  hasEnabledDbxTransportLayers,
  mongoDocumentRawId,
  noSqlCollectionContextMenuItems,
  noSqlDatabaseContextMenuItems,
  sqliteBackupSourcePath,
} from "./databaseViewModel";
import { dbxGridColumnSortable } from "../../lib/databaseUtils";
import { supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import type {
  DatabaseContextMenuState,
  DbxDatabaseContextMenuAction,
  DbxObjectContextMenuAction,
  DbxSchemaContextMenuAction,
  NoSqlContextMenuAction,
  WorkspaceTabContextMenuAction,
} from "./databaseViewModel";
import type {
  DbxGridCellContextMenuAction,
  DbxGridHeaderContextMenuAction,
} from "./databaseGridState";

type Icon = ComponentType<{ size?: number }>;

/**
 * 菜单项。默认是 flex 布局的 `role=menuitem`,图标缺省就不占位;
 * `iconSlot` 打开后改成 grid 布局并固定留出 16px 的图标 / 勾选槽 —— 标签页那一支
 * 需要所有项对齐,`checked` 再把该项升成 `menuitemcheckbox`。
 */
function ContextMenuItem({
  icon: IconComponent,
  iconSlot,
  label,
  danger,
  disabled,
  checked,
  onClick,
}: {
  icon?: Icon;
  iconSlot?: boolean;
  label: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  onClick: () => void;
}) {
  const style: CSSProperties = {
    ...s.fileCtxMenuItem,
    ...(iconSlot
      ? { display: "grid", gridTemplateColumns: "16px minmax(0, 1fr)" }
      : { display: "flex" }),
    alignItems: "center",
    gap: 8,
    ...(danger ? { color: "var(--danger)" } : {}),
  };
  return (
    <button
      type="button"
      role={checked === undefined ? "menuitem" : "menuitemcheckbox"}
      aria-checked={checked}
      disabled={disabled}
      style={style}
      onClick={onClick}
    >
      {iconSlot ? (
        <span
          style={{
            width: 16,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          {checked ? "✓" : IconComponent ? <IconComponent size={13} /> : ""}
        </span>
      ) : (
        IconComponent && <IconComponent size={13} />
      )}
      <span>{label}</span>
    </button>
  );
}

export interface DatabaseContextMenuActions {
  /** 连接节点(含 legacy)与 user-admin 节点。 */
  connection: (action: ConnectionContextMenuAction) => void;
  connectionGroup: (action: ConnectionGroupContextMenuAction) => void;
  database: (action: DbxDatabaseContextMenuAction) => void;
  schema: (action: DbxSchemaContextMenuAction) => void;
  object: (action: DbxObjectContextMenuAction) => void;
  objectGroup: (action: DbxObjectGroupContextMenuAction) => void;
  tableChild: (action: DbxTableChildContextMenuAction) => void;
  column: (action: DbxColumnContextMenuAction) => void;
  noSql: (action: NoSqlContextMenuAction) => void;
  gridHeader: (action: DbxGridHeaderContextMenuAction) => void;
  gridCell: (action: DbxGridCellContextMenuAction) => void;
  workspaceTab: (action: WorkspaceTabContextMenuAction) => void;
}

export type ConnectionContextMenuAction =
  | "open"
  | "close"
  | "newQuery"
  | "queryHistory"
  | "executeSqlFile"
  | "userAdmin"
  | "createDatabase"
  | "copyFinalProxyPort"
  | "selectVisibleDatabases"
  | "edit"
  | "revealDatabaseFile"
  | "backupSqliteDatabase"
  | "togglePin"
  | "moveToGroup"
  | "refresh"
  | "copy"
  | "delete";
export type ConnectionGroupContextMenuAction =
  | "copyName"
  | "newConnection"
  | "newGroup"
  | "renameGroup"
  | "deleteGroup";
export type DbxObjectGroupContextMenuAction = "createTable" | "createView" | "refresh";
export type DbxTableChildContextMenuAction = "copyName" | "dropTableChildObject";
export type DbxColumnContextMenuAction = "copyName" | "openFieldLineage" | "dropColumn";

/** 右键落点解析出来的连接:每种 kind 只用得上其中一个,拿不到就不渲染那一支。 */
export interface DatabaseContextMenuConnections {
  dbx: AeroricDbConnectionConfig | null;
  database: AeroricDbConnectionConfig | null;
  schema: AeroricDbConnectionConfig | null;
  object: AeroricDbConnectionConfig | null;
  objectGroup: AeroricDbConnectionConfig | null;
  tableChild: AeroricDbConnectionConfig | null;
  column: AeroricDbConnectionConfig | null;
  noSql: AeroricDbConnectionConfig | null;
}

export interface DatabaseContextMenuProps {
  menu: NonNullable<DatabaseContextMenuState>;
  onDismiss: () => void;
  connections: DatabaseContextMenuConnections;
  /** 连接级菜单的两个额外标记:当前连接是否已连上、有没有可移动的分组。 */
  connectionActive: boolean;
  connectionHasMoveTargets: boolean;
  /** 该树节点是否已固定(pin)。 */
  treeNodePinned: boolean;
  shortWorkspaceTabIds: Set<string>;
  grid: {
    orderByInput: string;
    queryResult: DbQueryResult | null;
    primaryKeys: string[];
    /** 网格里选中的行数:>1 时复制类文案换成复数 key。 */
    cellRowCount: number;
  };
  actions: DatabaseContextMenuActions;
}

/** 网格表头与单元格两支共用:两边重叠的 action 图标本来就一致。 */
const GRID_ICONS: Partial<Record<DbxGridCellContextMenuAction, Icon>> = {
  copyValue: Copy,
  copyColumnName: Copy,
  copyRowJson: Copy,
  copyRowInsert: Copy,
  copyRowInsertWithoutPrimaryKeys: Copy,
  copyRowUpdate: Copy,
  copyAllTsv: Copy,
  previewValue: Eye,
  previewRow: Eye,
  previewColumn: Eye,
  sortAscending: ArrowUp,
  sortDescending: ArrowDown,
  clearSort: ArrowUpDown,
  filterEquals: Search,
  filterNotEquals: Search,
  filterLike: Search,
  filterNotLike: Search,
  filterLessThan: Search,
  filterGreaterThan: Search,
  filterIsNull: Search,
  filterIsNotNull: Search,
  clearFilter: Eraser,
};

/** 侧栏树节点各支共用的图标;哪一支渲染哪些 key 由该支自己的 action 列表决定。 */
const TREE_ICONS = {
  togglePin: Pin,
  copyName: Copy,
  newQuery: FilePlus,
  openWorkspace: Play,
  setDefaultDatabase: Database,
  clearDefaultDatabase: Database,
  refresh: RefreshCcw,
  flushRedisDb: Eraser,
  deleteRedisKey: Trash2,
  deleteDocument: Trash2,
} as const;

export function DatabaseContextMenu(props: DatabaseContextMenuProps) {
  const { t } = useI18n();
  const { menu, onDismiss } = props;

  return (
    <>
      <div style={s.fileCtxBackdrop} onClick={onDismiss} />
      <div
        role="menu"
        aria-label={menu.kind === "workspace-tab" ? t("database.tabActions") : undefined}
        style={{
          ...s.fileCtxMenu,
          left: menu.x,
          top: menu.y,
          minWidth: 190,
        }}
      >
        <ContextMenuItems {...props} />
      </div>
    </>
  );
}

/**
 * 按 kind 逐支给出菜单项。分支顺序与原来的三元链一致 —— 带连接判空的那几支拿不到
 * 连接时会**继续往下落到最后的连接菜单**,这是原有行为,不是漏判。
 */
function ContextMenuItems({
  menu,
  connections,
  connectionActive,
  connectionHasMoveTargets,
  treeNodePinned,
  shortWorkspaceTabIds,
  grid,
  actions,
}: DatabaseContextMenuProps): ReactNode {
  const { t } = useI18n();

  if (menu.kind === "dbx-grid-header") {
    const sortable = dbxGridColumnSortable(grid.queryResult, menu.columnIndex);
    return (
      [
        ["copyColumnName", "database.copyColumnName"],
        ["previewColumn", "database.openColumnDetailsDialog"],
        ["sortAscending", "database.sortAscending"],
        ["sortDescending", "database.sortDescending"],
        ...(grid.orderByInput.trim() ? ([["clearSort", "database.clearSort"]] as const) : []),
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={GRID_ICONS[action]}
        label={t(labelKey)}
        disabled={
          (action === "sortAscending" || action === "sortDescending" || action === "clearSort") &&
          !sortable
        }
        onClick={() => actions.gridHeader(action)}
      />
    ));
  }

  if (menu.kind === "workspace-tab") {
    return (
      [
        ["toggleShortTitle", "database.shortenTabTitle"],
        ["pinTab", "database.pinTab"],
        ["closeTab", "database.closeTab"],
        ["closeOtherTabs", "database.closeOtherTabs"],
        ["closeAllTabs", "database.closeAllTabs"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        iconSlot
        icon={action === "pinTab" ? Pin : undefined}
        checked={action === "toggleShortTitle" ? shortWorkspaceTabIds.has(menu.tabId) : undefined}
        label={t(labelKey)}
        onClick={() => actions.workspaceTab(action)}
      />
    ));
  }

  if (menu.kind === "dbx-grid-cell") {
    const sortable = dbxGridColumnSortable(grid.queryResult, menu.columnIndex);
    const rowCount = grid.cellRowCount;
    return (
      [
        ["copyValue", "database.copyValue"],
        ["copyColumnName", "database.copyColumnName"],
        ["previewValue", "database.previewValue"],
        ["previewRow", "database.openRowDetailsDialog"],
        ["previewColumn", "database.openColumnDetailsDialog"],
        ["sortAscending", "database.sortAscending"],
        ["sortDescending", "database.sortDescending"],
        ...(grid.orderByInput.trim() ? ([["clearSort", "database.clearSort"]] as const) : []),
        ["filterEquals", "database.filterByValue"],
        ["filterNotEquals", "database.filterExcludeValue"],
        ["filterLike", "database.filterLike"],
        ["filterNotLike", "database.filterNotLike"],
        ["filterLessThan", "database.filterLessThan"],
        ["filterGreaterThan", "database.filterGreaterThan"],
        ["filterIsNull", "database.filterIsNull"],
        ["filterIsNotNull", "database.filterIsNotNull"],
        ["clearFilter", "database.clearFilter"],
        ["copyRowJson", "database.copyRow"],
        ["copyRowInsert", "database.copyRowInsert"],
        ...(grid.primaryKeys.length
          ? ([
              ["copyRowInsertWithoutPrimaryKeys", "database.copyRowInsertWithoutPrimaryKeys"],
            ] as const)
          : []),
        ["copyRowUpdate", "database.copyRowUpdate"],
        ["copyAllTsv", "database.copyAllTsv"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={GRID_ICONS[action]}
        label={t(multiRowLabelKey(action, rowCount) ?? labelKey, { count: rowCount })}
        disabled={
          (action === "copyRowUpdate" && grid.primaryKeys.length === 0) ||
          ((action === "sortAscending" || action === "sortDescending" || action === "clearSort") &&
            !sortable)
        }
        onClick={() => actions.gridCell(action)}
      />
    ));
  }

  if (menu.kind === "dbx-table-child" && connections.tableChild) {
    return (
      [
        ["copyName", "database.copyName"],
        ["dropTableChildObject", dbxTableChildDropLabelKey(menu.childObjectType)],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={action === "copyName" ? Copy : undefined}
        label={t(labelKey)}
        danger={action === "dropTableChildObject"}
        onClick={() => actions.tableChild(action)}
      />
    ));
  }

  if (menu.kind === "dbx-object-group" && connections.objectGroup) {
    return (
      [
        ...(menu.groupKey === "tables" ? ([["createTable", "database.createTable"]] as const) : []),
        ...(menu.groupKey === "views" ? ([["createView", "database.createView"]] as const) : []),
        ["refresh", "database.refresh"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={action === "refresh" ? RefreshCcw : Plus}
        label={t(labelKey)}
        onClick={() => actions.objectGroup(action)}
      />
    ));
  }

  if (menu.kind === "connection-group") {
    return (
      [
        ["copyName", "database.copyName"],
        ["newConnection", "database.newConnection"],
        ["newGroup", "database.newConnectionGroup"],
        ["renameGroup", "database.renameConnectionGroup"],
        ["deleteGroup", "database.deleteConnectionGroup"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={
          action === "copyName"
            ? Copy
            : action === "newConnection" || action === "newGroup"
              ? Plus
              : undefined
        }
        label={t(labelKey)}
        danger={action === "deleteGroup"}
        onClick={() => actions.connectionGroup(action)}
      />
    ));
  }

  if (menu.kind === "redis-key" && connections.noSql) {
    const readOnly = connections.noSql.readOnly;
    return (
      [
        ["copyName", "database.copyName"],
        ["openWorkspace", "database.openWorkspace"],
        ["refresh", "database.refresh"],
        ["deleteRedisKey", "database.redisDeleteKey"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={TREE_ICONS[action]}
        label={t(labelKey)}
        danger={action === "deleteRedisKey"}
        disabled={action === "deleteRedisKey" && readOnly}
        onClick={() => actions.noSql(action)}
      />
    ));
  }

  if (menu.kind === "mongo-document" && connections.noSql) {
    const undeletable = connections.noSql.readOnly || mongoDocumentRawId(menu.document) == null;
    return (
      [
        ["copyName", "database.copyName"],
        ["openWorkspace", "database.openWorkspace"],
        ["refresh", "database.refresh"],
        ["deleteDocument", "database.mongoDeleteDocument"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={TREE_ICONS[action]}
        label={t(labelKey)}
        danger={action === "deleteDocument"}
        disabled={action === "deleteDocument" && undeletable}
        onClick={() => actions.noSql(action)}
      />
    ));
  }

  if (
    (menu.kind === "redis-database" ||
      menu.kind === "mongo-database" ||
      menu.kind === "mongo-collection") &&
    connections.noSql
  ) {
    const readOnly = connections.noSql.readOnly;
    const items =
      menu.kind === "mongo-collection"
        ? noSqlCollectionContextMenuItems(treeNodePinned)
        : noSqlDatabaseContextMenuItems(menu, connections.noSql, treeNodePinned);
    return items.map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={TREE_ICONS[action as keyof typeof TREE_ICONS]}
        label={t(labelKey)}
        danger={action === "flushRedisDb"}
        disabled={action === "flushRedisDb" && readOnly}
        onClick={() => actions.noSql(action)}
      />
    ));
  }

  if (menu.kind === "dbx-column" && connections.column) {
    return (
      [
        ["copyName", "database.copyName"],
        ["openFieldLineage", "database.openFieldLineage"],
        ["dropColumn", "database.dropColumn"],
      ] as const
    ).map(([action, labelKey]) => (
      <ContextMenuItem
        key={action}
        icon={action === "copyName" ? Copy : undefined}
        label={t(labelKey)}
        danger={action === "dropColumn"}
        onClick={() => actions.column(action)}
      />
    ));
  }

  if (menu.kind === "dbx-object") {
    return dbxObjectContextMenuItems(menu.object, connections.object, treeNodePinned).map(
      ([action, labelKey]) => (
        <ContextMenuItem
          key={action}
          icon={action === "togglePin" ? Pin : action === "copyName" ? Copy : undefined}
          label={t(labelKey)}
          danger={action === "dropTable" || action === "dropObject"}
          onClick={() => actions.object(action)}
        />
      ),
    );
  }

  if (menu.kind === "dbx-schema" && connections.schema) {
    return dbxSchemaContextMenuItems(connections.schema, treeNodePinned).map(
      ([action, labelKey]) => (
        <ContextMenuItem
          key={action}
          icon={action === "togglePin" ? Pin : action === "copyName" ? Copy : undefined}
          label={t(labelKey)}
          danger={action === "dropSchema"}
          onClick={() => actions.schema(action)}
        />
      ),
    );
  }

  if (menu.kind === "dbx-database" && connections.database) {
    return dbxDatabaseContextMenuItems(connections.database, menu.database, treeNodePinned).map(
      ([action, labelKey]) => (
        <ContextMenuItem
          key={action}
          icon={action === "togglePin" ? Pin : action === "copyName" ? Copy : undefined}
          label={t(labelKey)}
          danger={action === "dropDatabase"}
          onClick={() => actions.database(action)}
        />
      ),
    );
  }

  if (menu.kind === "user-admin") {
    return (
      <ContextMenuItem
        icon={UsersRound}
        label={t("database.openUserAdmin")}
        onClick={() => actions.connection("userAdmin")}
      />
    );
  }

  const dbx = connections.dbx;
  const items: Array<[ConnectionContextMenuAction, string]> = [
    ...(dbx
      ? [
          ["togglePin", dbx.pinned ? "database.unpinConnection" : "database.pinConnection"] as [
            ConnectionContextMenuAction,
            string,
          ],
        ]
      : []),
    [
      connectionActive ? "close" : "open",
      connectionActive ? "database.closeConnection" : "database.openConnection",
    ],
    ["newQuery", "database.newQuery"],
    ["queryHistory", "database.queryHistory"],
    ...(supportsDbxUserAdmin(dbx?.dbType)
      ? ([["userAdmin", "database.userAdmin"]] as Array<[ConnectionContextMenuAction, string]>)
      : []),
    ...(dbx && hasEnabledDbxTransportLayers(dbx) && dbxConnectionFinalProxyPort(dbx) != null
      ? ([["copyFinalProxyPort", "database.copyFinalProxyPort"]] as Array<
          [ConnectionContextMenuAction, string]
        >)
      : []),
    ["executeSqlFile", "database.executeSqlFile"],
    ...(canCreateDatabaseForConnection(dbx)
      ? ([
          [
            "createDatabase",
            dbx?.dbType === "duckdb" ? "database.createDuckDbFile" : "database.createDatabase",
          ],
        ] as Array<[ConnectionContextMenuAction, string]>)
      : []),
    ...(dbx
      ? ([
          [
            "moveToGroup",
            connectionHasMoveTargets ? "database.moveToGroup" : "database.moveToNewGroup",
          ],
        ] as Array<[ConnectionContextMenuAction, string]>)
      : []),
    ["refresh", "database.refresh"],
    ...(dbx
      ? ([["selectVisibleDatabases", "database.selectVisibleDatabases"]] as Array<
          [ConnectionContextMenuAction, string]
        >)
      : []),
    ...(dbx
      ? ([["edit", "database.editConnection"]] as Array<[ConnectionContextMenuAction, string]>)
      : []),
    ...(dbxConnectionLocalFilePath(dbx)
      ? ([["revealDatabaseFile", "database.revealDatabaseFile"]] as Array<
          [ConnectionContextMenuAction, string]
        >)
      : []),
    ...(sqliteBackupSourcePath(dbx)
      ? ([["backupSqliteDatabase", "database.backupSqliteDatabase"]] as Array<
          [ConnectionContextMenuAction, string]
        >)
      : []),
    ["copy", "database.duplicateConnection"],
    ["delete", "database.deleteConnection"],
  ];
  return items.map(([action, labelKey]) => (
    <ContextMenuItem
      key={action}
      icon={action === "copy" ? Copy : undefined}
      label={t(labelKey)}
      onClick={() => actions.connection(action)}
    />
  ));
}

/** 网格单元格菜单在多选行时把复制类文案换成复数 key;其余 action 返回 null 用原 key。 */
function multiRowLabelKey(action: DbxGridCellContextMenuAction, rowCount: number): string | null {
  if (rowCount <= 1) return null;
  if (action === "copyRowJson") return "database.copyRows";
  if (action === "copyRowInsert") return "database.copyRowsInsert";
  if (action === "copyRowInsertWithoutPrimaryKeys")
    return "database.copyRowsInsertWithoutPrimaryKeys";
  if (action === "copyRowUpdate") return "database.copyRowsUpdate";
  return null;
}
