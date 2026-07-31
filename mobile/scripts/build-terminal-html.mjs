#!/usr/bin/env node
/**
 * 生成 WebView 内嵌终端页:把 @xterm/xterm、@xterm/addon-fit 与桥接胶水
 * 内联为单个 HTML,输出成 TS 字符串模块(source={{html}} 在 iOS/Android
 * WebView 上最稳,无需文件访问权限)。
 *
 * 依赖升级后重新运行:pnpm gen:terminal-html
 */

import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const root = dirname(fileURLToPath(import.meta.url));

const xtermJs = readFileSync(require.resolve("@xterm/xterm/lib/xterm.js"), "utf8");
const xtermCss = readFileSync(require.resolve("@xterm/xterm/css/xterm.css"), "utf8");
const fitJs = readFileSync(require.resolve("@xterm/addon-fit/lib/addon-fit.js"), "utf8");

const glue = `
(function () {
  function post(obj) {
    if (window.ReactNativeWebView) {
      window.ReactNativeWebView.postMessage(JSON.stringify(obj));
    }
  }
  var term = new Terminal({
    fontSize: 13,
    fontFamily: "Menlo, Consolas, 'DejaVu Sans Mono', monospace",
    scrollback: 10000,
    cursorBlink: true,
    theme: {
      background: "#0d1117",
      foreground: "#e6edf3",
      cursor: "#e6edf3",
      selectionBackground: "#4493f866",
    },
  });
  var fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById("root"));
  term.onData(function (d) {
    post({ type: "input", data: d });
  });

  // ── IME 友好输入:独立隐藏 textarea,绕开 xterm 键盘路径 ──
  // 手机输入法(GBoard/中文拼音等)走 composition 事件,直接敲 xterm 的
  // 隐藏 textarea 常只吐出第一个字符;这里用自建输入框收字,input 事件
  // 取增量发给 PTY,composition 期间缓冲、compositionend 一次性发送。
  var ime = document.createElement("textarea");
  ime.setAttribute("autocomplete", "off");
  ime.setAttribute("autocorrect", "off");
  ime.setAttribute("autocapitalize", "none");
  ime.setAttribute("spellcheck", "false");
  ime.style.cssText =
    "position:fixed;left:0;bottom:0;width:1px;height:1px;opacity:0;" +
    "padding:0;border:0;resize:none;background:transparent;color:transparent;" +
    "caret-color:transparent;z-index:-1;";
  document.body.appendChild(ime);
  var composing = false;
  function flushIme() {
    if (ime.value) {
      post({ type: "input", data: ime.value });
      ime.value = "";
    }
  }
  ime.addEventListener("compositionstart", function () {
    composing = true;
  });
  ime.addEventListener("compositionend", function () {
    composing = false;
    flushIme();
  });
  ime.addEventListener("input", function () {
    if (!composing) flushIme();
  });
  ime.addEventListener("keydown", function (ev) {
    if (composing) return;
    if (ev.key === "Enter") {
      ev.preventDefault();
      flushIme();
      post({ type: "input", data: "\\r" });
    } else if (ev.key === "Backspace" && !ime.value) {
      ev.preventDefault();
      post({ type: "input", data: "\\u007f" });
    }
  });
  function focusIme() {
    ime.focus({ preventScroll: true });
  }

  // ── 触摸滚动:xterm 不处理 touch,自己把位移换算成 scrollLines ──
  var screenEl = term.element;
  var touchY = null;
  var touchMoved = false;
  var pxRest = 0;
  screenEl.addEventListener(
    "touchstart",
    function (ev) {
      if (ev.touches.length !== 1) return;
      touchY = ev.touches[0].clientY;
      touchMoved = false;
      pxRest = 0;
    },
    { passive: true }
  );
  screenEl.addEventListener(
    "touchmove",
    function (ev) {
      if (touchY === null || ev.touches.length !== 1) return;
      var y = ev.touches[0].clientY;
      var cell = term.rows > 0 ? screenEl.clientHeight / term.rows : 17;
      if (!cell || cell < 4) cell = 17;
      var delta = touchY - y + pxRest;
      var lines = (delta / cell) | 0;
      pxRest = delta - lines * cell;
      touchY = y;
      if (lines !== 0) {
        touchMoved = true;
        term.scrollLines(lines);
      }
      ev.preventDefault();
    },
    { passive: false }
  );
  screenEl.addEventListener("touchend", function () {
    // 轻点(未滚动)时唤起键盘
    if (touchY !== null && !touchMoved) focusIme();
    touchY = null;
  });

  function atBottom() {
    var buf = term.buffer.active;
    return buf.viewportY >= buf.baseY;
  }
  window.__aeroricTerm = {
    handle: function (msg) {
      try {
        switch (msg.type) {
          case "write": {
            // 用户上翻阅读历史时不强行拉回底部
            var follow = atBottom();
            term.write(msg.data, function () {
              if (follow) term.scrollToBottom();
            });
            break;
          }
          case "reset":
            term.reset();
            break;
          case "resize":
            if (msg.cols > 1 && msg.rows > 1) term.resize(msg.cols, msg.rows);
            break;
          case "fontSize":
            term.options.fontSize = msg.size;
            break;
          case "fit": {
            var dims = fit.proposeDimensions();
            if (dims && dims.cols > 1 && dims.rows > 1) {
              term.resize(dims.cols, dims.rows);
              post({ type: "fit-result", cols: dims.cols, rows: dims.rows });
            }
            break;
          }
          case "focus":
            focusIme();
            break;
          case "scrollToBottom":
            term.scrollToBottom();
            break;
        }
      } catch (err) {
        post({ type: "glue-error", message: String(err) });
      }
    },
  };
  post({ type: "ready" });
})();
`;

const html = `<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no" />
<style>
${xtermCss}
html, body, #root { height: 100%; margin: 0; padding: 0; background: #0d1117; }
/* 滚动统一走 touch → scrollLines,禁用 viewport 自身滚动避免双重滚动冲突 */
.xterm .xterm-viewport { overflow-y: hidden; }
.xterm { touch-action: none; }
</style>
</head>
<body>
<div id="root"></div>
<script>${xtermJs}</script>
<script>${fitJs}</script>
<script>${glue}</script>
</body>
</html>`;

const out = `// AUTO-GENERATED by scripts/build-terminal-html.mjs — do not edit by hand.
// 重新生成:pnpm gen:terminal-html
export const TERMINAL_HTML: string = ${JSON.stringify(html)};
`;

const target = join(root, "..", "src", "terminal", "terminal-html.generated.ts");
writeFileSync(target, out);
console.log(`Wrote ${target} (${(out.length / 1024).toFixed(0)} KB)`);
