import { invoke } from "@tauri-apps/api/core";

let cachedFonts: string[] | null = null;

export async function loadSystemFonts(): Promise<string[]> {
  if (cachedFonts) return cachedFonts;

  try {
    const fonts = await invoke<string[]>("get_system_fonts");
    cachedFonts = fonts;
    return fonts;
  } catch {
    return [];
  }
}

export function parseFirstFontName(stack: string): string {
  const trimmed = stack.trim();
  if (!trimmed) return "";

  // Handle comma-separated stack: take first entry
  const first = trimmed.split(",")[0].trim();

  // Strip surrounding quotes
  if (
    (first.startsWith('"') && first.endsWith('"')) ||
    (first.startsWith("'") && first.endsWith("'"))
  ) {
    return first.slice(1, -1);
  }
  return first;
}

export function filterFonts(fonts: string[], query: string): string[] {
  if (!query) return fonts;
  const q = query.toLowerCase();

  const exact: string[] = [];
  const startsWith: string[] = [];
  const contains: string[] = [];

  for (const f of fonts) {
    const lower = f.toLowerCase();
    if (lower === q) exact.push(f);
    else if (lower.startsWith(q)) startsWith.push(f);
    else if (lower.includes(q)) contains.push(f);
  }

  return [...exact, ...startsWith, ...contains];
}

function splitFontStack(stack: string): string[] {
  return stack
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function quoteFontFamily(family: string): string {
  if (/^["'].*["']$/.test(family)) return family;
  // 通用族与单标识符名（Consolas、monospace）不需要引号，含空格的族名必须加引号。
  return /\s/.test(family) ? `"${family}"` : family;
}

/**
 * 把用户选择的字体与所在平台的默认字体链拼成一个完整栈。
 *
 * FontSelector 只写入单个族名（例如 Windows 上选 `Consolas`），若直接使用就会丢掉
 * CJK 与通用族回退，在 Windows / Linux 上出现豆腐块和终端宽度错位。这里保持用户
 * 选择在最前，其余按平台默认链补齐并去重。
 */
export function composeFontStack(value: string, platformFallback: string): string {
  const seen = new Set<string>();
  const parts: string[] = [];
  for (const family of [...splitFontStack(value), ...splitFontStack(platformFallback)]) {
    const quoted = quoteFontFamily(family);
    const key = quoted.replace(/^["']|["']$/g, "").toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    parts.push(quoted);
  }
  return parts.join(", ");
}
