/**
 * 终端配件键定义(字节序列移植自 orca 的 TERMINAL_ACCESSORY_KEYS)。
 * 纯数据无 RN 依赖,便于 vitest 直测字节映射。
 */

export interface TerminalAccessoryKey {
  id: string;
  label: string;
  /** 按下时写入 PTY 的原始字节。 */
  bytes: string;
  /** 长按连发(方向键、退格等)。 */
  repeatable?: boolean;
}

export const TERMINAL_ACCESSORY_KEYS: readonly TerminalAccessoryKey[] = [
  { id: "escape", label: "Esc", bytes: "\x1b" },
  { id: "tab", label: "Tab", bytes: "\t" },
  { id: "enter", label: "Enter", bytes: "\r" },
  { id: "shiftTab", label: "Shift+Tab", bytes: "\x1b[Z" },
  { id: "space", label: "Space", bytes: " " },
  { id: "backspace", label: "⌫", bytes: "\x7f", repeatable: true },
  { id: "delete", label: "Del", bytes: "\x1b[3~", repeatable: true },
  { id: "arrowUp", label: "↑", bytes: "\x1b[A", repeatable: true },
  { id: "arrowDown", label: "↓", bytes: "\x1b[B", repeatable: true },
  { id: "arrowLeft", label: "←", bytes: "\x1b[D", repeatable: true },
  { id: "arrowRight", label: "→", bytes: "\x1b[C", repeatable: true },
  { id: "ctrlC", label: "Ctrl+C", bytes: "\x03" },
  { id: "ctrlD", label: "Ctrl+D", bytes: "\x04" },
  { id: "ctrlL", label: "Ctrl+L", bytes: "\x0c" },
  { id: "ctrlZ", label: "Ctrl+Z", bytes: "\x1a" },
  { id: "ctrlR", label: "Ctrl+R", bytes: "\x12" },
  { id: "ctrlA", label: "Ctrl+A", bytes: "\x01" },
  { id: "ctrlE", label: "Ctrl+E", bytes: "\x05" },
  { id: "ctrlW", label: "Ctrl+W", bytes: "\x17" },
  { id: "ctrlU", label: "Ctrl+U", bytes: "\x15" },
] as const;

/** 长按连发的首次延迟与间隔(ms)。 */
export const REPEAT_DELAY_MS = 400;
export const REPEAT_INTERVAL_MS = 60;
