import type { DiagnosticItem } from "../../types";

export type DiagnosticProfileId = "typescript" | "eslint" | "cargo" | "ruff" | "mypy";

export type DiagnosticProfile = {
  id: DiagnosticProfileId;
  labelKey: string;
};

type DiagnosticFileGroup = {
  file: string;
  diagnostics: DiagnosticItem[];
};

export const diagnosticProfiles: DiagnosticProfile[] = [
  { id: "typescript", labelKey: "problems.typescript" },
  { id: "eslint", labelKey: "problems.eslint" },
  { id: "cargo", labelKey: "problems.cargo" },
  { id: "ruff", labelKey: "problems.ruff" },
  { id: "mypy", labelKey: "problems.mypy" },
];

const severityRank: Record<DiagnosticItem["severity"], number> = {
  error: 0,
  warning: 1,
  info: 2,
};

export function sortDiagnostics(diagnostics: DiagnosticItem[]): DiagnosticItem[] {
  return [...diagnostics].sort(
    (a, b) =>
      severityRank[a.severity] - severityRank[b.severity] ||
      a.file.localeCompare(b.file) ||
      a.line - b.line ||
      a.column - b.column ||
      a.message.localeCompare(b.message),
  );
}

export function groupDiagnosticsByFile(diagnostics: DiagnosticItem[]): DiagnosticFileGroup[] {
  const groups: DiagnosticFileGroup[] = [];
  const byFile = new Map<string, DiagnosticFileGroup>();
  for (const diagnostic of sortDiagnostics(diagnostics)) {
    let group = byFile.get(diagnostic.file);
    if (!group) {
      group = { file: diagnostic.file, diagnostics: [] };
      byFile.set(diagnostic.file, group);
      groups.push(group);
    }
    group.diagnostics.push(diagnostic);
  }
  return groups;
}

export function buildAgentFixPrompt(
  profile: DiagnosticProfileId | string,
  diagnostics: DiagnosticItem[],
  limit = 50,
): string {
  const sorted = sortDiagnostics(diagnostics);
  const visible = sorted.slice(0, limit);
  const omitted = Math.max(0, sorted.length - visible.length);
  const lines = visible.map((diagnostic) => {
    const code = diagnostic.code ? `/${diagnostic.code}` : "";
    return `- ${diagnostic.file}:${diagnostic.line}:${diagnostic.column} [${diagnostic.source}${code}] ${diagnostic.message}`;
  });
  if (omitted > 0) {
    lines.push(`- ... ${omitted} more diagnostics omitted`);
  }

  return [
    `Fix the current ${profile} diagnostics.`,
    "",
    "Diagnostics:",
    ...lines,
    "",
    "Please make the smallest safe code changes, keep existing behavior intact, and run the focused checks for the affected files.",
  ].join("\n");
}
