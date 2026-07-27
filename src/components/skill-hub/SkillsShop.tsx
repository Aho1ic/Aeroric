import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  CalendarDays,
  Check,
  Download,
  RefreshCw,
  Search,
  ShoppingBag,
  Sparkles,
  Star,
} from "lucide-react";
import { useI18n } from "../../i18n";
import type {
  MarketplaceCategory,
  MarketplaceInstallRecord,
  MarketplaceSkill,
  MarketplaceSort,
} from "../../types";
import { SKILL_HUB_CHANGED_EVENT } from "../app-settings/types";
import { Button } from "../ui/Button";
import {
  DEFAULT_MARKETPLACE_CATEGORY,
  DEFAULT_MARKETPLACE_QUERY,
  DEFAULT_MARKETPLACE_SORT,
  invalidateMarketplacePageCaches,
  loadMarketplacePage,
  MARKETPLACE_PAGE_SIZE,
  readCachedMarketplacePage,
} from "./marketplaceData";

const SORTS: MarketplaceSort[] = ["installs", "downloads", "stars", "updated", "published"];
const CATEGORIES: MarketplaceCategory[] = [
  "all",
  "agents",
  "integrations",
  "automation",
  "operations",
  "security",
  "research",
  "development",
  "finance",
  "lifestyle",
  "productivity",
  "other",
  "communication",
  "creative",
  "knowledge",
];

function formatCount(value: number, locale: string) {
  return new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(
    value,
  );
}

function formatDate(value: string | undefined, locale: string) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
  }).format(date);
}

function SkillAvatar({ skill }: { skill: MarketplaceSkill }) {
  const [failed, setFailed] = useState(false);
  const hue = Array.from(skill.name).reduce((sum, char) => sum + char.charCodeAt(0), 0) % 360;
  if (skill.publisherAvatar && !failed) {
    return (
      <img
        src={skill.publisherAvatar}
        alt=""
        className="marketplace-skill-avatar"
        onError={() => setFailed(true)}
      />
    );
  }
  return (
    <span
      className="marketplace-skill-avatar marketplace-skill-avatar--fallback"
      style={{ "--marketplace-avatar-hue": hue } as React.CSSProperties}
      aria-hidden="true"
    >
      {skill.name.slice(0, 2).toUpperCase()}
    </span>
  );
}

function MarketplaceSkeleton() {
  return (
    <div className="marketplace-card marketplace-card--skeleton" aria-hidden="true">
      <div className="marketplace-skeleton marketplace-skeleton--avatar" />
      <div className="marketplace-skeleton marketplace-skeleton--title" />
      <div className="marketplace-skeleton marketplace-skeleton--line" />
      <div className="marketplace-skeleton marketplace-skeleton--line marketplace-skeleton--short" />
      <div className="marketplace-skeleton marketplace-skeleton--footer" />
    </div>
  );
}

