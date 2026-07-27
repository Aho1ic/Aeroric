import { invoke } from "@tauri-apps/api/core";
import type { MarketplaceCategory, MarketplacePage, MarketplaceSort } from "../../types";

export const MARKETPLACE_PAGE_SIZE = 12;
export const DEFAULT_MARKETPLACE_QUERY = "";
export const DEFAULT_MARKETPLACE_SORT: MarketplaceSort = "installs";
export const DEFAULT_MARKETPLACE_CATEGORY: MarketplaceCategory = "all";

const CACHE_KEY_PREFIX = "aeroric:marketplace:page:";

interface MarketplacePageRequest {
  query: string;
  sort: MarketplaceSort;
  category: MarketplaceCategory;
  page: number;
  pageSize?: number;
}

interface StoredMarketplacePage {
  fetchedAt: number;
  page: MarketplacePage;
}

const resolvedPages = new Map<string, MarketplacePage>();
const pendingPages = new Map<string, Promise<MarketplacePage>>();

function requestKey(request: MarketplacePageRequest): string {
  return [
    request.query.trim(),
    request.sort,
    request.category,
    request.page,
    request.pageSize ?? MARKETPLACE_PAGE_SIZE,
  ].join(":");
}

export function marketplacePageCacheKey(
  query: string,
  sort: MarketplaceSort,
  category: MarketplaceCategory,
  page = 0,
): string {
  return `${CACHE_KEY_PREFIX}${encodeURIComponent(query)}:${sort}:${category}:${page}`;
}

export function readCachedMarketplacePage(
  query: string,
  sort: MarketplaceSort,
  category: MarketplaceCategory,
  page = 0,
): MarketplacePage | null {
  try {
    const raw =
      localStorage.getItem(marketplacePageCacheKey(query, sort, category, page)) ??
      (page === 0
        ? localStorage.getItem(
            `${CACHE_KEY_PREFIX}${encodeURIComponent(query)}:${sort}:${category}`,
          )
        : null);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as StoredMarketplacePage | MarketplacePage;
    if (
      "fetchedAt" in parsed &&
      typeof parsed.fetchedAt === "number" &&
      typeof parsed.page === "object"
    ) {
      return parsed.page;
    }
    return parsed as MarketplacePage;
  } catch {
    return null;
  }
}

function writeCachedMarketplacePage(request: MarketplacePageRequest, page: MarketplacePage): void {
  try {
    const stored: StoredMarketplacePage = { fetchedAt: Date.now(), page };
    localStorage.setItem(
      marketplacePageCacheKey(request.query, request.sort, request.category, request.page),
      JSON.stringify(stored),
    );
  } catch {
    // The in-memory cache still avoids duplicate requests when storage is unavailable.
  }
}

export function loadMarketplacePage(request: MarketplacePageRequest): Promise<MarketplacePage> {
  const normalizedRequest = {
    ...request,
    query: request.query.trim(),
    pageSize: request.pageSize ?? MARKETPLACE_PAGE_SIZE,
  };
  const key = requestKey(normalizedRequest);
  const resolved = resolvedPages.get(key);
  if (resolved) return Promise.resolve(resolved);
  const pending = pendingPages.get(key);
  if (pending) return pending;

  const promise = invoke<MarketplacePage>("search_marketplace_skills", normalizedRequest)
    .then((result) => {
      resolvedPages.set(key, result);
      writeCachedMarketplacePage(normalizedRequest, result);
      return result;
    })
    .finally(() => {
      pendingPages.delete(key);
    });
  pendingPages.set(key, promise);
  return promise;
}

export function preloadDefaultMarketplacePage(): Promise<MarketplacePage | null> {
  return loadMarketplacePage({
    query: DEFAULT_MARKETPLACE_QUERY,
    sort: DEFAULT_MARKETPLACE_SORT,
    category: DEFAULT_MARKETPLACE_CATEGORY,
    page: 0,
  }).catch(() => null);
}

export function scheduleMarketplacePreload(): () => void {
  const run = () => {
    void preloadDefaultMarketplacePage();
  };
  if ("requestIdleCallback" in window) {
    const id = window.requestIdleCallback(run, { timeout: 1500 });
    return () => window.cancelIdleCallback(id);
  }
  const id = globalThis.setTimeout(run, 600);
  return () => globalThis.clearTimeout(id);
}

export function clearMarketplacePageMemoryCache(): void {
  resolvedPages.clear();
  pendingPages.clear();
}

export function invalidateMarketplacePageCaches(): void {
  clearMarketplacePageMemoryCache();
  try {
    const keys = Array.from({ length: localStorage.length }, (_, index) =>
      localStorage.key(index),
    ).filter((key): key is string => Boolean(key?.startsWith(CACHE_KEY_PREFIX)));
    keys.forEach((key) => localStorage.removeItem(key));
  } catch {
    // Cache invalidation is best-effort when storage is unavailable.
  }
}
