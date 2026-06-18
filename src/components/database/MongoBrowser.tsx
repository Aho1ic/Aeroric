import { useEffect, useState } from "react";
import { Plus, RefreshCcw, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { useMongoBrowser } from "../../hooks/useMongoBrowser";
import s from "../../styles";

interface Props {
  connectionId: string;
  readOnly: boolean;
}

export function MongoBrowser({ connectionId, readOnly }: Props) {
  const { t } = useI18n();
  const mongo = useMongoBrowser(connectionId);
  const [activeDatabase, setActiveDatabase] = useState("");
  const [activeCollection, setActiveCollection] = useState("");
  const [filter, setFilter] = useState("{}");
  const [sort, setSort] = useState("{}");
  const [viewMode, setViewMode] = useState<"document" | "table">("document");
  const [documentDraft, setDocumentDraft] = useState("{\n  \n}");
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);

  useEffect(() => {
    mongo.loadDatabases().catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId]);

  const loadDocuments = (database = activeDatabase, collection = activeCollection) => {
    if (!database || !collection) return;
    setSelectedIndex(null);
    void mongo.findDocuments({
      database,
      collection,
      filter,
      sort,
      skip: 0,
      limit: 100,
    });
  };

  const selectDocument = (index: number, document: unknown) => {
    setSelectedIndex(index);
    setDocumentDraft(JSON.stringify(document, null, 2));
  };

  const saveDocument = async () => {
    if (selectedIndex == null || !activeDatabase || !activeCollection) return;
    const selected = mongo.documents[selectedIndex];
    const id = selected && typeof selected === "object" && "_id" in selected ? String((selected as { _id: unknown })._id) : "";
    if (!id) return;
    await mongo.updateDocument({
      database: activeDatabase,
      collection: activeCollection,
      id,
      docJson: documentDraft,
    });
    loadDocuments();
  };

  return (
    <div style={s.databaseBrowserRoot}>
      <div style={s.databaseBrowserSidebar}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>MongoDB</div>
            <div style={s.databaseDialogHint}>{t("database.mongoBrowserHint")}</div>
          </div>
          <button type="button" style={s.databaseIconButton} onClick={() => loadDocuments()} aria-label={t("database.refresh")}>
            <RefreshCcw size={14} />
          </button>
        </div>
        <div style={s.databaseSection}>
          <div style={s.databaseSectionTitle}>{t("database.databases")}</div>
          {mongo.databases.map((database) => (
            <button
              key={database}
              type="button"
              style={{ ...s.databaseListButton, ...(activeDatabase === database ? s.databaseListButtonActive : {}) }}
              onClick={() => {
                setActiveDatabase(database);
                setActiveCollection("");
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
              style={{ ...s.databaseListButton, ...(activeCollection === collection ? s.databaseListButtonActive : {}) }}
              onClick={() => {
                setActiveCollection(collection);
                loadDocuments(activeDatabase, collection);
              }}
            >
              {collection}
            </button>
          ))}
        </div>
      </div>
      <div style={s.databaseBrowserMain}>
        <div style={s.databaseWorkspaceHeader}>
          <div>
            <div style={s.databaseWorkspaceTitle}>{activeCollection || t("database.mongoCollection")}</div>
            <div style={s.databaseDialogHint}>{t("database.mongoTotal", { total: mongo.total })}</div>
          </div>
          <div style={s.databaseButtonRow}>
            <button
              type="button"
              style={{ ...s.databaseSmallButton, ...(viewMode === "document" ? s.databaseListButtonActive : {}) }}
              onClick={() => setViewMode("document")}
            >
              {t("database.documentMode")}
            </button>
            <button
              type="button"
              style={{ ...s.databaseSmallButton, ...(viewMode === "table" ? s.databaseListButtonActive : {}) }}
              onClick={() => setViewMode("table")}
            >
              {t("database.tableMode")}
            </button>
          </div>
        </div>
        <div style={s.databaseDialogFormGrid}>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.filterJson")}</span>
            <input style={s.databaseDialogInput} value={filter} onChange={(event) => setFilter(event.target.value)} />
          </label>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.sortJson")}</span>
            <input style={s.databaseDialogInput} value={sort} onChange={(event) => setSort(event.target.value)} />
          </label>
        </div>
        <div style={s.databaseToolbar}>
          <button type="button" style={s.databaseSmallButton} onClick={() => loadDocuments()} disabled={!activeDatabase || !activeCollection}>
            {t("database.applyFilter")}
          </button>
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={readOnly || !activeDatabase || !activeCollection}
            onClick={() => void mongo.insertDocument({ database: activeDatabase, collection: activeCollection, docJson: documentDraft })}
          >
            <Plus size={13} />
            {t("database.mongoInsertDocument")}
          </button>
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={readOnly || !activeDatabase || !activeCollection}
            onClick={() =>
              void mongo.deleteDocuments({
                database: activeDatabase,
                collection: activeCollection,
                filterJson: filter,
                many: true,
              })
            }
          >
            <Trash2 size={13} />
            {t("database.mongoDeleteDocument")}
          </button>
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
              >
                {JSON.stringify(document, null, 2)}
              </pre>
            ))}
          </div>
        ) : (
          <div style={s.databaseTableWrap}>
            <table style={s.databaseTable}>
              <tbody>
                {mongo.documents.map((document, index) => (
                  <tr key={index}>
                    <td style={s.databaseTd}>{JSON.stringify(document)}</td>
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
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={readOnly || selectedIndex == null}
            onClick={() => void saveDocument()}
          >
            {t("database.saveDocument")}
          </button>
        </div>
        {mongo.error && <div style={s.databaseError}>{mongo.error}</div>}
      </div>
    </div>
  );
}
