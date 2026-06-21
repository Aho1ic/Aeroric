import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { DatabaseSidebarTree } from "../components/database/DatabaseSidebarTree";
import type { AeroricDbConnectionConfig, DbConnectionConfig, DbObject, DbxColumnInfo, DbxObjectInfo } from "../types";

const legacyConnection: DbConnectionConfig = {
  id: "legacy",
  name: "local.db",
  endpoint: { kind: "local", path: "/tmp/local.db" },
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

const dbxConnection: AeroricDbConnectionConfig = {
  id: "dbx",
  name: "DBX Source",
  dbType: "postgres",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
  connectionGroup: "Production",
  pinned: true,
};

const redisConnection: AeroricDbConnectionConfig = {
  id: "redis",
  name: "Redis Source",
  dbType: "redis",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

const mongoConnection: AeroricDbConnectionConfig = {
  id: "mongo",
  name: "Mongo Source",
  dbType: "mongodb",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

const legacyObject: DbObject = {
  name: "notes",
  objectType: "table",
  columns: [],
  indexes: [],
  foreignKeys: [],
  triggers: [],
  editable: true,
  primaryKeys: ["id"],
  hasRowId: false,
};

const usersObject: DbxObjectInfo = { name: "users", object_type: "TABLE", schema: "public" };
const activeUsersView: DbxObjectInfo = { name: "active_users", object_type: "VIEW", schema: "public" };
const usersIndex: DbxObjectInfo = {
  name: "users_email_idx",
  object_type: "INDEX",
  schema: "public",
  parent_name: "users",
  parent_schema: "public",
};

const userColumns: DbxColumnInfo[] = [
  { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
  { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
];

function renderTree(overrides: Partial<React.ComponentProps<typeof DatabaseSidebarTree>> = {}) {
  const props: React.ComponentProps<typeof DatabaseSidebarTree> = {
    connections: [legacyConnection],
    dbxConnections: [dbxConnection],
    activeConnectionId: null,
    activeDbxConnectionId: "dbx",
    activeDbxConnection: dbxConnection,
    activeDbxDatabase: "main",
    activeDbxSchema: "public",
    activeObject: null,
    activeDbxObject: usersObject,
    userAdminActive: false,
    dbxHasSqlObjectBrowser: true,
    visibleDbxDatabases: [{ name: "main" }],
    dbxSchemas: ["public"],
    legacyObjects: [legacyObject],
    dbxObjects: [usersObject, activeUsersView, usersIndex],
    dbxColumnsByTable: { "public.users": userColumns },
    redisDatabasesByConnection: {},
    redisKeysByDatabase: {},
    redisScanStateByDatabase: {},
    mongoDatabasesByConnection: {},
    mongoCollectionsByDatabase: {},
    mongoDocumentsByCollection: {},
    mongoDocumentTotalsByCollection: {},
    activeMongoDocumentId: null,
    onSelectConnection: vi.fn(),
    onSelectDbxConnection: vi.fn(),
    onDeleteConnection: vi.fn(),
    onDeleteDbxConnection: vi.fn(),
    onSelectDatabase: vi.fn(),
    onSelectDbxSchema: vi.fn(),
    onSelectLegacyObject: vi.fn(),
    onSelectDbxObject: vi.fn(),
    onOpenUserAdmin: vi.fn(),
    onOpenNoSqlWorkspace: vi.fn(),
    onSelectRedisDatabase: vi.fn(),
    onExpandRedisDatabase: vi.fn(),
    onLoadMoreRedisKeys: vi.fn(),
    onSelectRedisKey: vi.fn(),
    onRedisKeyContextMenu: vi.fn(),
    onSelectMongoDatabase: vi.fn(),
    onExpandMongoDatabase: vi.fn(),
    onSelectMongoCollection: vi.fn(),
    onExpandMongoCollection: vi.fn(),
    onLoadMoreMongoDocuments: vi.fn(),
    onSelectMongoDocument: vi.fn(),
    onMongoDocumentContextMenu: vi.fn(),
    onRenameConnection: vi.fn(),
    onRenameDbxConnection: vi.fn(),
    onRefreshConnection: vi.fn(),
    onRefreshDbxConnection: vi.fn(),
    onRefreshDatabase: vi.fn(),
    onRefreshDbxSchema: vi.fn(),
    onCopyNodeName: vi.fn(),
    onDropDatabase: vi.fn(),
    onDropDbxSchema: vi.fn(),
    onDropDbxObject: vi.fn(),
    onDropDbxColumn: vi.fn(),
    onDropDbxTableChildObject: vi.fn(),
    onConnectionContextMenu: vi.fn(),
    onConnectionGroupContextMenu: vi.fn(),
    onUserAdminContextMenu: vi.fn(),
    onDbxDatabaseContextMenu: vi.fn(),
    onDbxSchemaContextMenu: vi.fn(),
    onDbxObjectContextMenu: vi.fn(),
    onDbxColumnContextMenu: vi.fn(),
    onDbxTableChildObjectContextMenu: vi.fn(),
    onDbxObjectGroupContextMenu: vi.fn(),
    onRedisDatabaseContextMenu: vi.fn(),
    onMongoDatabaseContextMenu: vi.fn(),
    onMongoCollectionContextMenu: vi.fn(),
    ...overrides,
  };

  render(
    <I18nProvider>
      <DatabaseSidebarTree {...props} />
    </I18nProvider>,
  );

  return props;
}

function createMockDataTransfer(initialData: Record<string, string> = {}) {
  const data = new Map(Object.entries(initialData));
  return {
    effectAllowed: "all",
    dropEffect: "none",
    setData: vi.fn((type: string, value: string) => {
      data.set(type, value);
    }),
    getData: vi.fn((type: string) => data.get(type) ?? ""),
    clearData: vi.fn((type?: string) => {
      if (type) data.delete(type);
      else data.clear();
    }),
  } as unknown as DataTransfer;
}

describe("DatabaseSidebarTree", () => {
  it("renders DBX connection, database, schema, grouped objects, columns, and table child objects", async () => {
    renderTree();

    const connectionButton = await screen.findByRole("button", { name: /DBX Source/i });
    expect(connectionButton).toBeInTheDocument();
    expect(within(connectionButton).getByText("DS")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^main$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^public$/i })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Tables" }).length).toBeGreaterThanOrEqual(2);
    expect(screen.getByRole("button", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^users\s+TABLE$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^active_users\s+VIEW$/i })).toBeInTheDocument();
    expect(screen.queryByText("public.users")).not.toBeInTheDocument();
    expect(screen.getByText("email").closest("button")).toBeInTheDocument();
    expect(screen.getByText("users_email_idx").closest("button")).toBeInTheDocument();
  });

  it("renders DBX user admin utility node for supported SQL connections", async () => {
    const props = renderTree({ userAdminActive: true });

    const userAdminButton = await screen.findByRole("button", { name: "Users and permissions" });
    expect(userAdminButton).toHaveAttribute("data-selected", "true");

    await userEvent.click(userAdminButton);
    expect(props.onOpenUserAdmin).toHaveBeenCalledWith(dbxConnection);

    fireEvent.contextMenu(userAdminButton);
    expect(props.onUserAdminContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx");
  });

  it("does not render DBX user admin utility node for unsupported connections", async () => {
    renderTree({
      dbxConnections: [{ ...dbxConnection, dbType: "sqlite" }],
      activeDbxConnection: { ...dbxConnection, dbType: "sqlite" },
    });

    expect(screen.queryByRole("button", { name: "Users and permissions" })).not.toBeInTheDocument();
  });

  it("orders pinned DBX database, schema, and object nodes before unpinned siblings", async () => {
    const accountsObject: DbxObjectInfo = { name: "accounts", object_type: "TABLE", schema: "public" };
    renderTree({
      activeDbxDatabase: "zeta",
      activeDbxSchema: "public",
      activeDbxObject: usersObject,
      visibleDbxDatabases: [{ name: "zeta" }, { name: "main" }],
      dbxSchemas: ["public", "archive"],
      dbxObjects: [accountsObject, usersObject],
      pinnedTreeNodeIds: new Set([
        "dbx-database:main",
        "dbx-schema:zeta:archive",
        "dbx-object:TABLE:public:users",
      ]),
    });

    const pinnedDatabase = await screen.findByRole("button", { name: /main/i });
    const unpinnedDatabase = screen.getByRole("button", { name: /^zeta$/i });
    expect(within(pinnedDatabase).getByLabelText("Pinned")).toBeInTheDocument();
    expect(pinnedDatabase.compareDocumentPosition(unpinnedDatabase) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    const pinnedSchema = screen.getByRole("button", { name: /archive/i });
    const unpinnedSchema = screen.getByRole("button", { name: /^public$/i });
    expect(within(pinnedSchema).getByLabelText("Pinned")).toBeInTheDocument();
    expect(pinnedSchema.compareDocumentPosition(unpinnedSchema) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    const pinnedTable = screen.getByRole("button", { name: /^users\s+TABLE$/i });
    const unpinnedTable = screen.getByRole("button", { name: /^accounts\s+TABLE$/i });
    expect(within(pinnedTable).getByLabelText("Pinned")).toBeInTheDocument();
    expect(pinnedTable.compareDocumentPosition(unpinnedTable) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("fires context menu callbacks for object, column, table child object, schema, database, and group nodes", async () => {
    const props = renderTree();

    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+TABLE$/i }));
    expect(props.onDbxObjectContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx", "main", usersObject);

    fireEvent.contextMenu(screen.getByText("email").closest("button") as HTMLButtonElement);
    expect(props.onDbxColumnContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx", "main", usersObject, userColumns[1]);

    fireEvent.contextMenu(screen.getByText("users_email_idx").closest("button") as HTMLButtonElement);
    expect(props.onDbxTableChildObjectContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx", "main", usersObject, usersIndex);

    fireEvent.contextMenu(screen.getByRole("button", { name: /^public$/i }));
    expect(props.onDbxSchemaContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx", "main", "public");

    fireEvent.contextMenu(screen.getByRole("button", { name: /^main$/i }));
    expect(props.onDbxDatabaseContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx", "main");

    const publicSchemaBranch = screen.getByRole("button", { name: /^public$/i }).parentElement!;
    fireEvent.contextMenu(within(publicSchemaBranch).getByRole("button", { name: "Tables" }));
    expect(props.onDbxObjectGroupContextMenu).toHaveBeenCalledWith(expect.anything(), "dbx", "main", "public", "tables", "Tables");

    fireEvent.contextMenu(screen.getByRole("button", { name: /Production/i }));
    expect(props.onConnectionGroupContextMenu).toHaveBeenCalledWith(expect.anything(), "Production");
  });

  it("supports copy, refresh, rename, and delete keyboard shortcuts on DBX tree nodes", async () => {
    const props = renderTree();
    const connectionButton = await screen.findByRole("button", { name: /DBX Source/i });

    fireEvent.keyDown(connectionButton, { key: "c", metaKey: true });
    expect(props.onCopyNodeName).toHaveBeenCalledWith("DBX Source");

    fireEvent.keyDown(connectionButton, { key: "F5" });
    expect(props.onRefreshDbxConnection).toHaveBeenCalledWith(dbxConnection);

    fireEvent.keyDown(connectionButton, { key: "F2" });
    expect(props.onRenameDbxConnection).toHaveBeenCalledWith(dbxConnection);

    fireEvent.keyDown(connectionButton, { key: "Delete" });
    expect(props.onDeleteDbxConnection).toHaveBeenCalledWith("dbx");

    const tableButton = screen.getByRole("button", { name: /^users\s+TABLE$/i });
    fireEvent.keyDown(tableButton, { key: "Backspace" });
    expect(props.onDropDbxObject).toHaveBeenCalledWith(dbxConnection, "main", usersObject);
  });

  it("supports Ctrl/Cmd multi-select without activating additional DBX tree nodes", async () => {
    const props = renderTree();
    const usersButton = await screen.findByRole("button", { name: /^users\s+TABLE$/i });
    const viewButton = screen.getByRole("button", { name: /^active_users\s+VIEW$/i });

    fireEvent.click(usersButton);
    expect(usersButton).toHaveAttribute("data-selected", "true");
    expect(props.onSelectDbxObject).toHaveBeenCalledWith(usersObject);

    fireEvent.click(viewButton, { metaKey: true });
    expect(usersButton).toHaveAttribute("data-selected", "true");
    expect(viewButton).toHaveAttribute("data-selected", "true");
    expect(props.onSelectDbxObject).not.toHaveBeenCalledWith(activeUsersView);

    fireEvent.click(usersButton, { ctrlKey: true });
    expect(usersButton).not.toHaveAttribute("data-selected");
    expect(viewButton).toHaveAttribute("data-selected", "true");
  });

  it("supports Shift range selection across visible DBX tree nodes", async () => {
    renderTree();
    const usersButton = await screen.findByRole("button", { name: /^users\s+TABLE$/i });
    const emailButton = screen.getByText("email").closest("button") as HTMLButtonElement;
    const indexButton = screen.getByText("users_email_idx").closest("button") as HTMLButtonElement;
    const viewButton = screen.getByRole("button", { name: /^active_users\s+VIEW$/i });

    fireEvent.click(usersButton);
    fireEvent.click(indexButton, { shiftKey: true });

    expect(usersButton).toHaveAttribute("data-selected", "true");
    expect(emailButton).toHaveAttribute("data-selected", "true");
    expect(indexButton).toHaveAttribute("data-selected", "true");
    expect(viewButton).not.toHaveAttribute("data-selected");
  });

  it("renders Redis database nodes with key counts and opens a selected DB", async () => {
    const props = renderTree({
      dbxConnections: [redisConnection],
      activeDbxConnectionId: "redis",
      activeDbxConnection: redisConnection,
      activeDbxDatabase: "db0",
      activeDbxSchema: null,
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      redisDatabasesByConnection: { redis: [{ db: 0, keys: 12 }, { db: 2, keys: 0 }] },
    });

    expect(await screen.findByRole("button", { name: /Redis Source/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /db012/i })).toHaveAttribute("data-selected", "true");
    await userEvent.click(screen.getByRole("button", { name: /db20/i }));

    expect(props.onSelectRedisDatabase).toHaveBeenCalledWith(redisConnection, 2);
  });

  it("renders lazy-loaded Redis keys below expanded database nodes", async () => {
    const props = renderTree({
      dbxConnections: [redisConnection],
      activeDbxConnectionId: "redis",
      activeDbxConnection: redisConnection,
      activeDbxDatabase: "db0",
      activeDbxSchema: "user:1",
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      redisDatabasesByConnection: { redis: [{ db: 0, keys: 12 }] },
      redisKeysByDatabase: {
        "redis:0": [
          { key_display: "user:1", key_raw: "user:1", key_type: "string", ttl: -1, size: 3, value_preview: "Ada" },
        ],
      },
      redisScanStateByDatabase: { "redis:0": { cursor: 42, totalKeys: 12 } },
    });

    const dbButton = await screen.findByRole("button", { name: /db012/i });
    const expandGlyph = dbButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(expandGlyph);
    expect(props.onExpandRedisDatabase).toHaveBeenCalledWith(redisConnection, 0);

    const keyButton = (await screen.findByText("user:1")).closest("button") as HTMLButtonElement;
    expect(keyButton).toHaveAttribute("data-selected", "true");
    fireEvent.contextMenu(keyButton);
    expect(props.onRedisKeyContextMenu).toHaveBeenCalledWith(expect.anything(), "redis", 0, "user:1");
    await userEvent.click(screen.getByRole("button", { name: /Load more \(1\/12\)/i }));
    expect(props.onLoadMoreRedisKeys).toHaveBeenCalledWith(redisConnection, 0);
    await userEvent.click(keyButton);
    expect(props.onSelectRedisKey).toHaveBeenCalledWith(redisConnection, 0, "user:1");
  });

  it("renders MongoDB databases and collections in the sidebar tree", async () => {
    const user = userEvent.setup();
    const props = renderTree({
      dbxConnections: [mongoConnection],
      activeDbxConnectionId: "mongo",
      activeDbxConnection: mongoConnection,
      activeDbxDatabase: "app",
      activeDbxSchema: "users",
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      mongoDatabasesByConnection: { mongo: ["app", "logs"] },
      mongoCollectionsByDatabase: { "mongo:app": ["users", "events"] },
    });

    expect(await screen.findByRole("button", { name: /Mongo Source/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^app$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^users$/i })).toHaveAttribute("data-selected", "true");

    await user.click(screen.getByRole("button", { name: /^events$/i }));
    expect(props.onSelectMongoCollection).toHaveBeenCalledWith(mongoConnection, "app", "events");

    await user.click(screen.getByRole("button", { name: /^logs$/i }));
    expect(props.onSelectMongoDatabase).toHaveBeenCalledWith(mongoConnection, "logs");
  });

  it("requests MongoDB collection lazy loading when a database node is expanded", async () => {
    const props = renderTree({
      dbxConnections: [mongoConnection],
      activeDbxConnectionId: "mongo",
      activeDbxConnection: mongoConnection,
      activeDbxDatabase: "app",
      activeDbxSchema: null,
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      mongoDatabasesByConnection: { mongo: ["app", "logs"] },
      mongoCollectionsByDatabase: { "mongo:app": ["users"] },
    });

    const logsButton = await screen.findByRole("button", { name: /^logs$/i });
    const expandGlyph = logsButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(expandGlyph);

    expect(props.onExpandMongoDatabase).toHaveBeenCalledWith(mongoConnection, "logs");
    expect(props.onSelectMongoDatabase).not.toHaveBeenCalled();
  });

  it("renders MongoDB document previews below expanded collection nodes", async () => {
    const document = { _id: "1", name: "Ada" };
    const props = renderTree({
      dbxConnections: [mongoConnection],
      activeDbxConnectionId: "mongo",
      activeDbxConnection: mongoConnection,
      activeDbxDatabase: "app",
      activeDbxSchema: "users",
      activeMongoDocumentId: "1",
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      mongoDatabasesByConnection: { mongo: ["app"] },
      mongoCollectionsByDatabase: { "mongo:app": ["users"] },
      mongoDocumentsByCollection: { "mongo:app:users": [document] },
      mongoDocumentTotalsByCollection: { "mongo:app:users": 2 },
    });

    const documentButton = (await screen.findByText(/1 name: Ada/)).closest("button") as HTMLButtonElement;
    expect(documentButton).toHaveAttribute("data-selected", "true");
    fireEvent.contextMenu(documentButton);
    expect(props.onMongoDocumentContextMenu).toHaveBeenCalledWith(expect.anything(), "mongo", "app", "users", document);
    await userEvent.click(screen.getByRole("button", { name: /Load more \(1\/2\)/i }));
    expect(props.onLoadMoreMongoDocuments).toHaveBeenCalledWith(mongoConnection, "app", "users");
    await userEvent.click(documentButton);
    expect(props.onSelectMongoDocument).toHaveBeenCalledWith(mongoConnection, "app", "users", document);
  });

  it("fires context menu callbacks for Redis and MongoDB tree nodes", async () => {
    let props = renderTree({
      dbxConnections: [mongoConnection],
      activeDbxConnectionId: "mongo",
      activeDbxConnection: mongoConnection,
      activeDbxDatabase: "app",
      activeDbxSchema: "users",
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      redisDatabasesByConnection: { redis: [{ db: 0, keys: 12 }] },
      mongoDatabasesByConnection: { mongo: ["app"] },
      mongoCollectionsByDatabase: { "mongo:app": ["users"] },
    });

    fireEvent.contextMenu(await screen.findByRole("button", { name: /^app$/i }));
    expect(props.onMongoDatabaseContextMenu).toHaveBeenCalledWith(expect.anything(), "mongo", "app");

    fireEvent.contextMenu(screen.getByRole("button", { name: /^users$/i }));
    expect(props.onMongoCollectionContextMenu).toHaveBeenCalledWith(expect.anything(), "mongo", "app", "users");

    cleanup();
    props = renderTree({
      dbxConnections: [redisConnection],
      activeDbxConnectionId: "redis",
      activeDbxConnection: redisConnection,
      activeDbxDatabase: "db0",
      activeDbxSchema: null,
      activeDbxObject: null,
      dbxHasSqlObjectBrowser: false,
      visibleDbxDatabases: [],
      dbxSchemas: [],
      dbxObjects: [],
      dbxColumnsByTable: {},
      redisDatabasesByConnection: { redis: [{ db: 0, keys: 12 }] },
    });

    fireEvent.contextMenu(await screen.findByRole("button", { name: /db012/i }));
    expect(props.onRedisDatabaseContextMenu).toHaveBeenCalledWith(expect.anything(), "redis", 0);
  });

  it("publishes DBX table references for editor drag-and-drop", async () => {
    renderTree();
    const dataTransfer = createMockDataTransfer();

    fireEvent.dragStart(await screen.findByRole("button", { name: /^users\s+TABLE$/i }), { dataTransfer });

    expect(dataTransfer.setData).toHaveBeenCalledWith("text/plain", "public.users");
    expect(dataTransfer.setData).toHaveBeenCalledWith(
      "application/x-aeroric-database-object",
      JSON.stringify({
        name: "users",
        schema: "public",
        objectType: "TABLE",
        reference: "public.users",
      }),
    );
  });

  it("filters tree search results while keeping matched object parents visible", async () => {
    const user = userEvent.setup();
    renderTree();

    await user.type(screen.getByLabelText("Sidebar search"), "email");

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /^active_users\s+VIEW$/i })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /^users\s+TABLE$/i })).toBeInTheDocument();
    expect(screen.getByText("email").closest("button")).toBeInTheDocument();
    expect(screen.getByText("users_email_idx").closest("button")).toBeInTheDocument();
  });
});
