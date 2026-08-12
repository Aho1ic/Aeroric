import { createScopedStore } from "../../state/createScopedStore";
import {
  createDatabaseWorkspaceStore,
  type DatabaseWorkspaceState,
} from "./databaseWorkspaceStore";

const scoped = createScopedStore<DatabaseWorkspaceState>(
  "DatabaseWorkspace",
  createDatabaseWorkspaceStore,
);

export const DatabaseWorkspaceProvider = scoped.Provider;
export const useDatabaseWorkspaceStore = scoped.useScopedStore;
