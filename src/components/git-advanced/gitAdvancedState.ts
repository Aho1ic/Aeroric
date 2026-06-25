import type {
  GitBlameLine,
  GitBranchGraphResult,
  GitConflictResolution,
  GitStashEntry,
} from "../../types";

export function projectRelativeGitPath(projectPath: string, filePath: string | null): string {
  if (!filePath) return "";
  const normalizedProject = projectPath.replace(/\\/g, "/").replace(/\/+$/, "");
  const normalizedFile = filePath.replace(/\\/g, "/");
  if (!normalizedProject) return normalizedFile;
  if (normalizedFile === normalizedProject) return "";
  const prefix = `${normalizedProject}/`;
  if (normalizedFile.startsWith(prefix)) return normalizedFile.slice(prefix.length);
  if (normalizedFile.startsWith("/") || /^[A-Za-z]:\//.test(normalizedFile)) return "";
  return normalizedFile;
}

export function summarizeBlameAuthors(lines: GitBlameLine[]): Array<{ author: string; lines: number }> {
  const counts = new Map<string, number>();
  for (const line of lines) {
    const author = line.author.trim() || "Unknown";
    counts.set(author, (counts.get(author) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([author, lineCount]) => ({ author, lines: lineCount }))
    .sort((a, b) => b.lines - a.lines || a.author.localeCompare(b.author));
}

export function inlineBlameText(line: GitBlameLine): string {
  const author = line.author.trim() || "Unknown";
  const summary = line.summary.trim();
  return summary
    ? `${line.shortCommit} ${author} - ${summary}`
    : `${line.shortCommit} ${author}`;
}

export function inlineBlameTitle(line: GitBlameLine): string {
  const author = line.author.trim() || "Unknown";
  const summary = line.summary.trim();
  return summary ? `${line.commit} ${author} - ${summary}` : `${line.commit} ${author}`;
}

export function stashDisplayTitle(entry: GitStashEntry): string {
  const message = entry.message.trim();
  return message ? `${entry.name} ${message}` : entry.name;
}

export function branchGraphSummary(graph: GitBranchGraphResult): {
  totalCommits: number;
  currentBranch: string | null;
  refs: string[];
} {
  const refs: string[] = [];
  let currentBranch: string | null = null;

  const addRef = (value: string) => {
    const normalized = value.trim();
    if (normalized && !refs.includes(normalized)) {
      refs.push(normalized);
    }
  };

  for (const commit of graph.commits) {
    for (const ref of commit.refs) {
      const trimmed = ref.trim();
      if (!trimmed) continue;
      if (trimmed.startsWith("HEAD -> ")) {
        const branch = trimmed.slice("HEAD -> ".length).trim();
        if (branch) {
          currentBranch = currentBranch ?? branch;
          addRef(branch);
        }
        continue;
      }
      if (trimmed.startsWith("tag: ")) {
        addRef(trimmed.slice("tag: ".length));
        continue;
      }
      if (trimmed !== "HEAD") {
        addRef(trimmed);
      }
    }
  }

  return {
    totalCommits: graph.commits.length,
    currentBranch,
    refs,
  };
}

export function isGitConflictResolution(value: string): value is GitConflictResolution {
  return value === "ours" || value === "theirs" || value === "both";
}
