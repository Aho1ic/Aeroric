import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { MongoBrowser } from "../components/database/MongoBrowser";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("MongoBrowser", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
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
});
