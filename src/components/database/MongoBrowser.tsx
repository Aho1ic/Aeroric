import { useEffect, useMemo, useState } from "react";
import type { MouseEvent } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronLeft,
  ChevronRight,
  Columns3,
  Copy,
  Filter,
  Plus,
  RefreshCcw,
  Search,
  Trash2,
  Wrench,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { useMongoBrowser } from "../../hooks/useMongoBrowser";
import s from "../../styles";
import type { AeroricDbConnectionConfig } from "../../types";
import { DbxButton, DbxMenuItem } from "./DbxButton";
import { confirmDbxProductionOperation, hasProductionProtection } from "./databaseProductionSafety";

interface Props {
  connectionId: string;
  connection?: AeroricDbConnectionConfig | null;
  readOnly: boolean;
  initialDatabase?: string;
  initialCollection?: string;
  initialDocumentId?: string;
  onDocumentsQueryApplied?: (
    database: string,
    collection: string,
    filter: string,
    sort: string,
    projection: string,
  ) => void;
}

const MONGO_WORKSPACE_DEFAULT_PAGE_SIZE = 100;
const MONGO_WORKSPACE_PAGE_SIZE_OPTIONS = [50, 100, 200, 500, 1000] as const;
const EMPTY_DOCUMENT_DRAFT = "{\n  \n}";
const MONGO_VIEW_MODE_STORAGE_KEY = "dbx-mongo-view-mode";
type MongoViewMode = "document" | "table";
type MongoFilterMode =
  | "equals"
  | "not-equals"
  | "like"
  | "not-like"
  | "greater-than"
  | "less-than"
  | "is-null"
  | "is-not-null";
type MongoFilterRule = {
  id: string;
  fieldName: string;
  mode: MongoFilterMode;
  rawValue: string;
  conjunction: "AND" | "OR";
};

const MONGO_FILTER_MODE_OPTIONS: Array<{ value: MongoFilterMode; labelKey: string }> = [
  { value: "equals", labelKey: "database.filterBuilderEquals" },
  { value: "not-equals", labelKey: "database.filterBuilderNotEquals" },
  { value: "like", labelKey: "database.filterBuilderContains" },
  { value: "not-like", labelKey: "database.filterBuilderNotContains" },
  { value: "greater-than", labelKey: "database.filterBuilderGreaterThan" },
  { value: "less-than", labelKey: "database.filterBuilderLessThan" },
  { value: "is-null", labelKey: "database.filterBuilderIsNull" },
  { value: "is-not-null", labelKey: "database.filterBuilderIsNotNull" },
];

let mongoFilterRuleSequence = 0;

function nextMongoFilterRuleId() {
  mongoFilterRuleSequence += 1;
  return `mongo-filter-${mongoFilterRuleSequence}`;
}

function loadMongoViewMode(): MongoViewMode {
  try {
    if (typeof window === "undefined") return "document";
    const value = window.localStorage.getItem(MONGO_VIEW_MODE_STORAGE_KEY);
    return value === "table" || value === "document" ? value : "document";
  } catch {
    return "document";
  }
}

function saveMongoViewMode(value: MongoViewMode) {
  try {
    if (typeof window !== "undefined")
      window.localStorage.setItem(MONGO_VIEW_MODE_STORAGE_KEY, value);
  } catch {
    // Ignore storage failures; the view mode still changes for the current session.
  }
}

function mongoDocumentId(document: unknown) {
  if (document && typeof document === "object" && "_id" in document)
    return String((document as { _id: unknown })._id);
  return "";
}

function mongoDocumentRawId(document: unknown): unknown | null {
  if (!document || typeof document !== "object" || !("_id" in document)) return null;
  return (document as { _id: unknown })._id;
}

function mongoDocumentColumns(documents: unknown[]): string[] {
  const columns = new Set<string>();
  for (const document of documents) {
    if (!document || typeof document !== "object" || Array.isArray(document)) continue;
    for (const key of Object.keys(document)) columns.add(key);
  }
  return Array.from(columns).sort((a, b) => {
    if (a === "_id") return -1;
    if (b === "_id") return 1;
    return a.localeCompare(b);
  });
}

function mongoCellValue(document: unknown, column: string): unknown {
  if (!document || typeof document !== "object" || Array.isArray(document)) return undefined;
  return (document as Record<string, unknown>)[column];
}

function mongoCellText(value: unknown): string {
  if (value == null) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value);
}

function mongoSortDirection(sortJson: string, column: string): "asc" | "desc" | null {
  try {
    const parsed = JSON.parse(sortJson);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    const value = (parsed as Record<string, unknown>)[column];
    if (value === 1 || value === "1" || value === "asc") return "asc";
    if (value === -1 || value === "-1" || value === "desc") return "desc";
  } catch {
    return null;
  }
  return null;
}

function createMongoFilterRule(fieldName = ""): MongoFilterRule {
  return {
    id: nextMongoFilterRuleId(),
    fieldName,
    mode: "equals",
    rawValue: "",
    conjunction: "AND",
  };
}

function mongoFilterModeNeedsValue(mode: MongoFilterMode) {
  return mode !== "is-null" && mode !== "is-not-null";
}

function parseMongoFilterValue(raw: string): unknown {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  try {
    return JSON.parse(trimmed);
  } catch {
    return trimmed;
  }
}

