import { describe, expect, it, vi } from "vitest";

vi.mock("expo-secure-store", () => ({
  getItemAsync: vi.fn(),
  setItemAsync: vi.fn(),
}));

import type { PairedHost } from "../types";
import { addOrReplaceHost, type HostStoreState } from "../storage/host-store";
import { HostStoreRepository, type HostStorePersistence } from "./host-store-repository";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function host(id: string): PairedHost {
  return {
    id,
    name: id,
    endpoints: [`ws://127.0.0.1/${id}`],
    deviceId: `device-${id}`,
    deviceToken: `token-${id}`,
    pairedAt: 1,
  };
}

function persistence(
  load: HostStorePersistence["load"],
  save: HostStorePersistence["save"] = vi.fn(async () => undefined),
): HostStorePersistence {
  return { load, save };
}

describe("HostStoreRepository", () => {
  it("waits for the initial read before applying an early mutation", async () => {
    const pendingLoad = deferred<HostStoreState>();
    const save = vi.fn(async () => undefined);
    const repository = new HostStoreRepository(persistence(() => pendingLoad.promise, save));

    const mutation = repository.transact((current) => addOrReplaceHost(current, host("new")));
    await Promise.resolve();
    expect(save).not.toHaveBeenCalled();

    pendingLoad.resolve({ hosts: [host("existing")], activeHostId: "existing" });
    await mutation;

    expect(save).toHaveBeenCalledWith({
      hosts: [host("existing"), host("new")],
      activeHostId: "new",
    });
    expect(repository.getSnapshot().state.hosts.map(({ id }) => id)).toEqual(["existing", "new"]);
  });

  it("blocks every write when the initial read fails", async () => {
    const failure = new Error("keychain unavailable");
    const save = vi.fn(async () => undefined);
    const repository = new HostStoreRepository(
      persistence(async () => Promise.reject(failure), save),
    );

    await expect(
      repository.transact((current) => addOrReplaceHost(current, host("new"))),
    ).rejects.toBe(failure);
    expect(save).not.toHaveBeenCalled();
    expect(repository.getSnapshot()).toMatchObject({ status: "error", loadError: failure });
  });

  it("installs a new load barrier on retry and resumes later mutations", async () => {
    const failure = new Error("device locked");
    const save = vi.fn(async () => undefined);
    const load = vi
      .fn<HostStorePersistence["load"]>()
      .mockRejectedValueOnce(failure)
      .mockResolvedValueOnce({ hosts: [host("existing")], activeHostId: "existing" });
    const repository = new HostStoreRepository(persistence(load, save));

    await expect(repository.initialize()).rejects.toBe(failure);
    await repository.retryLoad();
    await repository.transact((current) => addOrReplaceHost(current, host("new")));

    expect(load).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenCalledWith({
      hosts: [host("existing"), host("new")],
      activeHostId: "new",
    });
    expect(repository.getSnapshot().status).toBe("ready");
  });

  it("publishes a mutation only after the durable write succeeds", async () => {
    const saveFailure = new Error("secure storage full");
    const repository = new HostStoreRepository(
      persistence(
        async () => ({ hosts: [host("existing")], activeHostId: "existing" }),
        async () => Promise.reject(saveFailure),
      ),
    );
    await repository.initialize();

    await expect(
      repository.transact((current) => addOrReplaceHost(current, host("new"))),
    ).rejects.toBe(saveFailure);
    expect(repository.getSnapshot().state).toEqual({
      hosts: [host("existing")],
      activeHostId: "existing",
    });
  });
});
