/**
 * The launcher writes a stable marker for this recoverable Windows failure.
 * Older generated launchers only surface PowerShell's localized error, so
 * retain those signatures while their wrapper is automatically refreshed.
 */
export function shouldOfferWindowsNodeInstaller(os: string, terminalOutput: string): boolean {
  if (os.toLowerCase() !== "windows") return false;

  const output = terminalOutput.toLowerCase();
  if (output.includes("aeroric_claude_cli_not_found")) return true;
  if (!output.includes("claude")) return false;

  return (
    output.includes("claude code cli was not found") ||
    output.includes("the term 'claude' is not recognized") ||
    output.includes("is not recognized as the name") ||
    terminalOutput.includes("无法将“claude”项识别") ||
    terminalOutput.includes('无法将"claude"项识别') ||
    terminalOutput.includes("无法将'claude'项识别")
  );
}
