/**
 * 全应用唯一的"写剪贴板"入口。优先走异步 Clipboard API，被 WebView 拒绝时
 * 退回 execCommand。notebook 面板刻意不用这里的 execCommand 回退（会抢
 * CodeMirror 焦点），那边直接用 navigator.clipboard，是既定例外。
 */
export async function writeClipboardText(text: string) {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Fall back to execCommand for WebViews that deny the async clipboard API.
    }
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.top = "0";
  textarea.style.left = "0";
  textarea.style.width = "1px";
  textarea.style.height = "1px";
  textarea.style.opacity = "0";

  document.body.appendChild(textarea);
  textarea.focus();
  textarea.select();

  try {
    if (!document.execCommand("copy")) {
      throw new Error("Copy command was rejected");
    }
  } finally {
    document.body.removeChild(textarea);
  }
}
