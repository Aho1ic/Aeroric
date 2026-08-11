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

  // ── 字形降级 ──
  // iOS 等宽字体(Menlo)缺 U+23FA(⏺)等码位,WKWebView 会沿字体栈回退到
  // Apple Color Emoji,终端里就出现彩色图标。系统中除 emoji 字体外无任何字体
  // 覆盖这些码位,font-family 兜底无效,只能在写入 xterm 前替换成等宽单色字形。
  // 替换表与 src/terminal/glyph-fallback.ts 保持一致(该文件承载单测)。
  var GLYPH_FALLBACK = {
    0x231b: "○",
    0x23f3: "○",
    0x23f8: "║",
    0x23fa: "●",
    0x2705: "✓",
    0x274c: "✗",
    0x2b50: "★",
    0x1f4a1: "‼",
  };
  var GLYPH_FALLBACK_RE = /[\\u231b\\u23f3\\u23f8\\u23fa\\u2705\\u274c\\u2b50\\u{1f4a1}]/gu;
  function remapGlyphs(text) {
    if (!text) return text;
    GLYPH_FALLBACK_RE.lastIndex = 0;
    if (!GLYPH_FALLBACK_RE.test(text)) return text;
    return text.replace(GLYPH_FALLBACK_RE, function (ch) {
      var mapped = GLYPH_FALLBACK[ch.codePointAt(0)];
      return mapped === undefined ? ch : mapped;
    });
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
    "position:fixed;left:2px;bottom:2px;width:2px;height:2px;opacity:0.01;" +
    "padding:0;border:0;resize:none;background:transparent;color:transparent;" +
    "caret-color:transparent;z-index:20;pointer-events:none;";
  document.body.appendChild(ime);
  var composing = false;
  var suppressNextInput = false;
  function flushIme() {
    if (ime.value) {
      post({ type: "input", data: ime.value });
      ime.value = "";
    }
  }
  ime.addEventListener("compositionstart", function () {
    composing = true;
    suppressNextInput = false;
  });
  ime.addEventListener("compositionend", function () {
    composing = false;
    flushIme();
  });
  // iOS/Android 的英文、数字、符号输入有时只触发 beforeinput/input,
  // 不会进入 composition。beforeinput 直接发送可见字符,避免隐藏 textarea
  // 的零尺寸/负 z-index 让非中文输入丢失。
  ime.addEventListener("beforeinput", function (ev) {
    if (composing) return;
    if (ev.inputType === "insertText" && ev.data) {
      ev.preventDefault();
      post({ type: "input", data: ev.data });
      ime.value = "";
      suppressNextInput = true;
    } else if (ev.inputType === "deleteContentBackward" && !ime.value) {
      ev.preventDefault();
      post({ type: "input", data: "\u007f" });
    }
  });
  ime.addEventListener("input", function () {
    if (suppressNextInput) {
      suppressNextInput = false;
      ime.value = "";
      return;
    }
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
    term.blur();
    ime.value = "";
    ime.focus({ preventScroll: true });
  }
  function blurIme() {
    ime.blur();
  }

  // ── 触摸滚动:xterm 不处理 touch,自己做整行滚动 + 惯性 ──
  // 只让 xterm 的 buffer viewport 改变位置,不再同时 transform 画布,避免文字闪烁。
  var screenEl = term.element;
  var touchY = null;
  var touchX = null;
  var touchMoved = false;
  var subPx = 0;
  var velocity = 0;
  var lastMoveAt = 0;
  var inertiaId = null;
  // 待消费的手指位移:touchmove 只累加,真正的 scrollLines 留到 rAF 里做。
  // iOS ProMotion 的 touchmove 可达 120Hz,逐事件同步渲染会让 DOM renderer
  // 持续掉帧(表现为滑动发涩、断续)。
  var pendingScrollPx = 0;
  var scrollFrameId = null;

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

  // 把累积的手指位移合并成一次滚动,每帧最多一次。
  function flushScroll() {
    scrollFrameId = null;
    if (pendingScrollPx === 0) return;
    var px = pendingScrollPx;
    pendingScrollPx = 0;
    scrollByPx(px);
  }
  function queueScroll(px) {
    pendingScrollPx += px;
    if (scrollFrameId === null) {
      scrollFrameId = requestAnimationFrame(flushScroll);
    }
  }
  function cancelQueuedScroll() {
    if (scrollFrameId !== null) {
      cancelAnimationFrame(scrollFrameId);
      scrollFrameId = null;
    }
    pendingScrollPx = 0;
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

  // ── 长按选择文字 ──
  // xterm 的鼠标选区不响应 touch。这里长按 LONG_PRESS_MS 进入选择模式:
  // 以按下位置所在单词为初始选区,拖动调整,松手后保留选区并由 RN 侧显示
  // 「复制」按钮。选区坐标全部用 cell 网格换算,兼容电脑视图的 transform 缩放。
  var LONG_PRESS_MS = 420;
  var LONG_PRESS_SLOP_PX = 10;
  var longPressTimer = null;
  var selecting = false;
  var anchor = null;

  function cancelLongPress() {
    if (longPressTimer !== null) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  /** 屏幕坐标 → 终端 cell 坐标(考虑电脑视图的 scale)。 */
  function pointToCell(clientX, clientY) {
    var rect = screenEl.getBoundingClientRect();
    var cell = rendererCellSize();
    // rect 已含 transform 缩放,用它反推实际缩放比
    var scaleX = screenEl.offsetWidth > 0 ? rect.width / screenEl.offsetWidth : 1;
    var scaleY = screenEl.offsetHeight > 0 ? rect.height / screenEl.offsetHeight : 1;
    if (!scaleX || !isFinite(scaleX)) scaleX = 1;
    if (!scaleY || !isFinite(scaleY)) scaleY = 1;
    var col = Math.floor((clientX - rect.left) / (cell.width * scaleX));
    var row = Math.floor((clientY - rect.top) / (cell.height * scaleY));
    return {
      col: Math.max(0, Math.min(term.cols - 1, col)),
      row: Math.max(0, Math.min(term.rows - 1, row)),
    };
  }

  function emitSelectionState() {
    post({ type: "selection", hasSelection: term.hasSelection() });
  }

  function clearSelectionAndNotify() {
    term.clearSelection();
    anchor = null;
    emitSelectionState();
  }

  /**
   * 以 (col,row) 所在的「单词」作为初始选区,空白处则退化为整行。
   * 逐 cell 取字符而不是 translateToString 后按下标取:CJK 宽字符占 2 列但只算
   * 1 个 JS 字符,按字符串下标找边界会在含中文的行上错位。
   */
  function charAtColumn(line, col) {
    var cell = line.getCell(col);
    if (!cell) return "";
    var chars = cell.getChars();
    // 宽字符的后半格 width=0、chars 为空,视作与前一格同属一个字符
    if (chars === "" && cell.getWidth() === 0) return "\\u0000";
    return chars;
  }

  function selectWordAt(cell) {
    var absRow = term.buffer.active.viewportY + cell.row;
    var line = term.buffer.active.getLine(absRow);
    if (!line) return;
    var isWordChar = function (c) {
      return c !== "" && c !== " " && !/^\\s$/.test(c);
    };
    if (!isWordChar(charAtColumn(line, cell.col))) {
      term.selectLines(absRow, absRow);
      return;
    }
    var start = cell.col;
    var end = cell.col;
    while (start > 0 && isWordChar(charAtColumn(line, start - 1))) start--;
    while (end < term.cols - 1 && isWordChar(charAtColumn(line, end + 1))) end++;
    term.select(start, absRow, end - start + 1);
  }

  /** 从 anchor 拖到当前点,按缓冲区绝对行号计算选区。 */
  function updateSelectionTo(clientX, clientY) {
    if (!anchor) return;
    var cell = pointToCell(clientX, clientY);
    var absRow = term.buffer.active.viewportY + cell.row;
    var from = anchor;
    var to = { row: absRow, col: cell.col };
    // 保证 from 在 to 之前
    if (to.row < from.row || (to.row === from.row && to.col < from.col)) {
      var tmp = from;
      from = to;
      to = tmp;
    }
    if (from.row === to.row) {
      term.select(from.col, from.row, Math.max(1, to.col - from.col + 1));
      return;
    }
    // 跨行:xterm 没有直接的 (start,end) API,用 selectLines 取整行范围,
    // 保证复制内容完整(手机上按行复制比精确到列更实用)。
    term.selectLines(from.row, to.row);
  }

  function scheduleLongPress(clientX, clientY) {
    cancelLongPress();
    var startX = clientX;
    var startY = clientY;
    longPressTimer = setTimeout(function () {
      longPressTimer = null;
      if (touchY === null) return;
      // 手指已明显移动 → 视为滚动,不进入选择
      if (
        Math.abs(touchY - startY) > LONG_PRESS_SLOP_PX ||
        (touchX !== null && Math.abs(touchX - startX) > LONG_PRESS_SLOP_PX)
      ) {
        return;
      }
      selecting = true;
      stopInertia();
      cancelQueuedScroll();
      velocity = 0;
      blurIme();
      var cell = pointToCell(startX, startY);
      anchor = { row: term.buffer.active.viewportY + cell.row, col: cell.col };
      selectWordAt(cell);
      emitSelectionState();
      post({ type: "haptic" });
    }, LONG_PRESS_MS);
  }

  screenEl.addEventListener(
    "touchstart",
    function (ev) {
      if (ev.touches.length !== 1) return;
      stopInertia();
      cancelQueuedScroll();
      velocity = 0;
      touchY = ev.touches[0].clientY;
      touchX = ev.touches[0].clientX;
      touchMoved = false;
      lastMoveAt = now();
      scheduleLongPress(ev.touches[0].clientX, ev.touches[0].clientY);
    },
    { passive: true }
  );
  screenEl.addEventListener(
    "touchmove",
    function (ev) {
      if (touchY === null || ev.touches.length !== 1) return;
      var y = ev.touches[0].clientY;
      var x = ev.touches[0].clientX;
      // 选择模式下手指拖动 = 调整选区,不滚动
      if (selecting) {
        ev.preventDefault();
        touchY = y;
        touchX = x;
        updateSelectionTo(x, y);
        return;
      }
      ev.preventDefault();
      var delta = touchY - y;
      touchY = y;
      touchX = x;
      var t = now();
      var dt = t - lastMoveAt;
      lastMoveAt = t;
      if (dt > 0 && dt < 200) velocity = velocity * 0.6 + (delta / dt) * 0.4;
      if (Math.abs(delta) >= 1) {
        touchMoved = true;
        cancelLongPress();
      }
      queueScroll(delta);
    },
    { passive: false }
  );
  screenEl.addEventListener("touchend", function () {
    cancelLongPress();
    if (selecting) {
      // 松手结束选区调整,选区与工具条保留,等用户点「复制」
      selecting = false;
      touchY = null;
      touchX = null;
      emitSelectionState();
      return;
    }
    // 轻点(未滚动):有选区时先取消选区,否则唤起键盘
    if (touchY !== null && !touchMoved) {
      if (term.hasSelection()) {
        clearSelectionAndNotify();
      } else {
        focusIme();
      }
    }
    touchY = null;
    touchX = null;
    if (touchMoved && Math.abs(velocity) > 0.05 && now() - lastMoveAt < 120) {
      startInertia();
    } else {
      velocity = 0;
      cancelQueuedScroll();
      resetSubPx();
    }
  });
  screenEl.addEventListener("touchcancel", function () {
    cancelLongPress();
    selecting = false;
    touchY = null;
    touchX = null;
    velocity = 0;
    stopInertia();
    cancelQueuedScroll();
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
            term.write(remapGlyphs(msg.data), function () {
              if (follow || msg.scrollToBottom) {
                resetSubPx();
                term.scrollToBottom();
              }
              if (typeof msg.writeId === "number" && msg.writeId > 0) {
                post({ type: "write-complete", writeId: msg.writeId });
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
          case "copySelection":
            post({ type: "selection-text", text: term.getSelection() });
            break;
          case "clearSelection":
            clearSelectionAndNotify();
            break;
          case "selectAll":
            term.selectAll();
            emitSelectionState();
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
/* 长按选区由脚本接管:屏蔽 iOS 原生长按放大镜/共享菜单,避免与自定义选区冲突。
   选区高亮走 xterm 自己的 selection 图层,不依赖原生 user-select。 */
.xterm, #root {
  -webkit-touch-callout: none;
  -webkit-user-select: none;
  user-select: none;
}
/* 提示层由 GPU 合成,避免选区变化触发整屏重排 */
.xterm .xterm-selection div { will-change: transform; }
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