function parseMongoFilterInput(input: string): Record<string, unknown> | null {
  const trimmed = input.trim();
  if (!trimmed || trimmed === "{}") return {};
  try {
    const parsed = JSON.parse(trimmed);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function buildMongoFilterCondition(rule: MongoFilterRule): Record<string, unknown> | null {
  if (!rule.fieldName) return null;
  if (mongoFilterModeNeedsValue(rule.mode) && !rule.rawValue.trim()) return null;
  const value = mongoFilterModeNeedsValue(rule.mode) ? parseMongoFilterValue(rule.rawValue) : null;
  switch (rule.mode) {
    case "equals":
      return { [rule.fieldName]: value };
    case "not-equals":
      return { [rule.fieldName]: { $ne: value } };
    case "like":
      return { [rule.fieldName]: { $regex: String(value), $options: "i" } };
    case "not-like":
      return { [rule.fieldName]: { $not: { $regex: String(value), $options: "i" } } };
    case "greater-than":
      return { [rule.fieldName]: { $gt: value } };
    case "less-than":
      return { [rule.fieldName]: { $lt: value } };
    case "is-null":
      return { [rule.fieldName]: null };
    case "is-not-null":
      return { [rule.fieldName]: { $ne: null } };
  }
}

function combineMongoFilterConditions(
  conditions: Record<string, unknown>[],
  rules: Pick<MongoFilterRule, "conjunction">[],
): Record<string, unknown> | null {
  if (conditions.length === 0) return null;
  let result = conditions[0];
  for (let index = 1; index < conditions.length; index += 1) {
    const operator = rules[index]?.conjunction === "OR" ? "$or" : "$and";
    result = { [operator]: [result, conditions[index]] };
  }
  return result;
}

function mongoFilterJson(
  manualFilter: string,
  structuredFilter: Record<string, unknown> | null,
): string {
  const manual = parseMongoFilterInput(manualFilter);
  if (!structuredFilter)
    return manual
      ? Object.keys(manual).length
        ? JSON.stringify(manual)
        : "{}"
      : manualFilter.trim() || "{}";
  if (!manual) return JSON.stringify(structuredFilter);
  const combined = Object.keys(manual).length
    ? { $and: [manual, structuredFilter] }
    : structuredFilter;
  return Object.keys(combined).length ? JSON.stringify(combined) : "{}";
}

export function MongoBrowser({
  connectionId,
  connection,
  readOnly,
  initialDatabase,
  initialCollection,
  initialDocumentId,
  onDocumentsQueryApplied,
}: Props) {
  const { t } = useI18n();
  const mongo = useMongoBrowser(connectionId);
  const productionProtected = Boolean(connection && hasProductionProtection(connection));
  const [activeDatabase, setActiveDatabase] = useState("");
  const [activeCollection, setActiveCollection] = useState("");
  const [filter, setFilter] = useState("{}");
  const [structuredFilterOpen, setStructuredFilterOpen] = useState(false);
  const [filterRules, setFilterRules] = useState<MongoFilterRule[]>([]);
  const [appliedStructuredFilter, setAppliedStructuredFilter] = useState<Record<
    string,
    unknown
  > | null>(null);
  const [sort, setSort] = useState("{}");
  const [projection, setProjection] = useState("{}");
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState<number>(MONGO_WORKSPACE_DEFAULT_PAGE_SIZE);
  const [viewMode, setViewMode] = useState<MongoViewMode>(loadMongoViewMode);
  const [documentDraft, setDocumentDraft] = useState(EMPTY_DOCUMENT_DRAFT);
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [collectionContextMenu, setCollectionContextMenu] = useState<{
    x: number;
    y: number;
    collection: string;
  } | null>(null);
  const [documentContextMenu, setDocumentContextMenu] = useState<{
    x: number;
    y: number;
    index: number;
    document: unknown;
  } | null>(null);
  const [columnVisibilityOpen, setColumnVisibilityOpen] = useState(false);
  const [columnVisibilityQuery, setColumnVisibilityQuery] = useState("");
  const [tableOptionsOpen, setTableOptionsOpen] = useState(false);
  const [hiddenTableColumns, setHiddenTableColumns] = useState<Set<string>>(new Set());
  const [hideNullTableColumns, setHideNullTableColumns] = useState(false);
  const tableColumns = useMemo(() => mongoDocumentColumns(mongo.documents), [mongo.documents]);
  const effectiveFilter = useMemo(
    () => mongoFilterJson(filter, appliedStructuredFilter),
    [appliedStructuredFilter, filter],
  );
  const filteredTableColumns = useMemo(() => {
    const query = columnVisibilityQuery.trim().toLowerCase();
    return tableColumns.filter((column) => !query || column.toLowerCase().includes(query));
  }, [columnVisibilityQuery, tableColumns]);
  const allNullTableColumns = useMemo(
    () =>
      new Set(
        tableColumns.filter(
          (column) =>
            mongo.documents.length > 0 &&
            mongo.documents.every((document) => mongoCellValue(document, column) == null),
        ),
      ),
    [mongo.documents, tableColumns],
  );
  const manuallyVisibleTableColumns = useMemo(
    () => tableColumns.filter((column) => !hiddenTableColumns.has(column)),
    [hiddenTableColumns, tableColumns],
  );
  const manuallyVisibleNullTableColumns = useMemo(
    () => manuallyVisibleTableColumns.filter((column) => allNullTableColumns.has(column)),
    [allNullTableColumns, manuallyVisibleTableColumns],
  );
  const canHideNullTableColumns =
    manuallyVisibleNullTableColumns.length > 0 &&
    manuallyVisibleTableColumns.length > manuallyVisibleNullTableColumns.length;
  const effectiveHideNullTableColumns = hideNullTableColumns && canHideNullTableColumns;
  const visibleTableColumns = useMemo(
    () =>
      manuallyVisibleTableColumns.filter(
        (column) => !(effectiveHideNullTableColumns && allNullTableColumns.has(column)),
      ),
    [allNullTableColumns, effectiveHideNullTableColumns, manuallyVisibleTableColumns],
  );

  useEffect(() => {
    setActiveDatabase("");
    setActiveCollection("");
    setFilter("{}");
    setStructuredFilterOpen(false);
    setFilterRules([]);
    setAppliedStructuredFilter(null);
    setSort("{}");
    setProjection("{}");
    setPage(0);
    setSelectedIndex(null);
    setDocumentDraft(EMPTY_DOCUMENT_DRAFT);
    setCollectionContextMenu(null);
    setDocumentContextMenu(null);
    setColumnVisibilityOpen(false);
    setColumnVisibilityQuery("");
    setTableOptionsOpen(false);
    mongo.resetBrowserState();
    mongo
      .loadDatabases()
      .then(async () => {
        if (!initialDatabase) return;
        setActiveDatabase(initialDatabase);
        await mongo.loadCollections(initialDatabase);
        if (!initialCollection) return;
        setActiveCollection(initialCollection);
        clearSelectedDocument();
        const result = await mongo.findDocuments({
          database: initialDatabase,
          collection: initialCollection,
          filter: "{}",
          projection: "{}",
          sort: "{}",
          skip: 0,
          limit: MONGO_WORKSPACE_DEFAULT_PAGE_SIZE,
        });
        if (!initialDocumentId) return;
        const index = result.documents.findIndex(
          (document) => mongoDocumentId(document) === initialDocumentId,
        );
        if (index >= 0) selectDocument(index, result.documents[index]);
      })
      .catch(() => undefined);
    // sync initial selection from the sidebar without re-running for hook identity changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId, initialDatabase, initialCollection, initialDocumentId]);

  useEffect(() => {
    setHiddenTableColumns((current) => {
      const available = new Set(tableColumns);
      const next = new Set([...current].filter((column) => available.has(column)));
      if (tableColumns.length > 0 && next.size >= tableColumns.length) {
        next.delete(tableColumns[0]);
      }
      if (next.size !== current.size) return next;
      for (const column of next) {
        if (!current.has(column)) return next;
      }
      return current;
    });
  }, [tableColumns]);

  useEffect(() => {
    setFilterRules((current) => {
      if (current.length === 0 || tableColumns.length === 0) return current;
      let changed = false;
      const next = current.map((rule) => {
        if (tableColumns.includes(rule.fieldName)) return rule;
        changed = true;
        return { ...rule, fieldName: tableColumns[0] ?? "" };
      });
      return changed ? next : current;
    });
  }, [tableColumns]);

  const loadDocuments = (
    database = activeDatabase,
    collection = activeCollection,
    nextFilter = filter,
    nextSort = sort,
    nextPageSize = pageSize,
    nextStructuredFilter = appliedStructuredFilter,
    nextProjection = projection,
  ) => {
    if (!database || !collection) return;
    const queryFilter = mongoFilterJson(nextFilter, nextStructuredFilter);
    setPage(0);
    clearSelectedDocument();
    mongo.clearDocuments();
    void mongo.findDocuments({
      database,
      collection,
      filter: queryFilter,
      projection: nextProjection,
      sort: nextSort,
      skip: 0,
      limit: nextPageSize,
    });
    onDocumentsQueryApplied?.(database, collection, queryFilter, nextSort, nextProjection);
  };
  const clearDocumentFilters = () => {
    const nextFilter = "{}";
    const nextSort = "{}";
    const nextProjection = "{}";
    setFilter(nextFilter);
    setStructuredFilterOpen(false);
    setAppliedStructuredFilter(null);
    setFilterRules(tableColumns.length > 0 ? [createMongoFilterRule(tableColumns[0])] : []);
    setSort(nextSort);
    setProjection(nextProjection);
    loadDocuments(
      activeDatabase,
      activeCollection,
      nextFilter,
      nextSort,
      pageSize,
      null,
      nextProjection,
    );
  };
  const loadMoreDocuments = () => {
    const loadedThroughCurrentPage = page * pageSize + mongo.documents.length;
    if (!activeDatabase || !activeCollection || loadedThroughCurrentPage >= mongo.total) return;
    void mongo.findDocuments({
      database: activeDatabase,
      collection: activeCollection,
      filter: effectiveFilter,
      projection,
      sort,
      skip: loadedThroughCurrentPage,
      limit: pageSize,
      append: true,
    });
  };
  const loadDocumentPage = (nextPage: number) => {
    if (!activeDatabase || !activeCollection || nextPage < 0) return;
    setPage(nextPage);
    clearSelectedDocument();
    mongo.clearDocuments();
    void mongo.findDocuments({
      database: activeDatabase,
      collection: activeCollection,
      filter: effectiveFilter,
      projection,
      sort,
      skip: nextPage * pageSize,
      limit: pageSize,
    });
  };
  const changePageSize = (nextPageSize: number) => {
    setPageSize(nextPageSize);
    loadDocuments(
      activeDatabase,
      activeCollection,
      filter,
      sort,
      nextPageSize,
      appliedStructuredFilter,
    );
  };
  const sortByColumn = (column: string) => {
    const currentDirection = mongoSortDirection(sort, column);
    const nextSort =
      currentDirection === "asc"
        ? JSON.stringify({ [column]: -1 })
        : currentDirection === "desc"
          ? "{}"
          : JSON.stringify({ [column]: 1 });
    setSort(nextSort);
    loadDocuments(
      activeDatabase,
      activeCollection,
      filter,
      nextSort,
      pageSize,
      appliedStructuredFilter,
    );
  };
  const toggleTableColumn = (column: string) => {
    setHiddenTableColumns((current) => {
      const next = new Set(current);
      if (next.has(column)) {
        next.delete(column);
      } else {
        if (visibleTableColumns.includes(column) && visibleTableColumns.length <= 1) return current;
        next.add(column);
        const nextManualColumns = tableColumns.filter((candidate) => !next.has(candidate));
        const nextNullColumns = nextManualColumns.filter((candidate) =>
          allNullTableColumns.has(candidate),
        );
        const nextHideNullApplies =
          hideNullTableColumns && nextManualColumns.length > nextNullColumns.length;
        const nextVisibleCount = nextManualColumns.filter(
          (candidate) => !(nextHideNullApplies && allNullTableColumns.has(candidate)),
        ).length;
        if (nextVisibleCount < 1) return current;
      }
      return next;
    });
  };
  const showAllTableColumns = () => setHiddenTableColumns(new Set());
  const invertTableColumnVisibility = () => {
    const next = new Set(visibleTableColumns);
    if (next.size === tableColumns.length && tableColumns.length > 0) {
      next.delete(tableColumns[0]);
    }
    const nextManualColumns = tableColumns.filter((column) => !next.has(column));
    const nextNullColumns = nextManualColumns.filter((column) => allNullTableColumns.has(column));
    const nextHideNullApplies =
      hideNullTableColumns && nextManualColumns.length > nextNullColumns.length;
    const nextVisibleColumns = nextManualColumns.filter(
      (column) => !(nextHideNullApplies && allNullTableColumns.has(column)),
    );
    if (nextVisibleColumns.length === 0 && hideNullTableColumns) {
      setHideNullTableColumns(false);
    }
    setHiddenTableColumns(next);
  };
  const toggleHideNullTableColumns = () => {
    setHideNullTableColumns((current) => {
      if (current) return false;
      return canHideNullTableColumns ? true : current;
    });
  };
  const ensureFilterRule = () => {
    if (filterRules.length === 0) {
      setFilterRules([createMongoFilterRule(tableColumns[0] ?? "")]);
    }
  };
  const addFilterRule = () => {
    setFilterRules((current) => [...current, createMongoFilterRule(tableColumns[0] ?? "")]);
  };
  const updateFilterRule = (id: string, patch: Partial<MongoFilterRule>) => {
    setFilterRules((current) =>
      current.map((rule) => {
        if (rule.id !== id) return rule;
        const next = { ...rule, ...patch };
        if (patch.mode && !mongoFilterModeNeedsValue(patch.mode)) next.rawValue = "";
        return next;
      }),
    );
  };
  const removeFilterRule = (id: string) => {
    setFilterRules((current) =>
      current.length <= 1 ? current : current.filter((rule) => rule.id !== id),
    );
  };
  const resetStructuredFilterBuilder = () => {
    setAppliedStructuredFilter(null);
    setFilterRules(tableColumns.length > 0 ? [createMongoFilterRule(tableColumns[0])] : []);
  };
  const applyStructuredFilters = () => {
    const items = filterRules
      .map((rule) => ({ rule, condition: buildMongoFilterCondition(rule) }))
      .filter(
        (item): item is { rule: MongoFilterRule; condition: Record<string, unknown> } =>
          item.condition != null,
      );
    const structured = combineMongoFilterConditions(
      items.map((item) => item.condition),
      items.map((item) => item.rule),
    );
    setAppliedStructuredFilter(structured);
    setStructuredFilterOpen(false);
    loadDocuments(activeDatabase, activeCollection, filter, sort, pageSize, structured);
  };
  const copyCollectionName = (collection: string) => {
    navigator.clipboard?.writeText(collection).catch(() => undefined);
  };
  const copyDocumentJson = (document: unknown) => {
    navigator.clipboard?.writeText(JSON.stringify(document, null, 2)).catch(() => undefined);
  };
  const selectCollection = (collection: string) => {
    setColumnVisibilityOpen(false);
    setColumnVisibilityQuery("");
    setTableOptionsOpen(false);
    setStructuredFilterOpen(false);
    setActiveCollection(collection);
    loadDocuments(activeDatabase, collection);
  };
  const refreshDocumentsKeepingSelection = async (
    id: string,
    database = activeDatabase,
    collection = activeCollection,
    pageIndex = page,
  ) => {
    if (!database || !collection) return;
    const queryFilter = mongoFilterJson(filter, appliedStructuredFilter);
    const result = await mongo.findDocuments({
      database,
      collection,
      filter: queryFilter,
      projection,
      sort,
      skip: pageIndex * pageSize,
      limit: pageSize,
    });
    const nextIndex = result.documents.findIndex((document) => mongoDocumentId(document) === id);
    if (nextIndex >= 0) {
      selectDocument(nextIndex, result.documents[nextIndex]);
    } else {
      clearSelectedDocument();
    }
    onDocumentsQueryApplied?.(database, collection, queryFilter, sort, projection);
  };
  const insertDocument = async (collection = activeCollection) => {
    if (!activeDatabase || !collection) return;
    if (
      productionProtected &&
      connection &&
      !(await confirmDbxProductionOperation({
        connection,
        database: activeDatabase,
        operation: t("database.productionMongoInsertOperation", { collection }),
        okLabel: t("database.mongoInsertDocument"),
        t,
      }))
    )
      return;
    setActiveCollection(collection);
    const insertedId = await mongo.insertDocument({
      database: activeDatabase,
      collection,
      docJson: documentDraft,
    });
    setPage(0);
    await refreshDocumentsKeepingSelection(insertedId, activeDatabase, collection, 0);
  };
  const deleteMatchingDocuments = async (collection = activeCollection) => {
    if (!activeDatabase || !collection) return;
    const queryFilter = mongoFilterJson(filter, appliedStructuredFilter);
    const operation = t("database.confirmDeleteMongoDocuments", {
      collection,
      filter: queryFilter,
    });
    const ok =
      productionProtected && connection
        ? await confirmDbxProductionOperation({
            connection,
            database: activeDatabase,
            operation,
            okLabel: t("database.mongoDeleteMatchingDocuments"),
            t,
          })
        : await confirm(operation, {
            title: t("database.mongoDeleteMatchingDocuments"),
            kind: "warning",
            okLabel: t("database.mongoDeleteMatchingDocuments"),
            cancelLabel: t("common.cancel"),
          });
    if (!ok) return;
    setActiveCollection(collection);
    await mongo.deleteDocuments({
      database: activeDatabase,
      collection,
      filterJson: queryFilter,
      many: true,
    });
    loadDocuments(activeDatabase, collection, filter, sort, pageSize, appliedStructuredFilter);
  };
  const deleteDocument = async (document: unknown) => {
    if (!activeDatabase || !activeCollection) return;
    const rawId = mongoDocumentRawId(document);
    const id = mongoDocumentId(document);
    if (rawId == null || !id) return;
    const operation = t("database.confirmDeleteMongoDocument", {
      collection: activeCollection,
      id,
    });
    const ok =
      productionProtected && connection
        ? await confirmDbxProductionOperation({
            connection,
            database: activeDatabase,
            operation,
            okLabel: t("database.mongoDeleteDocument"),
            t,
          })
        : await confirm(operation, {
            title: t("database.mongoDeleteDocument"),
            kind: "warning",
            okLabel: t("database.mongoDeleteDocument"),
            cancelLabel: t("common.cancel"),
          });
    if (!ok) return;
    await mongo.deleteDocuments({
      database: activeDatabase,
      collection: activeCollection,
      filterJson: JSON.stringify({ _id: rawId }),
      many: false,
    });
    clearSelectedDocument();
    loadDocuments();
  };
  const openCollectionContextMenu = (event: MouseEvent, collection: string) => {
    event.preventDefault();
    setCollectionContextMenu({
      x: event.clientX,
      y: event.clientY,
      collection,
    });
  };
  const openDocumentContextMenu = (event: MouseEvent, index: number, document: unknown) => {
    event.preventDefault();
    setDocumentContextMenu({
      x: event.clientX,
      y: event.clientY,
      index,
      document,
    });
  };

  const selectDocument = (index: number, document: unknown) => {
    setSelectedIndex(index);
    setDocumentDraft(JSON.stringify(document, null, 2));
  };

  const clearSelectedDocument = () => {
    setSelectedIndex(null);
    setDocumentDraft(EMPTY_DOCUMENT_DRAFT);
  };

  const saveDocument = async () => {
    if (selectedIndex == null || !activeDatabase || !activeCollection) return;
    const selected = mongo.documents[selectedIndex];
    const id = mongoDocumentId(selected);
    if (!id) return;
    if (
      productionProtected &&
      connection &&
      !(await confirmDbxProductionOperation({
        connection,
        database: activeDatabase,
        operation: t("database.productionMongoUpdateOperation", {
          collection: activeCollection,
          id,
        }),
        okLabel: t("database.saveDocument"),
        t,
      }))
    )
      return;
    await mongo.updateDocument({
      database: activeDatabase,
      collection: activeCollection,
      id,
      docJson: documentDraft,
    });
    await refreshDocumentsKeepingSelection(id);
  };

  return (
    <div style={s.databaseBrowserRoot}>
      <div style={s.databaseBrowserSidebar}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>MongoDB</div>
            <div style={s.databaseDialogHint}>{t("database.mongoBrowserHint")}</div>
          </div>
          <DbxButton
            variant="ghost"
            size="icon-sm"
            icon={RefreshCcw}
            onClick={() => loadDocuments()}
            aria-label={t("database.refresh")}
          />
        </div>
        <div style={s.databaseSection}>
          <div style={s.databaseSectionTitle}>{t("database.databases")}</div>
          {mongo.databases.map((database) => (
            <button
              key={database}
              type="button"
              style={{
                ...s.databaseListButton,
                ...(activeDatabase === database ? s.databaseListButtonActive : {}),
              }}
              onClick={() => {
                setActiveDatabase(database);
                setActiveCollection("");
                setColumnVisibilityOpen(false);
                setColumnVisibilityQuery("");
                setTableOptionsOpen(false);
                setStructuredFilterOpen(false);
                setPage(0);
                clearSelectedDocument();
                mongo.clearDocuments();
                mongo.clearCollections();
                void mongo.loadCollections(database);
              }}
            >
              {database}
            </button>
          ))}
        </div>
        <div style={s.databaseSection}>
          <div style={s.databaseSectionTitle}>{t("database.collections")}</div>
          {mongo.collections.map((collection) => (
            <button
              key={collection}
              type="button"
              style={{
                ...s.databaseListButton,
                ...(activeCollection === collection ? s.databaseListButtonActive : {}),
              }}
              onClick={() => {
                selectCollection(collection);
              }}
              onContextMenu={(event) => openCollectionContextMenu(event, collection)}
            >
              {collection}
            </button>
          ))}
        </div>
      </div>
      <div style={s.databaseBrowserMain}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>
              {activeCollection || t("database.mongoCollection")}
            </div>
            <div style={s.databaseDialogHint}>
              {t("database.mongoTotal", { total: mongo.total })}
            </div>
          </div>
          <div style={s.databaseButtonRow}>
            <DbxButton
              variant={viewMode === "document" ? "default" : "outline"}
              size="sm"
              onClick={() => {
                setViewMode("document");
                setColumnVisibilityOpen(false);
                setColumnVisibilityQuery("");
                setTableOptionsOpen(false);
                saveMongoViewMode("document");
              }}
            >
              {t("database.documentMode")}
            </DbxButton>
            <DbxButton
              variant={viewMode === "table" ? "default" : "outline"}
              size="sm"
              onClick={() => {
                setViewMode("table");
                saveMongoViewMode("table");
              }}
            >
              {t("database.tableMode")}
            </DbxButton>
            {viewMode === "table" && tableColumns.length > 0 && (
              <span style={{ position: "relative", display: "inline-flex" }}>
                <DbxButton
                  variant={hiddenTableColumns.size > 0 ? "default" : "outline"}
                  size="sm"
                  icon={Columns3}
                  onClick={() => {
                    setColumnVisibilityOpen((open) => !open);
                    setTableOptionsOpen(false);
                  }}
                  aria-expanded={columnVisibilityOpen}
                  aria-haspopup="menu"
                >
                  {t("database.gridColumnVisibility")}
                  {hiddenTableColumns.size > 0 && (
                    <span style={s.databasePill}>
                      {visibleTableColumns.length}/{tableColumns.length}
                    </span>
                  )}
                </DbxButton>
                {columnVisibilityOpen && (
                  <div
                    role="menu"
                    style={{
                      ...s.fileCtxMenu,
                      position: "absolute",
                      right: 0,
                      top: "calc(100% + 6px)",
                      minWidth: 260,
                      maxHeight: 340,
                      overflow: "hidden",
                      zIndex: 20,
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 8,
                        padding: "7px 9px",
                        borderBottom: "1px solid var(--border-dim)",
                      }}
                    >
                      <span style={{ fontSize: 12, fontWeight: 700 }}>
                        {t("database.gridColumnVisibility")}
                      </span>
                      <span
                        style={{
                          fontSize: 11,
                          color: "var(--text-muted)",
                          fontVariantNumeric: "tabular-nums",
                        }}
                      >
                        {visibleTableColumns.length}/{tableColumns.length}
                      </span>
                    </div>
                    <label
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 6,
                        padding: "6px 9px",
                        borderBottom: "1px solid var(--border-dim)",
                        color: "var(--text-muted)",
                      }}
                    >
                      <Search size={13} />
                      <input
                        style={{
                          minWidth: 0,
                          flex: 1,
                          height: 24,
                          border: "none",
                          outline: "none",
                          background: "transparent",
                          color: "var(--text-primary)",
                          fontSize: 12,
                        }}
                        value={columnVisibilityQuery}
                        onChange={(event) => setColumnVisibilityQuery(event.target.value)}
                        placeholder={t("database.gridSearchColumns")}
                        aria-label={t("database.gridSearchColumns")}
                      />
                    </label>
                    <div style={{ maxHeight: 210, overflow: "auto", padding: "2px 0" }}>
                      {filteredTableColumns.map((column) => {
                        const visible = !hiddenTableColumns.has(column);
                        const effectivelyVisible = visibleTableColumns.includes(column);
                        const disableHide =
                          visible && effectivelyVisible && visibleTableColumns.length <= 1;
                        return (
                          <label
                            key={column}
                            style={{
                              ...s.fileCtxMenuItem,
                              display: "grid",
                              gridTemplateColumns: "18px minmax(0, 1fr)",
                              alignItems: "center",
                              gap: 8,
                              opacity: disableHide ? 0.65 : 1,
                            }}
                          >
                            <input
                              type="checkbox"
                              checked={visible}
                              disabled={disableHide}
                              onChange={() => toggleTableColumn(column)}
                              aria-label={column}
                            />
                            <span
                              style={{
                                minWidth: 0,
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                                fontFamily: "var(--font-mono)",
                              }}
                            >
                              {column}
                            </span>
                          </label>
                        );
                      })}
                      {filteredTableColumns.length === 0 && (
                        <div
                          style={{
                            padding: "18px 10px",
                            textAlign: "center",
                            fontSize: 12,
                            color: "var(--text-muted)",
                          }}
                        >
                          {t("database.gridNoSearchResults")}
                        </div>
                      )}
                    </div>
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 8,
                        padding: "5px 6px",
                        borderTop: "1px solid var(--border-dim)",
                        background: "var(--bg-muted)",
                      }}
                    >
                      <span style={{ minWidth: 0, color: "var(--text-muted)", fontSize: 11 }}>
                        {t("database.gridColumnVisibilityHint")}
                      </span>
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                        <button
                          type="button"
                          role="menuitem"
                          disabled={tableColumns.length <= 1}
                          style={{
                            ...s.fileCtxMenuItem,
                            width: "auto",
                            margin: 0,
                            padding: "0 8px",
                            opacity: tableColumns.length <= 1 ? 0.55 : 1,
                          }}
                          onClick={invertTableColumnVisibility}
                        >
                          {t("database.gridInvertColumnVisibility")}
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          disabled={hiddenTableColumns.size === 0}
                          style={{
                            ...s.fileCtxMenuItem,
                            width: "auto",
                            margin: 0,
                            padding: "0 8px",
                            display: "inline-flex",
                            alignItems: "center",
                            gap: 5,
                            opacity: hiddenTableColumns.size === 0 ? 0.55 : 1,
                          }}
                          onClick={showAllTableColumns}
                        >
                          <Check size={13} />
                          <span>{t("database.gridShowAllColumns")}</span>
                        </button>
                      </span>
                    </div>
                  </div>
                )}
              </span>
            )}
            {viewMode === "table" && tableColumns.length > 0 && (
              <span style={{ position: "relative", display: "inline-flex" }}>
                <DbxButton
                  variant={effectiveHideNullTableColumns ? "default" : "ghost"}
                  size="icon-sm"
                  icon={Wrench}
                  onClick={() => {
                    setTableOptionsOpen((open) => !open);
                    setColumnVisibilityOpen(false);
                  }}
                  aria-label={t("database.gridViewOptions")}
                  aria-expanded={tableOptionsOpen}
                  aria-haspopup="menu"
                  title={t("database.gridViewOptions")}
                />
                {tableOptionsOpen && (
                  <div
                    role="menu"
                    style={{
                      ...s.fileCtxMenu,
                      position: "absolute",
                      right: 0,
                      top: "calc(100% + 6px)",
                      minWidth: 190,
                      zIndex: 20,
                    }}
                  >
                    <label
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "grid",
                        gridTemplateColumns: "18px minmax(0, 1fr)",
                        alignItems: "center",
                        gap: 8,
                        opacity: !hideNullTableColumns && !canHideNullTableColumns ? 0.65 : 1,
                      }}
                    >
                      <input
                        type="checkbox"
                        checked={hideNullTableColumns}
                        disabled={!hideNullTableColumns && !canHideNullTableColumns}
                        onChange={toggleHideNullTableColumns}
                      />
                      <span
                        style={{
                          minWidth: 0,
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 4,
                        }}
                      >
                        <span>{t("database.gridHideNullColumns")}</span>
                        {manuallyVisibleNullTableColumns.length > 0 && (
                          <span
                            style={{
                              color: "var(--text-muted)",
                              fontVariantNumeric: "tabular-nums",
                            }}
                          >
                            ({manuallyVisibleNullTableColumns.length})
                          </span>
                        )}
                      </span>
                    </label>
                  </div>
                )}
              </span>
            )}
          </div>
        </div>
        <div style={s.databaseDialogFormGrid}>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.filterJson")}</span>
            <input
              style={s.databaseDialogInput}
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") loadDocuments();
              }}
            />
          </label>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.sortJson")}</span>
            <input
              style={s.databaseDialogInput}
              value={sort}
              onChange={(event) => setSort(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") loadDocuments();
              }}
            />
          </label>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.projectionJson")}</span>
            <input
              style={s.databaseDialogInput}
              value={projection}
              onChange={(event) => setProjection(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") loadDocuments();
              }}
            />
          </label>
        </div>
        <div style={s.databaseToolbar}>
          <span style={{ position: "relative", display: "inline-flex" }}>
            <DbxButton
              variant={appliedStructuredFilter ? "default" : "outline"}
              size="sm"
              icon={Filter}
              disabled={!activeDatabase || !activeCollection}
              onClick={() => {
                ensureFilterRule();
                setStructuredFilterOpen((open) => !open);
              }}
              aria-expanded={structuredFilterOpen}
              aria-haspopup="dialog"
            >
              {t("database.gridFilter")}
              {appliedStructuredFilter && <span style={s.databasePill}>1</span>}
            </DbxButton>
            {structuredFilterOpen && (
              <div
                role="dialog"
                aria-label={t("database.gridFilter")}
                style={{
                  ...s.fileCtxMenu,
                  position: "absolute",
                  left: 0,
                  top: "calc(100% + 6px)",
                  width: 380,
                  maxWidth: "calc(100vw - 32px)",
                  padding: 10,
                  zIndex: 20,
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    marginBottom: 8,
                  }}
                >
                  <span style={{ fontSize: 12, fontWeight: 700 }}>{t("database.gridFilter")}</span>
                  <DbxButton variant="outline" size="sm" icon={Plus} onClick={addFilterRule}>
                    {t("database.filterBuilderAddRule")}
                  </DbxButton>
                </div>
                {filterRules.length > 0 ? (
                  <div style={{ display: "grid", gap: 7 }}>
                    {filterRules.map((rule, index) => (
                      <div key={rule.id} style={{ display: "grid", gap: 5 }}>
                        {index > 0 && (
                          <DbxButton
                            variant="outline"
                            size="xs"
                            onClick={() =>
                              updateFilterRule(rule.id, {
                                conjunction: rule.conjunction === "AND" ? "OR" : "AND",
                              })
                            }
                            style={{ width: 54, justifySelf: "center" }}
                          >
                            {rule.conjunction}
                          </DbxButton>
                        )}
                        <div
                          style={{
                            display: "grid",
                            gridTemplateColumns:
                              "minmax(0, 1fr) minmax(0, 1fr) minmax(0, 1fr) 30px",
                            gap: 6,
                            alignItems: "center",
                          }}
                        >
                          <select
                            style={{ ...s.databaseDialogInput, height: 30, padding: "0 6px" }}
                            value={rule.fieldName}
                            aria-label={t("database.filterBuilderColumn")}
                            onChange={(event) =>
                              updateFilterRule(rule.id, { fieldName: event.currentTarget.value })
                            }
                          >
                            {tableColumns.length === 0 && (
                              <option value="">{t("database.filterBuilderColumn")}</option>
                            )}
                            {tableColumns.map((column) => (
                              <option key={column} value={column}>
                                {column}
                              </option>
                            ))}
                          </select>
                          <select
                            style={{ ...s.databaseDialogInput, height: 30, padding: "0 6px" }}
                            value={rule.mode}
                            aria-label={t("database.filterBuilderMode")}
                            onChange={(event) =>
                              updateFilterRule(rule.id, {
                                mode: event.currentTarget.value as MongoFilterMode,
                              })
                            }
                          >
                            {MONGO_FILTER_MODE_OPTIONS.map((option) => (
                              <option key={option.value} value={option.value}>
                                {t(option.labelKey)}
                              </option>
                            ))}
                          </select>
                          {mongoFilterModeNeedsValue(rule.mode) ? (
                            <input
                              style={{ ...s.databaseDialogInput, height: 30, padding: "0 6px" }}
                              value={rule.rawValue}
                              placeholder={t("database.filterBuilderValue")}
                              aria-label={t("database.filterBuilderValue")}
                              onChange={(event) =>
                                updateFilterRule(rule.id, { rawValue: event.target.value })
                              }
                              onKeyDown={(event) => {
                                if (event.key === "Enter") applyStructuredFilters();
                              }}
                            />
                          ) : (
                            <span
                              style={{
                                ...s.databaseDialogInput,
                                height: 30,
                                display: "inline-flex",
                                alignItems: "center",
                                color: "var(--text-muted)",
                              }}
                            >
                              {t("database.filterBuilderNoValue")}
                            </span>
                          )}
                          <DbxButton
                            variant="destructive"
                            size="icon-xs"
                            icon={Trash2}
                            disabled={filterRules.length <= 1}
                            aria-label={t("database.filterBuilderRemoveRule")}
                            onClick={() => removeFilterRule(rule.id)}
                            style={{ opacity: filterRules.length <= 1 ? 0.45 : 1 }}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div
                    style={{
                      border: "1px dashed var(--border-medium)",
                      borderRadius: 7,
                      padding: 14,
                      textAlign: "center",
                      color: "var(--text-muted)",
                      fontSize: 12,
                    }}
                  >
                    {t("database.filterBuilderEmpty")}
                  </div>
                )}
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    gap: 8,
                    marginTop: 10,
                  }}
                >
                  <DbxButton variant="outline" size="sm" onClick={clearDocumentFilters}>
                    {t("database.clearFilter")}
                  </DbxButton>
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                    <DbxButton variant="outline" size="sm" onClick={resetStructuredFilterBuilder}>
                      {t("database.resetFilterBuilder")}
                    </DbxButton>
                    <DbxButton variant="default" size="sm" onClick={applyStructuredFilters}>
                      {t("database.applyFilter")}
                    </DbxButton>
                  </span>
                </div>
              </div>
            )}
          </span>
          <DbxButton
            variant="outline"
            size="sm"
            onClick={() => loadDocuments()}
            disabled={!activeDatabase || !activeCollection}
          >
            {t("database.applyFilter")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            onClick={clearDocumentFilters}
            disabled={!activeDatabase || !activeCollection}
          >
            {t("database.clearFilter")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Plus}
            disabled={readOnly || !activeDatabase || !activeCollection}
            onClick={() => void insertDocument()}
          >
            {t("database.mongoInsertDocument")}
          </DbxButton>
          <DbxButton
            variant="destructive"
            size="sm"
            icon={Trash2}
            disabled={readOnly || !activeDatabase || !activeCollection}
            onClick={() => void deleteMatchingDocuments()}
          >
            {t("database.mongoDeleteMatchingDocuments")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            disabled={
              !activeDatabase ||
              !activeCollection ||
              page * pageSize + mongo.documents.length >= mongo.total ||
              mongo.loading
            }
            onClick={loadMoreDocuments}
          >
            {t("database.loadMore")} ({mongo.documents.length}/{mongo.total})
          </DbxButton>
          <DbxButton
            variant="ghost"
            size="icon-sm"
            icon={ChevronLeft}
            aria-label={t("database.previousPage")}
            disabled={!activeDatabase || !activeCollection || page <= 0 || mongo.loading}
            onClick={() => loadDocumentPage(page - 1)}
          />
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {t("database.page", {
              page: page + 1,
              total: Math.max(1, Math.ceil(mongo.total / pageSize)),
            })}
          </span>
          <label
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              fontSize: 12,
              color: "var(--text-muted)",
            }}
          >
            <span>{t("database.gridRowsPerPage")}</span>
            <select
              style={{ ...s.databaseDialogInput, width: 82, height: 28, padding: "0 6px" }}
              value={pageSize}
              disabled={mongo.loading}
              aria-label={t("database.gridRowsPerPage")}
              onChange={(event) => {
                changePageSize(Number(event.currentTarget.value));
              }}
            >
              {MONGO_WORKSPACE_PAGE_SIZE_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </label>
          <DbxButton
            variant="ghost"
            size="icon-sm"
            icon={ChevronRight}
            aria-label={t("database.nextPage")}
            disabled={
              !activeDatabase ||
              !activeCollection ||
              (page + 1) * pageSize >= mongo.total ||
              mongo.loading
            }
            onClick={() => loadDocumentPage(page + 1)}
          />
        </div>
        {viewMode === "document" ? (
          <div style={s.databaseDocumentList}>
            {mongo.documents.map((document, index) => (
              <pre
                key={index}
                style={{
                  ...s.databaseDocumentCard,
                  ...(selectedIndex === index ? s.databaseListButtonActive : {}),
                  cursor: "pointer",
                }}
                onClick={() => selectDocument(index, document)}
                onContextMenu={(event) => openDocumentContextMenu(event, index, document)}
              >
                {JSON.stringify(document, null, 2)}
              </pre>
            ))}
          </div>
        ) : (
          <div style={s.databaseTableWrap}>
            <table style={s.databaseTable}>
              <thead>
                <tr>
                  {visibleTableColumns.map((column) => {
                    const direction = mongoSortDirection(sort, column);
                    return (
                      <th
                        key={column}
                        style={s.databaseTh}
                        aria-sort={
                          direction === "asc"
                            ? "ascending"
                            : direction === "desc"
                              ? "descending"
                              : "none"
                        }
                      >
                        <button
                          type="button"
                          style={{
                            width: "100%",
                            minWidth: 0,
                            border: "none",
                            background: "transparent",
                            color: "inherit",
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 6,
                            padding: 0,
                            font: "inherit",
                            fontWeight: 700,
                            textAlign: "left",
                            cursor: activeDatabase && activeCollection ? "pointer" : "default",
                          }}
                          disabled={!activeDatabase || !activeCollection || mongo.loading}
                          onClick={() => sortByColumn(column)}
                        >
                          <span
                            style={{
                              minWidth: 0,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {column}
                          </span>
                          {direction === "asc" && <ArrowUp size={12} aria-hidden="true" />}
                          {direction === "desc" && <ArrowDown size={12} aria-hidden="true" />}
                        </button>
                      </th>
                    );
                  })}
                </tr>
              </thead>
              <tbody>
                {mongo.documents.map((document, index) => (
                  <tr key={index}>
                    {visibleTableColumns.map((column) => (
                      <td
                        key={column}
                        style={s.databaseTd}
                        onClick={() => selectDocument(index, document)}
                        onContextMenu={(event) => openDocumentContextMenu(event, index, document)}
                      >
                        {mongoCellText(mongoCellValue(document, column))}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <textarea
          aria-label={t("database.documentJson")}
          style={{ ...s.databaseLargeTextArea, minHeight: 96, flex: "0 0 120px" }}
          value={documentDraft}
          onChange={(event) => setDocumentDraft(event.target.value)}
          readOnly={readOnly}
        />
        <div style={s.databaseButtonRow}>
          <DbxButton
            variant="default"
            size="sm"
            disabled={readOnly || selectedIndex == null}
            onClick={() => void saveDocument()}
          >
            {t("database.saveDocument")}
          </DbxButton>
        </div>
        {mongo.error && <div style={s.databaseError}>{mongo.error}</div>}
      </div>
      {collectionContextMenu && (
        <>
          <div style={s.fileCtxBackdrop} onClick={() => setCollectionContextMenu(null)} />
          <div
            role="menu"
            style={{
              ...s.fileCtxMenu,
              left: collectionContextMenu.x,
              top: collectionContextMenu.y,
              minWidth: 210,
            }}
          >
            {(
              [
                ["copyName", "database.copyName"],
                ["refresh", "database.refresh"],
                ["insertDocument", "database.mongoInsertDocument"],
                ["deleteDocuments", "database.mongoDeleteMatchingDocuments"],
              ] as const
            ).map(([action, labelKey]) => (
              <DbxMenuItem
                key={action}
                icon={
                  action === "copyName"
                    ? Copy
                    : action === "refresh"
                      ? RefreshCcw
                      : action === "insertDocument"
                        ? Plus
                        : Trash2
                }
                disabled={(action === "insertDocument" || action === "deleteDocuments") && readOnly}
                destructive={action === "deleteDocuments"}
                onClick={() => {
                  const menu = collectionContextMenu;
                  setCollectionContextMenu(null);
                  if (action === "copyName") {
                    copyCollectionName(menu.collection);
                  } else if (action === "refresh") {
                    selectCollection(menu.collection);
                  } else if (action === "insertDocument") {
                    void insertDocument(menu.collection);
                  } else {
                    void deleteMatchingDocuments(menu.collection);
                  }
                }}
              >
                {t(labelKey)}
              </DbxMenuItem>
            ))}
          </div>
        </>
      )}
      {documentContextMenu && (
        <>
          <div style={s.fileCtxBackdrop} onClick={() => setDocumentContextMenu(null)} />
          <div
            role="menu"
            style={{
              ...s.fileCtxMenu,
              left: documentContextMenu.x,
              top: documentContextMenu.y,
              minWidth: 210,
            }}
          >
            {(
              [
                ["copyDocument", "database.copyDocumentJson"],
                ["refresh", "database.refresh"],
                ["deleteDocument", "database.mongoDeleteDocument"],
              ] as const
            ).map(([action, labelKey]) => (
              <DbxMenuItem
                key={action}
                icon={action === "copyDocument" ? Copy : action === "refresh" ? RefreshCcw : Trash2}
                disabled={
                  action === "deleteDocument" &&
                  (readOnly || mongoDocumentRawId(documentContextMenu.document) == null)
                }
                destructive={action === "deleteDocument"}
                onClick={() => {
                  const menu = documentContextMenu;
                  setDocumentContextMenu(null);
                  if (action === "copyDocument") {
                    copyDocumentJson(menu.document);
                  } else if (action === "refresh") {
                    const id = mongoDocumentId(menu.document);
                    if (id) void refreshDocumentsKeepingSelection(id);
                  } else {
                    void deleteDocument(menu.document);
                  }
                }}
              >
                {t(labelKey)}
              </DbxMenuItem>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
