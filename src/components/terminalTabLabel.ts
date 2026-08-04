/** Keep shell tabs scannable without losing the full command name in the title. */
export function compactTerminalLabel(label: string): string {
  const normalized = label.trim();
  if (normalized.toLowerCase() === "windows powershell") return "PowerShell";
  return normalized || "Shell";
}

export function formatTerminalTabLabel(label: string, index?: number): string {
  const compact = compactTerminalLabel(label);
  return index === undefined ? compact : `${compact} ${index + 1}`;
}
