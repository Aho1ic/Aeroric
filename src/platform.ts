export type AppPlatform = "windows" | "macos" | "other";
export type FontPlatform = "windows" | "macos" | "linux";

type PlatformNavigator = Pick<Navigator, "platform" | "userAgent">;

export function detectAppPlatform(
  currentNavigator: PlatformNavigator | undefined = globalThis.navigator,
): AppPlatform {
  if (!currentNavigator) {
    return "other";
  }

  const platform = currentNavigator.platform.toLowerCase();
  const userAgent = currentNavigator.userAgent.toLowerCase();

  if (platform.includes("win") || userAgent.includes("windows")) {
    return "windows";
  }

  if (platform.includes("mac") || userAgent.includes("mac os")) {
    return "macos";
  }

  return "other";
}

export const APP_PLATFORM = detectAppPlatform();
export const FONT_PLATFORM: FontPlatform =
  APP_PLATFORM === "windows" ? "windows" : APP_PLATFORM === "macos" ? "macos" : "linux";

export function getFontStorageKey(
  kind: "ui" | "mono",
  platform: FontPlatform = FONT_PLATFORM,
): string {
  return `aeroric:${platform}:${kind}FontFamily`;
}

/** 终端字号同样按平台隔离：Windows / Linux 的等宽字形度量与 macOS 不同，共用字号会导致行宽错位。 */
export function getTerminalFontSizeStorageKey(platform: FontPlatform = FONT_PLATFORM): string {
  return `aeroric:${platform}:terminalFontSize`;
}
export const ENABLE_USAGE_INSIGHTS = true;

export function isAppleWebKit(
  currentNavigator: PlatformNavigator | undefined = globalThis.navigator,
): boolean {
  return currentNavigator?.userAgent.includes("AppleWebKit") ?? false;
}

export const IS_MAC_WEBKIT = APP_PLATFORM === "macos" && isAppleWebKit();
export const IS_OTHER_WEBKIT = APP_PLATFORM === "other" && isAppleWebKit();
