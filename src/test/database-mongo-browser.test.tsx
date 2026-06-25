import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { MongoBrowser } from "../components/database/MongoBrowser";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: vi.fn() }));

describe("MongoBrowser", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    window.localStorage.clear();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
  });

  it("loads databases, collections, and documents", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();
  });

  it("clears stale Mongo collection documents when switching databases", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app", "logs"]);
      if (command === "dbx_mongo_list_collections") {
        const request = args as { database?: string };
        return Promise.resolve(request.database === "logs" ? ["events"] : ["users"]);
      }
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(await screen.findByText(/Ada/));
    expect(screen.getByLabelText(/文档 JSON|Document JSON/)).toHaveValue(
      JSON.stringify({ _id: "1", name: "Ada" }, null, 2),
    );

    await userEvent.click(screen.getByText("logs"));

    expect(screen.queryByText(/Ada/)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/文档 JSON|Document JSON/)).toHaveValue("{\n  \n}");
    expect(await screen.findByText("events")).toBeInTheDocument();
  });

  it("ignores stale Mongo collections when switching databases", async () => {
    let resolveAppCollections: (value: string[]) => void = () => undefined;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app", "logs"]);
      if (command === "dbx_mongo_list_collections") {
        const request = args as { database?: string };
        if (request.database === "app") {
          return new Promise((resolve) => {
            resolveAppCollections = resolve;
          });
        }
        return Promise.resolve(["events"]);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(screen.getByText("logs"));

    expect(await screen.findByText("events")).toBeInTheDocument();
    resolveAppCollections(["users"]);

    expect(await screen.findByText("events")).toBeInTheDocument();
    expect(screen.queryByText("users")).not.toBeInTheDocument();
  });

  it("clears Mongo collections immediately when switching databases", async () => {
    let resolveLogCollections: (value: string[]) => void = () => undefined;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app", "logs"]);
      if (command === "dbx_mongo_list_collections") {
        const request = args as { database?: string };
        if (request.database === "logs") {
          return new Promise((resolve) => {
            resolveLogCollections = resolve;
          });
        }
        return Promise.resolve(["users"]);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    expect(await screen.findByText("users")).toBeInTheDocument();

    await userEvent.click(screen.getByText("logs"));

    expect(screen.queryByText("users")).not.toBeInTheDocument();

    resolveLogCollections(["events"]);
    expect(await screen.findByText("events")).toBeInTheDocument();
  });

  it("clears stale Mongo documents immediately when switching collections", async () => {
    let resolveAuditDocuments: (value: { documents: unknown[]; total: number }) => void = () =>
      undefined;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users", "audits"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { collection?: string };
        if (request.collection === "audits") {
          return new Promise((resolve) => {
            resolveAuditDocuments = resolve;
          });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(await screen.findByText(/Ada/));
    expect(screen.getByLabelText(/文档 JSON|Document JSON/)).toHaveValue(
      JSON.stringify({ _id: "1", name: "Ada" }, null, 2),
    );

    await userEvent.click(screen.getByText("audits"));

    expect(screen.queryByText(/Ada/)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/文档 JSON|Document JSON/)).toHaveValue("{\n  \n}");

    resolveAuditDocuments({ documents: [{ _id: "2", event: "login" }], total: 1 });
    expect(await screen.findByText(/login/)).toBeInTheDocument();
  });

  it("ignores late Mongo document results from the previous collection", async () => {
    let resolveUsersDocuments: (value: { documents: unknown[]; total: number }) => void = () =>
      undefined;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users", "audits"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { collection?: string };
        if (request.collection === "users") {
          return new Promise((resolve) => {
            resolveUsersDocuments = resolve;
          });
        }
        return Promise.resolve({ documents: [{ _id: "2", event: "login" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByText("audits"));
    expect(await screen.findByText(/login/)).toBeInTheDocument();

    resolveUsersDocuments({ documents: [{ _id: "1", name: "Ada" }], total: 1 });

    expect(await screen.findByText(/login/)).toBeInTheDocument();
    expect(screen.queryByText(/Ada/)).not.toBeInTheDocument();
  });

  it("clears stale Mongo workspace state when switching connections", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") {
        const request = args as { connectionId?: string };
        return Promise.resolve(request.connectionId === "mongo-b" ? ["logs"] : ["app"]);
      }
      if (command === "dbx_mongo_list_collections") {
        const request = args as { connectionId?: string };
        return Promise.resolve(request.connectionId === "mongo-b" ? ["events"] : ["users"]);
      }
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    const { rerender } = render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo-a" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(await screen.findByText(/Ada/));
    expect(screen.getByLabelText(/文档 JSON|Document JSON/)).toHaveValue(
      JSON.stringify({ _id: "1", name: "Ada" }, null, 2),
    );

    rerender(
      <I18nProvider>
        <MongoBrowser connectionId="mongo-b" readOnly={false} />
      </I18nProvider>,
    );

    expect(await screen.findByText("logs")).toBeInTheDocument();
    expect(screen.queryByText("app")).not.toBeInTheDocument();
    expect(screen.queryByText("users")).not.toBeInTheDocument();
    expect(screen.queryByText(/Ada/)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/文档 JSON|Document JSON/)).toHaveValue("{\n  \n}");
  });

  it("opens the initial database, collection, and document from sidebar selection", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser
          connectionId="mongo"
          readOnly={false}
          initialDatabase="app"
          initialCollection="users"
          initialDocumentId="1"
        />
      </I18nProvider>,
    );

    const editor = await screen.findByLabelText(/文档 JSON|Document JSON/);
    expect(editor).toHaveValue(JSON.stringify({ _id: "1", name: "Ada" }, null, 2));
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 100,
    });
  });

  it("updates the selected Mongo document from the JSON editor", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      if (command === "dbx_mongo_update_document") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(await screen.findByText(/Ada/));
    const editor = screen.getByLabelText(/文档 JSON|Document JSON/);
    fireEvent.change(editor, { target: { value: JSON.stringify({ _id: "1", name: "Grace" }) } });
    await userEvent.click(screen.getByRole("button", { name: /保存文档|Save document/ }));

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_update_document", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      id: "1",
      docJson: JSON.stringify({ _id: "1", name: "Grace" }),
    });
  });

  it("refreshes and selects the inserted Mongo document", async () => {
    let documents = [{ _id: "1", name: "Ada" }];
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents")
        return Promise.resolve({ documents, total: documents.length });
      if (command === "dbx_mongo_insert_document") {
        const inserted = JSON.parse((args as { docJson: string }).docJson) as {
          _id: string;
          name: string;
        };
        documents = [...documents, inserted];
        return Promise.resolve(inserted._id);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const editor = screen.getByLabelText(/文档 JSON|Document JSON/);
    fireEvent.change(editor, { target: { value: JSON.stringify({ _id: "2", name: "Grace" }) } });
    await userEvent.click(screen.getByRole("button", { name: /插入文档|Insert document/ }));

    expect(await screen.findByText(/Grace/, { selector: "pre" })).toBeInTheDocument();
    expect(editor).toHaveValue(JSON.stringify({ _id: "2", name: "Grace" }, null, 2));
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_insert_document", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      docJson: JSON.stringify({ _id: "2", name: "Grace" }),
    });
  });

  it("refreshes and keeps the Mongo document selected after saving", async () => {
    let currentDocument = { _id: "1", name: "Ada" };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents")
        return Promise.resolve({ documents: [currentDocument], total: 1 });
      if (command === "dbx_mongo_update_document") {
        currentDocument = JSON.parse(
          (args as { docJson: string }).docJson,
        ) as typeof currentDocument;
        return Promise.resolve(1);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(await screen.findByText(/Ada/));
    const editor = screen.getByLabelText(/文档 JSON|Document JSON/);
    fireEvent.change(editor, { target: { value: JSON.stringify({ _id: "1", name: "Grace" }) } });
    await userEvent.click(screen.getByRole("button", { name: /保存文档|Save document/ }));

    expect(await screen.findByText(/Grace/, { selector: "pre" })).toBeInTheDocument();
    expect(editor).toHaveValue(JSON.stringify({ _id: "1", name: "Grace" }, null, 2));
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 100,
    });
  });

  it("renders Mongo table mode as document field columns", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({
          documents: [
            { _id: "1", name: "Ada", role: "admin" },
            { _id: "2", name: "Grace", active: true },
          ],
          total: 2,
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByRole("button", { name: /Table|表格/ }));

    expect(screen.getByRole("columnheader", { name: "_id" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "active" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "name" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "role" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "Ada" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "true" })).toBeInTheDocument();
  });

  it("persists the DBX-style Mongo workspace view mode", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      return Promise.resolve(undefined);
    });

    const firstRender = render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByRole("button", { name: /Table|表格/ }));
    expect(window.localStorage.getItem("dbx-mongo-view-mode")).toBe("table");
    expect(screen.getByRole("columnheader", { name: "_id" })).toBeInTheDocument();

    firstRender.unmount();

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    expect(await screen.findByRole("columnheader", { name: "_id" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Document|文档/ }));
    expect(window.localStorage.getItem("dbx-mongo-view-mode")).toBe("document");
  });

  it("toggles DBX-style Mongo table column visibility", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({
          documents: [
            { _id: "1", name: "Ada", role: "admin" },
            { _id: "2", name: "Grace", role: "author" },
          ],
          total: 2,
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByRole("button", { name: /Table|表格/ }));
    expect(screen.getByRole("columnheader", { name: "role" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "admin" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Columns|字段显示/ }));
    await userEvent.click(screen.getByRole("checkbox", { name: "role" }));

    expect(screen.queryByRole("columnheader", { name: "role" })).not.toBeInTheDocument();
    expect(screen.queryByRole("cell", { name: "admin" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Columns .*2\/3|字段显示 .*2\/3/ }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("menuitem", { name: /Show all|显示全部/ }));

    expect(screen.getByRole("columnheader", { name: "role" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "admin" })).toBeInTheDocument();
  });

  it("filters and inverts DBX-style Mongo table column visibility options", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({
          documents: [
            { _id: "1", name: "Ada", role: "admin" },
            { _id: "2", name: "Grace", role: "author" },
          ],
          total: 2,
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByRole("button", { name: /Table|表格/ }));
    await userEvent.click(screen.getByRole("button", { name: /Columns|字段显示/ }));
    const columnSearch = screen.getByRole("textbox", { name: /Search columns|搜索字段/ });
    expect(
      screen.getByText(/At least one column stays visible|至少保留一列可见/),
    ).toBeInTheDocument();

    await userEvent.type(columnSearch, "rol");

    expect(screen.getByRole("checkbox", { name: "role" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "name" })).not.toBeInTheDocument();

    await userEvent.clear(columnSearch);
    await userEvent.type(columnSearch, "missing");

    expect(screen.getByText(/No matches|没有匹配结果/)).toBeInTheDocument();

    await userEvent.clear(columnSearch);
    await userEvent.click(screen.getByRole("menuitem", { name: /Invert|反选/ }));

    expect(screen.getByRole("columnheader", { name: "_id" })).toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "name" })).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "role" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Columns .*1\/3|字段显示 .*1\/3/ }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("menuitem", { name: /Show all|显示全部/ }));

    expect(screen.getByRole("columnheader", { name: "name" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "role" })).toBeInTheDocument();
  });

  it("hides DBX-style Mongo table columns that are null across loaded documents", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({
          documents: [
            { _id: "1", name: "Ada", nickname: null },
            { _id: "2", name: "Grace" },
          ],
          total: 2,
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByRole("button", { name: /Table|表格/ }));
    expect(screen.getByRole("columnheader", { name: "nickname" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "name" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /View options|视图选项/ }));
    await userEvent.click(screen.getByRole("checkbox", { name: /Hide null columns|隐藏空值列/ }));

    expect(screen.queryByRole("columnheader", { name: "nickname" })).not.toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "name" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("checkbox", { name: /Hide null columns|隐藏空值列/ }));

    expect(screen.getByRole("columnheader", { name: "nickname" })).toBeInTheDocument();
  });

  it("sorts Mongo table mode documents from column headers", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { sort?: string };
        if (request.sort === '{"name":-1}') {
          return Promise.resolve({ documents: [{ _id: "2", name: "Grace" }], total: 2 });
        }
        if (request.sort === '{"name":1}') {
          return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 2 });
        }
        return Promise.resolve({ documents: [{ _id: "0", name: "Zoe" }], total: 2 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    await userEvent.click(screen.getByRole("button", { name: /Table|表格/ }));
    expect(await screen.findByRole("cell", { name: "Zoe" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "name" }));

    expect(await screen.findByRole("cell", { name: "Ada" })).toBeInTheDocument();
    expect(screen.getByLabelText(/Sort JSON|排序 JSON/)).toHaveValue('{"name":1}');
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: '{"name":1}',
      skip: 0,
      limit: 100,
    });

    await userEvent.click(screen.getByRole("button", { name: "name" }));

    expect(await screen.findByRole("cell", { name: "Grace" })).toBeInTheDocument();
    expect(screen.getByLabelText(/Sort JSON|排序 JSON/)).toHaveValue('{"name":-1}');
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: '{"name":-1}',
      skip: 0,
      limit: 100,
    });
  });

  it("loads additional Mongo documents from the workspace", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { skip?: number };
        if (request.skip === 1) {
          return Promise.resolve({ documents: [{ _id: "2", name: "Grace" }], total: 2 });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 2 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: /Load more \(1\/2\)|加载更多 \(1\/2\)/ }),
    );

    expect(await screen.findByText(/Grace/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 1,
      limit: 100,
    });
  });

  it("navigates Mongo documents by page without appending previous page results", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { skip?: number };
        if (request.skip === 100) {
          return Promise.resolve({ documents: [{ _id: "101", name: "Grace" }], total: 250 });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 250 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();
    expect(screen.getByText(/Page 1 \/ 3|第 1 \/ 3 页/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Next page|下一页/ }));

    expect(await screen.findByText(/Grace/)).toBeInTheDocument();
    expect(screen.queryByText(/Ada/)).not.toBeInTheDocument();
    expect(screen.getByText(/Page 2 \/ 3|第 2 \/ 3 页/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 100,
      limit: 100,
    });

    await userEvent.click(screen.getByRole("button", { name: /Previous page|上一页/ }));
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();
    expect(screen.queryByText(/Grace/)).not.toBeInTheDocument();
  });

  it("changes Mongo document page size before paging", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { skip?: number; limit?: number };
        if (request.limit === 50 && request.skip === 50) {
          return Promise.resolve({ documents: [{ _id: "51", name: "Hopper" }], total: 120 });
        }
        if (request.limit === 50) {
          return Promise.resolve({ documents: [{ _id: "1", name: "Grace" }], total: 120 });
        }
        return Promise.resolve({ documents: [{ _id: "0", name: "Ada" }], total: 120 });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText(/Rows|行数/), "50");

    expect(await screen.findByText(/Grace/)).toBeInTheDocument();
    expect(screen.queryByText(/Ada/)).not.toBeInTheDocument();
    expect(screen.getByText(/Page 1 \/ 3|第 1 \/ 3 页/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 50,
    });

    await userEvent.click(screen.getByRole("button", { name: /Next page|下一页/ }));

    expect(await screen.findByText(/Hopper/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 50,
      limit: 50,
    });
  });

  it("applies Mongo document filters with Enter", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents")
        return Promise.resolve({ documents: [], total: 0 });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const filterInput = screen.getByLabelText(/Filter JSON|过滤 JSON/);
    fireEvent.change(filterInput, { target: { value: '{"active":true}' } });
    fireEvent.keyDown(filterInput, { key: "Enter", code: "Enter" });

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: '{"active":true}',
      sort: "{}",
      skip: 0,
      limit: 100,
    });
  });

  it("builds DBX-style Mongo structured filters with the manual filter JSON", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({
          documents: [{ _id: "1", active: true, age: 37, name: "Ada" }],
          total: 1,
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const filterInput = screen.getByLabelText(/Filter JSON|过滤 JSON/);
    fireEvent.change(filterInput, { target: { value: '{"active":true}' } });

    await userEvent.click(screen.getByRole("button", { name: /^(Filter|过滤)$/ }));
    const dialog = screen.getByRole("dialog", { name: /Filter|过滤/ });
    await userEvent.selectOptions(
      within(dialog).getByRole("combobox", { name: /Column|字段/ }),
      "name",
    );
    await userEvent.selectOptions(
      within(dialog).getByRole("combobox", { name: /Condition|条件/ }),
      "like",
    );
    await userEvent.type(within(dialog).getByRole("textbox", { name: /Value|值/ }), "ada");
    await userEvent.click(within(dialog).getByRole("button", { name: /Apply filter|应用过滤/ }));

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: '{"$and":[{"active":true},{"name":{"$regex":"ada","$options":"i"}}]}',
      sort: "{}",
      skip: 0,
      limit: 100,
    });
  });

  it("clears Mongo document filter and sort before reloading documents", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents")
        return Promise.resolve({ documents: [], total: 0 });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const filterInput = screen.getByLabelText(/Filter JSON|过滤 JSON/);
    const sortInput = screen.getByLabelText(/Sort JSON|排序 JSON/);
    fireEvent.change(filterInput, { target: { value: '{"active":true}' } });
    fireEvent.change(sortInput, { target: { value: '{"name":1}' } });
    await userEvent.click(screen.getByRole("button", { name: /Clear filter|清空过滤/ }));

    expect(filterInput).toHaveValue("{}");
    expect(sortInput).toHaveValue("{}");
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 100,
    });
  });

  it("opens a Mongo collection context menu and confirms bulk document deletion", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      if (command === "dbx_mongo_delete_documents") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    const collectionButton = (await screen.findByText("users")).closest(
      "button",
    ) as HTMLButtonElement;
    fireEvent.contextMenu(collectionButton);
    expect(screen.getByRole("menuitem", { name: /Copy name|复制名称/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Refresh|刷新/ })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("menuitem", { name: /Copy name|复制名称/ }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("users");

    fireEvent.contextMenu(collectionButton);
    await userEvent.click(screen.getByRole("menuitem", { name: /Refresh|刷新/ }));
    expect(await screen.findByText(/Ada/)).toBeInTheDocument();

    fireEvent.contextMenu(collectionButton);
    const deleteMatchingItem = screen.getByRole("menuitem", {
      name: /Delete matching documents|删除匹配文档/,
    });
    expect(deleteMatchingItem).toHaveStyle({
      background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
      borderRadius: "8px",
    });
    await userEvent.click(deleteMatchingItem);
    expect(confirm).toHaveBeenCalledWith(
      'Delete documents in "users" matching the current filter?\n\nFilter: {}',
      {
        title: "Delete matching documents",
        kind: "warning",
        okLabel: "Delete matching documents",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_delete_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filterJson: "{}",
      many: true,
    });
  });

  it("opens a Mongo document context menu for copy and single-document deletion", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      }
      if (command === "dbx_mongo_delete_documents") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const documentCard = (await screen.findByText(/Ada/)).closest("pre") as HTMLPreElement;

    fireEvent.contextMenu(documentCard);
    expect(
      screen.getByRole("menuitem", { name: /Copy document JSON|复制文档 JSON/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Refresh|刷新/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Delete document|删除文档/ })).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("menuitem", { name: /Copy document JSON|复制文档 JSON/ }),
    );
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      JSON.stringify({ _id: "1", name: "Ada" }, null, 2),
    );

    fireEvent.contextMenu(documentCard);
    const deleteDocumentItem = screen.getByRole("menuitem", { name: /Delete document|删除文档/ });
    expect(deleteDocumentItem).toHaveStyle({
      background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
      borderRadius: "8px",
    });
    await userEvent.click(deleteDocumentItem);
    expect(confirm).toHaveBeenCalledWith('Delete document "1" from "users"?', {
      title: "Delete document",
      kind: "warning",
      okLabel: "Delete document",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_delete_documents", {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      filterJson: JSON.stringify({ _id: "1" }),
      many: false,
    });
  });

  it("clears the Mongo document editor after deleting the selected document", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents")
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
      if (command === "dbx_mongo_delete_documents") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const documentCard = (await screen.findByText(/Ada/)).closest("pre") as HTMLPreElement;
    await userEvent.click(documentCard);
    const editor = screen.getByLabelText(/文档 JSON|Document JSON/);
    expect(editor).toHaveValue(JSON.stringify({ _id: "1", name: "Ada" }, null, 2));

    fireEvent.contextMenu(documentCard);
    await userEvent.click(screen.getByRole("menuitem", { name: /Delete document|删除文档/ }));

    expect(editor).toHaveValue("{\n  \n}");
  });

  it("refreshes and keeps the Mongo document selected from the document context menu", async () => {
    let currentDocument = { _id: "1", name: "Ada" };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents")
        return Promise.resolve({ documents: [currentDocument], total: 1 });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <MongoBrowser connectionId="mongo" readOnly={false} />
      </I18nProvider>,
    );

    await userEvent.click(await screen.findByText("app"));
    await userEvent.click(await screen.findByText("users"));
    const documentCard = (await screen.findByText(/Ada/)).closest("pre") as HTMLPreElement;
    await userEvent.click(documentCard);
    const editor = screen.getByLabelText(/文档 JSON|Document JSON/);
    expect(editor).toHaveValue(JSON.stringify({ _id: "1", name: "Ada" }, null, 2));

    currentDocument = { _id: "1", name: "Grace" };
    fireEvent.contextMenu(documentCard);
    await userEvent.click(screen.getByRole("menuitem", { name: /Refresh|刷新/ }));

    expect(await screen.findByText(/Grace/, { selector: "pre" })).toBeInTheDocument();
    expect(editor).toHaveValue(JSON.stringify({ _id: "1", name: "Grace" }, null, 2));
  });
});
