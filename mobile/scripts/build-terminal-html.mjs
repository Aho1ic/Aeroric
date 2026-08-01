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
      background: "#111111",
      foreground: "#e0e0e0",
      cursor: "#e0e0e0",
      selectionBackground: "#3b82f666",
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
  // 键盘显隐由 RN 侧的工具栏按钮联动:RN 的 Keyboard 事件不覆盖 WebView 内的 focus,
  // 所以这里主动上报隐藏 input 的 focus 状态。
  ime.addEventListener("focus", function () {
    post({ type: "ime-focus", focused: true });
  });
  ime.addEventListener("blur", function () {
    post({ type: "ime-focus", focused: false });
  });
  function focusIme() {
    ime.focus({ preventScroll: true });
  }
  function blurIme() {
    ime.blur();
  }

  // ── 触摸滚动:xterm 不处理 touch,自己做整行滚动 + 惯性 ──
  // 只让 xterm 的 buffer viewport 改变位置,不再同时 transform 画布,避免文字闪烁。
  var screenEl = term.element;
  var touchY = null;
  var touchMoved = false;
  var subPx = 0;
  var velocity = 0;
  var lastMoveAt = 0;
  var inertiaId = null;

  function now() {
    return typeof performance !== "undefined" && performance.now ? performance.now() : +new Date();
  }
  function cellHeight() {
    var cell = term.rows > 0 ? screenEl.clientHeight / term.rows : 17;
    return !cell || cell < 4 ? 17 : cell;
  }
  function atBottom() {
    var buf = term.buffer.active;
    return buf.viewportY >= buf.baseY;
  }
  function atTop() {
    return term.buffer.active.viewportY <= 0;
  }
  function resetSubPx() {
    subPx = 0;
  }
  // 正数 = 内容向上走(看更新的输出)。
  function scrollByPx(px) {
    var cell = cellHeight();
    subPx += px;
    var lines = Math.trunc(subPx / cell);
    if (lines !== 0) {
      var before = term.buffer.active.viewportY;
      term.scrollLines(lines);
      subPx -= (term.buffer.active.viewportY - before) * cell;
    }
    // 到顶/到底不留残余偏移,否则边缘会露出空白条
    if ((subPx > 0 && atBottom()) || (subPx < 0 && atTop())) subPx = 0;
  }

  function stopInertia() {
    if (inertiaId !== null) {
      cancelAnimationFrame(inertiaId);
      inertiaId = null;
    }
  }
  function startInertia() {
    var prev = now();
    function step() {
      var t = now();
      var dt = t - prev;
      prev = t;
      if (dt <= 0) dt = 16;
      if (dt > 50) dt = 50;
      scrollByPx(velocity * dt);
      velocity *= Math.pow(0.95, dt / 16);
      var stuck = (velocity > 0 && atBottom()) || (velocity < 0 && atTop());
      if (stuck || Math.abs(velocity) < 0.02) {
        inertiaId = null;
        velocity = 0;
        resetSubPx();
        return;
      }
      inertiaId = requestAnimationFrame(step);
    }
    inertiaId = requestAnimationFrame(step);
  }

  screenEl.addEventListener(
    "touchstart",
    function (ev) {
      if (ev.touches.length !== 1) return;
      stopInertia();
      velocity = 0;
      touchY = ev.touches[0].clientY;
      touchMoved = false;
      lastMoveAt = now();
    },
    { passive: true }
  );
  screenEl.addEventListener(
    "touchmove",
    function (ev) {
      if (touchY === null || ev.touches.length !== 1) return;
      ev.preventDefault();
      var y = ev.touches[0].clientY;
      var delta = touchY - y;
      touchY = y;
      var t = now();
      var dt = t - lastMoveAt;
      lastMoveAt = t;
      if (dt > 0 && dt < 200) velocity = velocity * 0.6 + (delta / dt) * 0.4;
      if (Math.abs(delta) >= 1) touchMoved = true;
      scrollByPx(delta);
    },
    { passive: false }
  );
  screenEl.addEventListener("touchend", function () {
    // 轻点(未滚动)时唤起键盘
    if (touchY !== null && !touchMoved) focusIme();
    touchY = null;
    if (touchMoved && Math.abs(velocity) > 0.05 && now() - lastMoveAt < 120) {
      startInertia();
    } else {
      velocity = 0;
      resetSubPx();
    }
  });
  screenEl.addEventListener("touchcancel", function () {
    touchY = null;
    velocity = 0;
    stopInertia();
    resetSubPx();
  });

  // ── 电脑视图:保留桌面端 cols/rows,但按比例缩放到手机 WebView ──
  var rootEl = document.getElementById("root");
  var desktopMode = false;

  function rendererCellSize() {
    try {
      var cell = term._core._renderService.dimensions.css.cell;
      if (cell && cell.width > 0 && cell.height > 0) return cell;
    } catch (_) {}
    return { width: 8, height: 17 };
  }

  function clearDesktopScale() {
    desktopMode = false;
    screenEl.style.transform = "";
    screenEl.style.transformOrigin = "";
    screenEl.style.width = "";
    screenEl.style.height = "";
    rootEl.style.overflow = "hidden";
  }

  function applyDesktopScale(cols, rows, notify) {
    desktopMode = true;
    term.resize(cols, rows);
    var cell = rendererCellSize();
    var naturalWidth = Math.max(1, cols * cell.width);
    var naturalHeight = Math.max(1, rows * cell.height);
    var availableWidth = Math.max(1, document.documentElement.clientWidth);
    var availableHeight = Math.max(1, document.documentElement.clientHeight);
    var scale = Math.min(1, availableWidth / naturalWidth, availableHeight / naturalHeight);
    screenEl.style.width = naturalWidth + "px";
    screenEl.style.height = naturalHeight + "px";
    screenEl.style.transformOrigin = "top left";
    screenEl.style.transform = "scale(" + scale + ")";
    // 缩放后通常完整可见;scale=1 时保留原始尺寸的滚动兜底。
    rootEl.style.overflow = scale < 0.999 ? "hidden" : "auto";
    if (notify) post({ type: "resize-result", cols: cols, rows: rows });
  }

  function fitPhone() {
    clearDesktopScale();
    var dims = fit.proposeDimensions();
    if (dims && dims.cols > 1 && dims.rows > 1) {
      term.resize(dims.cols, dims.rows);
      post({ type: "fit-result", cols: dims.cols, rows: dims.rows });
    }
  }

  window.__aeroricTerm = {
    handle: function (msg) {
      try {
        switch (msg.type) {
          case "write": {
            // 用户上翻阅读历史时不强行拉回底部
            var follow = atBottom();
            term.write(msg.data, function () {
              if (follow || msg.scrollToBottom) {
                resetSubPx();
                term.scrollToBottom();
              }
            });
            break;
          }
          case "reset":
            resetSubPx();
            term.reset();
            break;
          case "resize":
            if (msg.cols > 1 && msg.rows > 1) {
              if (desktopMode) applyDesktopScale(msg.cols, msg.rows, false);
              else term.resize(msg.cols, msg.rows);
            }
            break;
          case "fontSize":
            term.options.fontSize = msg.size;
            break;
          case "fit": {
            fitPhone();
            break;
          }
          case "viewMode": {
            if (msg.mode === "desktop" && msg.cols > 1 && msg.rows > 1) {
              applyDesktopScale(msg.cols, msg.rows, true);
            } else {
              fitPhone();
            }
            break;
          }
          case "focus":
            focusIme();
            break;
          case "blur":
            blurIme();
            break;
          case "scrollToBottom":
            resetSubPx();
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
html, body, #root { height: 100%; margin: 0; padding: 0; background: #111111; }
/* 滚动统一走 touch → xterm buffer viewport,避免 viewport 与画布双重滚动 */
.xterm .xterm-viewport { overflow-y: hidden; }
.xterm { touch-action: none; }
.xterm .xterm-screen { transform-origin: top left; }
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
