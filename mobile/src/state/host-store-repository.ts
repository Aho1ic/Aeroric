import { loadHostStore, saveHostStore, type HostStoreState } from "../storage/host-store";

export type HostStoreRepositoryStatus = "idle" | "loading" | "ready" | "error";

export interface HostStoreRepositorySnapshot {
  status: HostStoreRepositoryStatus;
  state: HostStoreState;
  loadError: Error | null;
}

export interface HostStorePersistence {
  load(): Promise<HostStoreState>;
  save(state: HostStoreState): Promise<void>;
}

const defaultPersistence: HostStorePersistence = {
  load: loadHostStore,
  save: saveHostStore,
};

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

/**
 * Linearizes the initial read and every later mutation.
 *
 * React components never compute a write from their render-time snapshot.
 * Each update runs against the latest successfully loaded/saved state inside
 * this queue, and a failed initial read prevents `save` from being reached.
 */
export class HostStoreRepository {
  private state: HostStoreState = { hosts: [], activeHostId: null };
  private status: HostStoreRepositoryStatus = "idle";
  private loadError: Error | null = null;
  private loadPromise: Promise<void> | null = null;
  private mutationQueue: Promise<void> = Promise.resolve();
  private readonly listeners = new Set<(snapshot: HostStoreRepositorySnapshot) => void>();

  constructor(private readonly persistence: HostStorePersistence = defaultPersistence) {}

  getSnapshot(): HostStoreRepositorySnapshot {
    return { status: this.status, state: this.state, loadError: this.loadError };
  }

  subscribe(listener: (snapshot: HostStoreRepositorySnapshot) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(): void {
    const snapshot = this.getSnapshot();
    for (const listener of this.listeners) listener(snapshot);
  }

  initialize(): Promise<void> {
    if (this.status === "ready") return Promise.resolve();
    if (this.status === "error") return Promise.reject(this.loadError);
    if (this.loadPromise) return this.loadPromise;
    return this.startLoad();
  }

  retryLoad(): Promise<void> {
    if (this.status === "ready") return Promise.resolve();
    if (this.loadPromise) return this.loadPromise;
    return this.startLoad();
  }

  waitUntilReady(): Promise<void> {
    return this.initialize();
  }

  private startLoad(): Promise<void> {
    this.status = "loading";
    this.loadError = null;
    this.emit();

    const operation = (async () => {
      try {
        const loaded = await this.persistence.load();
        this.state = loaded;
        this.status = "ready";
        this.emit();
      } catch (error) {
        this.loadError = asError(error);
        this.status = "error";
        this.emit();
        throw this.loadError;
      }
    })();
    this.loadPromise = operation;
    void operation.then(
      () => {
        if (this.loadPromise === operation) this.loadPromise = null;
      },
      () => {
        if (this.loadPromise === operation) this.loadPromise = null;
      },
    );
    return operation;
  }

  transact(update: (current: HostStoreState) => HostStoreState): Promise<void> {
    const operation = this.mutationQueue.then(async () => {
      await this.waitUntilReady();
      const current = this.state;
      const next = update(current);
      if (next === current) return;
      await this.persistence.save(next);
      this.state = next;
      this.emit();
    });
    // Keep later operations usable without changing the rejection observed by
    // the caller that owns `operation`.
    this.mutationQueue = operation.then(
      () => undefined,
      () => undefined,
    );
    return operation;
  }
}
