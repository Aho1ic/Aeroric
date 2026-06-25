import type { RightPanel } from "../hooks/useProjectPanels";

export type IdeToolAvailability = {
  filesDisabled?: boolean;
  gitDisabled?: boolean;
  problemsDisabled?: boolean;
  terminalDisabled?: boolean;
  searchDisabled?: boolean;
  debugDisabled?: boolean;
  previewDisabled?: boolean;
};

export type IdeToolAvailabilityFlag = keyof IdeToolAvailability;

export type IdeToolIcon =
  | "bug"
  | "circle-alert"
  | "flask"
  | "git-branch"
  | "globe"
  | "play"
  | "search";

export type IdeToolToolbarGroup = "primary" | "utility";

export type IdeToolMetadata = {
  id: string;
  panel: Exclude<RightPanel, null>;
  titleKey: string;
  icon: IdeToolIcon;
  commandId: string;
  commandKeywords: readonly string[];
  toolbarGroup: IdeToolToolbarGroup;
  order: number;
  disableWhen?: readonly IdeToolAvailabilityFlag[];
};

export type IdeToolWithAvailability = IdeToolMetadata & {
  disabled: boolean;
};

export const IDE_TOOL_REGISTRY = [
  {
    id: "git-advanced",
    panel: "git-advanced",
    titleKey: "gitAdvanced.title",
    icon: "git-branch",
    commandId: "git-advanced",
    commandKeywords: ["git", "blame", "stash", "conflict", "merge"],
    toolbarGroup: "primary",
    order: 10,
    disableWhen: ["gitDisabled"],
  },
  {
    id: "problems",
    panel: "problems",
    titleKey: "problems.title",
    icon: "circle-alert",
    commandId: "problems",
    commandKeywords: ["diagnostics", "errors", "warnings"],
    toolbarGroup: "primary",
    order: 20,
    disableWhen: ["problemsDisabled"],
  },
  {
    id: "tests",
    panel: "tests",
    titleKey: "tests.title",
    icon: "flask",
    commandId: "test-explorer",
    commandKeywords: ["test", "vitest", "cargo"],
    toolbarGroup: "primary",
    order: 30,
    disableWhen: ["terminalDisabled"],
  },
  {
    id: "debug",
    panel: "debug",
    titleKey: "debug.title",
    icon: "bug",
    commandId: "debug",
    commandKeywords: ["debugger", "breakpoint", "call stack", "variables"],
    toolbarGroup: "primary",
    order: 40,
    disableWhen: ["debugDisabled"],
  },
  {
    id: "run",
    panel: "run",
    titleKey: "run.title",
    icon: "play",
    commandId: "run-configurations",
    commandKeywords: ["run", "configuration", "task"],
    toolbarGroup: "primary",
    order: 50,
    disableWhen: ["terminalDisabled"],
  },
  {
    id: "preview",
    panel: "preview",
    titleKey: "preview.title",
    icon: "globe",
    commandId: "web-preview",
    commandKeywords: ["web", "preview", "port", "localhost"],
    toolbarGroup: "primary",
    order: 60,
    disableWhen: ["previewDisabled"],
  },
  {
    id: "search",
    panel: "search",
    titleKey: "toolbar.search",
    icon: "search",
    commandId: "search-files",
    commandKeywords: ["find", "text", "workspace"],
    toolbarGroup: "utility",
    order: 100,
    disableWhen: ["searchDisabled"],
  },
] as const satisfies readonly IdeToolMetadata[];

function sortIdeTools<T extends IdeToolMetadata>(tools: readonly T[]): T[] {
  return [...tools].sort(
    (a, b) => a.order - b.order || a.titleKey.localeCompare(b.titleKey) || a.id.localeCompare(b.id),
  );
}

export function isIdeToolDisabled(
  tool: IdeToolMetadata,
  availability: IdeToolAvailability,
): boolean {
  return tool.disableWhen?.some((flag) => Boolean(availability[flag])) ?? false;
}

export function getToolbarIdeTools(availability: IdeToolAvailability): IdeToolWithAvailability[] {
  return sortIdeTools(IDE_TOOL_REGISTRY).map((tool) => ({
    ...tool,
    disabled: isIdeToolDisabled(tool, availability),
  }));
}

export function getCommandPaletteIdeTools(availability: IdeToolAvailability): IdeToolMetadata[] {
  return sortIdeTools(IDE_TOOL_REGISTRY).filter((tool) => !isIdeToolDisabled(tool, availability));
}