export function SkillsShop() {
  const { t, language } = useI18n();
  const locale = language === "zh" ? "zh-CN" : "en-US";
  const initialPage = useMemo(
    () =>
      readCachedMarketplacePage(
        DEFAULT_MARKETPLACE_QUERY,
        DEFAULT_MARKETPLACE_SORT,
        DEFAULT_MARKETPLACE_CATEGORY,
      ),
    [],
  );
  const [query, setQuery] = useState(DEFAULT_MARKETPLACE_QUERY);
  const [debouncedQuery, setDebouncedQuery] = useState(DEFAULT_MARKETPLACE_QUERY);
  const [sort, setSort] = useState<MarketplaceSort>(DEFAULT_MARKETPLACE_SORT);
  const [category, setCategory] = useState<MarketplaceCategory>(DEFAULT_MARKETPLACE_CATEGORY);
  const [page, setPage] = useState(0);
  const [items, setItems] = useState<MarketplaceSkill[]>(initialPage?.items ?? []);
  const [total, setTotal] = useState(initialPage?.total ?? 0);
  const [hasMore, setHasMore] = useState(initialPage?.hasMore ?? false);
  const [stale, setStale] = useState(initialPage !== null);
  const [warning, setWarning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);
  const [loading, setLoading] = useState(initialPage === null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [installing, setInstalling] = useState<Set<string>>(new Set());
  const requestIdRef = useRef(0);
  const loadingMoreRef = useRef(false);
  const loadingMoreRequestIdRef = useRef(0);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const trimmed = query.trim();
      setDebouncedQuery(trimmed.length >= 2 ? trimmed : "");
    }, 300);
    return () => window.clearTimeout(timer);
  }, [query]);

  const load = useCallback(
    async (nextPage: number, append: boolean) => {
      if (append && loadingMoreRef.current) return;
      const requestId = ++requestIdRef.current;
      if (append) {
        loadingMoreRef.current = true;
        loadingMoreRequestIdRef.current = requestId;
        setLoadingMore(true);
      } else {
        setLoading(true);
      }
      if (!append) setError(null);
      const cached = !append
        ? readCachedMarketplacePage(debouncedQuery, sort, category, nextPage)
        : null;
      if (cached) {
        setItems(cached.items);
        setTotal(cached.total);
        setHasMore(cached.hasMore);
        setStale(true);
        setWarning(null);
        setLoading(false);
      } else if (!append) {
        setItems([]);
        setTotal(0);
        setHasMore(false);
      }
      try {
        const result = await loadMarketplacePage({
          query: debouncedQuery,
          sort,
          category,
          page: nextPage,
          pageSize: MARKETPLACE_PAGE_SIZE,
        });
        if (requestId !== requestIdRef.current) return;
        setItems((current) => {
          if (!append) return result.items;
          const merged = new Map(current.map((item) => [item.id, item]));
          result.items.forEach((item) => merged.set(item.id, item));
          return Array.from(merged.values());
        });
        setTotal(result.total);
        setHasMore(result.hasMore);
        setStale(result.stale);
        setWarning(result.warning ?? null);
        setPage(nextPage);
      } catch (reason) {
        if (requestId !== requestIdRef.current) return;
        if (append) {
          setWarning(String(reason));
        } else if (cached) {
          setItems(cached.items);
          setTotal(cached.total);
          setHasMore(cached.hasMore);
          setStale(true);
          setWarning(String(reason));
        } else {
          setError(String(reason));
        }
      } finally {
        if (append && loadingMoreRequestIdRef.current === requestId) {
          loadingMoreRef.current = false;
          setLoadingMore(false);
        }
        if (requestId === requestIdRef.current) {
          setLoading(false);
        }
      }
    },
    [category, debouncedQuery, sort],
  );

  useEffect(() => {
    setPage(0);
    void load(0, false);
  }, [load]);

  const install = useCallback(
    async (skill: MarketplaceSkill, overwriteConflict = false) => {
      setInstalling((current) => new Set(current).add(skill.id));
      setInstallError(null);
      try {
        await invoke<MarketplaceInstallRecord>("install_marketplace_skill", {
          skill,
          overwriteConflict,
        });
        invalidateMarketplacePageCaches();
        setItems((current) =>
          current.map((item) =>
            item.id === skill.id ? { ...item, installStatus: "installed" } : item,
          ),
        );
        window.dispatchEvent(new CustomEvent(SKILL_HUB_CHANGED_EVENT));
      } catch (reason) {
        const message = String(reason);
        if (message.includes("MARKETPLACE_NAME_CONFLICT:") && !overwriteConflict) {
          const accepted = await confirm(t("skill.shop.conflictPrompt", { name: skill.name }), {
            title: t("skill.shop.conflictTitle"),
            kind: "warning",
            okLabel: t("skill.shop.replace"),
            cancelLabel: t("common.cancel"),
          });
          if (accepted) {
            await install(skill, true);
            return;
          }
        } else {
          setInstallError(message);
        }
      } finally {
        setInstalling((current) => {
          const next = new Set(current);
          next.delete(skill.id);
          return next;
        });
      }
    },
    [t],
  );

  const visibleCategories = useMemo(
    () => CATEGORIES.map((value) => ({ value, label: t(`skill.shop.category.${value}`) })),
    [t],
  );

  return (
    <div className="marketplace-shop">
      <div className="marketplace-toolbar">
        <label className="marketplace-search">
          <Search size={14} aria-hidden="true" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("skill.shop.search")}
            aria-label={t("skill.shop.search")}
          />
        </label>
        <label className="marketplace-select-label">
          <span>{t("skill.shop.sort")}</span>
          <select value={sort} onChange={(event) => setSort(event.target.value as MarketplaceSort)}>
            {SORTS.map((value) => (
              <option key={value} value={value}>
                {t(`skill.shop.sort.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="marketplace-select-label">
          <span>{t("skill.shop.category")}</span>
          <select
            value={category}
            onChange={(event) => setCategory(event.target.value as MarketplaceCategory)}
          >
            {visibleCategories.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      {stale || warning ? (
        <div className="marketplace-notice" role="status">
          <AlertCircle size={14} />
          <span>
            {stale ? t("skill.shop.stale") : t("skill.shop.partial")}
            {warning ? ` ${warning}` : ""}
          </span>
          {warning && warning.toLowerCase().includes("rate limit") ? (
            <span className="marketplace-notice__hint">{t("skill.shop.rateLimitHint")}</span>
          ) : null}
        </div>
      ) : null}

      {installError ? (
        <div className="marketplace-notice marketplace-notice--error" role="alert">
          <AlertCircle size={14} />
          <span>{installError}</span>
        </div>
      ) : null}

      {error ? (
        <div className="marketplace-state">
          <AlertCircle size={30} />
          <strong>{t("skill.shop.loadFailed")}</strong>
          <span>{error}</span>
          <Button size="sm" variant="outline" onClick={() => void load(0, false)}>
            <RefreshCw size={13} />
            {t("common.retry")}
          </Button>
        </div>
      ) : loading ? (
        <div className="marketplace-grid" aria-label={t("common.loading")}>
          {Array.from({ length: 6 }, (_, index) => (
            <MarketplaceSkeleton key={index} />
          ))}
        </div>
      ) : items.length === 0 ? (
        <div className="marketplace-state">
          <ShoppingBag size={34} />
          <strong>{t("skill.shop.empty")}</strong>
          <span>{t("skill.shop.emptyHint")}</span>
        </div>
      ) : (
        <>
          <div className="marketplace-summary">{t("skill.shop.results", { count: total })}</div>
          <div className="marketplace-grid">
            {items.map((skill) => {
              const busy = installing.has(skill.id);
              const installed = skill.installStatus === "installed";
              const update = skill.installStatus === "update";
              return (
                <article className="marketplace-card" key={skill.id}>
                  <header className="marketplace-card__header">
                    <SkillAvatar skill={skill} />
                    <div className="marketplace-card__identity">
                      <div className="marketplace-card__name-row">
                        <h3 title={skill.name}>{skill.name}</h3>
                        {skill.isOfficial ? (
                          <span className="marketplace-official" title={t("skill.shop.official")}>
                            <Sparkles size={11} />
                          </span>
                        ) : null}
                      </div>
                      <div className="marketplace-card__publisher">
                        {skill.publisher} · {skill.latestVersion}
                      </div>
                    </div>
                  </header>

                  <div className="marketplace-tags">
                    {skill.categories.slice(0, 3).map((value) => (
                      <span key={value}>{t(`skill.shop.category.${value}`)}</span>
                    ))}
                  </div>

                  <p className="marketplace-card__description">
                    {skill.description || t("skill.shop.noDescription")}
                  </p>

                  <dl className="marketplace-stats">
                    <div title={t("skill.shop.downloads7d")}>
                      <Download size={13} />
                      <dt>{t("skill.shop.downloads7d")}</dt>
                      <dd>{formatCount(skill.downloads7d, locale)}</dd>
                    </div>
                    <div title={t("skill.shop.totalInstalls")}>
                      <Check size={13} />
                      <dt>{t("skill.shop.totalInstalls")}</dt>
                      <dd>{formatCount(skill.totalInstalls, locale)}</dd>
                    </div>
                    <div title={t("skill.shop.stars")}>
                      <Star size={13} />
                      <dt>{t("skill.shop.stars")}</dt>
                      <dd>{formatCount(skill.stars, locale)}</dd>
                    </div>
                  </dl>

                  <div className="marketplace-dates">
                    <span title={t("skill.shop.published")}>
                      <CalendarDays size={12} />
                      {formatDate(skill.publishedAt, locale)}
                    </span>
                    <span title={t("skill.shop.updated")}>
                      <RefreshCw size={12} />
                      {formatDate(skill.updatedAt, locale)}
                    </span>
                  </div>

                  <footer className="marketplace-card__footer">
                    <span
                      className={`marketplace-status marketplace-status--${skill.installStatus}`}
                    >
                      {t(`skill.shop.status.${busy ? "installing" : skill.installStatus}`)}
                    </span>
                    <Button
                      size="sm"
                      variant={update ? "default" : "outline"}
                      disabled={busy || installed}
                      onClick={() => void install(skill)}
                    >
                      {busy ? <RefreshCw size={13} className="spin" /> : <Download size={13} />}
                      {t(
                        `skill.shop.action.${busy ? "installing" : installed ? "installed" : update ? "update" : "install"}`,
                      )}
                    </Button>
                  </footer>
                </article>
              );
            })}
          </div>
          {hasMore ? (
            <div className="marketplace-load-more">
              {loadingMore ? (
                <div className="marketplace-load-more__status" role="status">
                  <RefreshCw size={14} className="spin" />
                  <span>{t("common.loading")}</span>
                </div>
              ) : (
                <Button variant="outline" onClick={() => void load(page + 1, true)}>
                  {t("skill.shop.loadMore")}
                </Button>
              )}
            </div>
          ) : null}
        </>
      )}
    </div>
  );
}
