import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SkillsShop } from "../components/skill-hub/SkillsShop";
import { SKILL_HUB_CHANGED_EVENT } from "../components/app-settings/types";
import {
  clearMarketplacePageMemoryCache,
  marketplacePageCacheKey,
  preloadDefaultMarketplacePage,
} from "../components/skill-hub/marketplaceData";
import { I18nProvider } from "../i18n";
import type { MarketplacePage, MarketplaceSkill } from "../types";

const { invokeMock, confirmMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  confirmMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: confirmMock,
}));

const skill: MarketplaceSkill = {
  id: "example/skills/review-code",
  source: "example/skills",
  skillId: "review-code",
  name: "review-code",
  publisher: "example",
  latestVersion: "1.2.0",
  latestRef: "abc12345",
  categories: ["development"],
  description: "Reviews source code and suggests focused improvements.",
  downloads7d: 1200,
  totalInstalls: 3400,
  stars: 520,
  publishedAt: "2026-06-01T00:00:00Z",
  updatedAt: "2026-07-20T00:00:00Z",
  installStatus: "available",
  isOfficial: false,
};

function page(items = [skill]): MarketplacePage {
  return {
    items,
    total: items.length,
    page: 0,
    pageSize: 12,
    hasMore: false,
    stale: false,
  };
}

describe("SkillsShop", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("aeroric:language", "en");
    clearMarketplacePageMemoryCache();
    invokeMock.mockReset();
    confirmMock.mockReset();
    invokeMock.mockImplementation((command: string) => {
      if (command === "search_marketplace_skills") return Promise.resolve(page());
      if (command === "install_marketplace_skill") {
        return Promise.resolve({
          source: skill.source,
          skillId: skill.skillId,
          skillName: skill.name,
          version: skill.latestVersion,
          gitRef: skill.latestRef,
          installedAt: Date.now(),
          targetPath: `/tmp/${skill.name}`,
        });
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
  });

  it("loads cards and sends sorting, category and debounced search parameters", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider>
        <SkillsShop />
      </I18nProvider>,
    );

    expect(await screen.findByRole("heading", { name: "review-code" })).toBeInTheDocument();
    expect(screen.getByText("1.2K")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("search_marketplace_skills", {
      query: "",
      sort: "installs",
      category: "all",
      page: 0,
      pageSize: 12,
    });

    await user.selectOptions(screen.getByLabelText("Sort"), "stars");
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "search_marketplace_skills",
        expect.objectContaining({ sort: "stars" }),
      ),
    );

    await user.selectOptions(screen.getByLabelText("Category"), "development");
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "search_marketplace_skills",
        expect.objectContaining({ category: "development" }),
      ),
    );

    await user.type(screen.getByRole("textbox", { name: "Search online skills..." }), "react");
    await waitFor(
      () =>
        expect(invokeMock).toHaveBeenCalledWith(
          "search_marketplace_skills",
          expect.objectContaining({ query: "react" }),
        ),
      { timeout: 1200 },
    );
  });

  it("shows the persisted first page while refreshing it in the background", async () => {
    let resolveRefresh: (value: MarketplacePage) => void = () => {};
    invokeMock.mockImplementation(
      (command: string) =>
        new Promise<MarketplacePage>((resolve, reject) => {
          if (command === "search_marketplace_skills") {
            resolveRefresh = resolve;
          } else {
            reject(new Error(`Unexpected command: ${command}`));
          }
        }),
    );
    localStorage.setItem(
      marketplacePageCacheKey("", "installs", "all"),
      JSON.stringify({ fetchedAt: Date.now(), page: page() }),
    );

    render(
      <I18nProvider>
        <SkillsShop />
      </I18nProvider>,
    );

    expect(screen.getByRole("heading", { name: "review-code" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Showing the latest cached marketplace data",
    );

    resolveRefresh(page());
    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
  });

  it("deduplicates startup preload and the first shop request", async () => {
    let resolveRequest: (value: MarketplacePage) => void = () => {};
    invokeMock.mockImplementation(
      (command: string) =>
        new Promise<MarketplacePage>((resolve, reject) => {
          if (command === "search_marketplace_skills") {
            resolveRequest = resolve;
          } else {
            reject(new Error(`Unexpected command: ${command}`));
          }
        }),
    );

    const preload = preloadDefaultMarketplacePage();
    render(
      <I18nProvider>
        <SkillsShop />
      </I18nProvider>,
    );

    expect(invokeMock).toHaveBeenCalledTimes(1);
    resolveRequest(page());
    await preload;
    expect(await screen.findByRole("heading", { name: "review-code" })).toBeInTheDocument();
  });

  it("hides load more and ignores repeated clicks while the next page is loading", async () => {
    const nextSkill = { ...skill, id: "example/skills/test-code", name: "test-code" };
    let resolveNextPage: (value: MarketplacePage) => void = () => {};
    invokeMock.mockImplementation((command: string, args?: { page?: number }) => {
      if (command !== "search_marketplace_skills") {
        return Promise.reject(new Error(`Unexpected command: ${command}`));
      }
      if (args?.page === 1) {
        return new Promise<MarketplacePage>((resolve) => {
          resolveNextPage = resolve;
        });
      }
      return Promise.resolve({ ...page(), total: 2, hasMore: true });
    });

    render(
      <I18nProvider>
        <SkillsShop />
      </I18nProvider>,
    );

    const loadMore = await screen.findByRole("button", { name: "Load more" });
    fireEvent.click(loadMore);
    fireEvent.click(loadMore);

    expect(
      invokeMock.mock.calls.filter(
        ([command, args]) =>
          command === "search_marketplace_skills" && (args as { page?: number })?.page === 1,
      ),
    ).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading...");

    resolveNextPage({
      items: [nextSkill],
      total: 2,
      page: 1,
      pageSize: 12,
      hasMore: false,
      stale: false,
    });
    expect(await screen.findByRole("heading", { name: "test-code" })).toBeInTheDocument();
  });

  it("installs into the hub and emits the existing refresh event", async () => {
    const user = userEvent.setup();
    const changed = vi.fn();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, changed);
    render(
      <I18nProvider>
        <SkillsShop />
      </I18nProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Install" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("install_marketplace_skill", {
        skill,
        overwriteConflict: false,
      }),
    );
    expect(changed).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Installed" })).toBeDisabled();
    window.removeEventListener(SKILL_HUB_CHANGED_EVENT, changed);
  });

  it("keeps the loaded cards visible when installation fails", async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation((command: string) => {
      if (command === "search_marketplace_skills") return Promise.resolve(page());
      if (command === "install_marketplace_skill") {
        return Promise.reject("Skill Hub is not configured");
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <SkillsShop />
      </I18nProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Install" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Skill Hub is not configured");
    expect(screen.getByRole("heading", { name: "review-code" })).toBeInTheDocument();
  });
});
