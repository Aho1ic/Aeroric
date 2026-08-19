/**
 * 终端面板:WebView 内嵌 xterm + 触屏工具条。
 * 从 M2 的 app/terminal/[taskId] 页面抽出,供任务详情页「终端」tab 复用;
 * `active=false`(切到会话 tab)时退订终端流,切回时重订阅走快照恢复。
 * 键盘避让、终端重排与工具栏在本面板内统一处理。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { ScrollView, StyleSheet, Text, useWindowDimensions, Vibration, View } from "react-native";
import { WebView, type WebViewMessageEvent } from "react-native-webview";
import * as Clipboard from "expo-clipboard";
import { ChevronsDownUp, ClipboardPaste, Copy, Monitor, Smartphone, X } from "lucide-react-native";
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
import { AnimatedPressable } from "../ui/AnimatedPressable";
import { useKeyboardInset } from "../ui/use-keyboard-inset";
import {
  REPEAT_DELAY_MS,
  REPEAT_INTERVAL_MS,
  TERMINAL_ACCESSORY_KEYS,
  type TerminalAccessoryKey,
} from "./accessory-keys";
import {
  decodeTerminalFrame,
  encodeTerminalFrame,
  jsonPayload,
  parseJsonPayload,
  payloadText,
  TerminalOpcode,
  textPayload,
} from "./terminal-frames";
import { TERMINAL_HTML } from "./terminal-html.generated";
import { radii, spacing, theme, typography } from "../ui/theme";

// streamId 连接内唯一即可;模块级递增避免屏幕快速开关时撞号
let nextStreamId = 1;

/** 视作「打字回显」的片段上限:超过这个长度按批量输出处理,走 rAF 合并。 */
const ECHO_FLUSH_MAX_BYTES = 64;
/** 距上次写入超过这个间隔,说明输出已空闲,下一个小片段立即写入。 */
const ECHO_IDLE_GAP_MS = 40;

interface SnapshotMeta {
  cols?: number | null;
  rows?: number | null;
}

export function TerminalPane({ taskId, active }: { taskId: string; active: boolean }) {
  const { status, sendBinary, onBinary } = useConnection();
  const webviewRef = useRef<WebView>(null);
  const streamIdRef = useRef(0);
  const [webReady, setWebReady] = useState(false);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [, setFontSize] = useState(13);
  const [imeFocused, setImeFocused] = useState(false);
  const [hasSelection, setHasSelection] = useState(false);
  const [copied, setCopied] = useState(false);
  const [viewMode, setViewMode] = useState<"phone" | "desktop">("phone");
  const { width: windowWidth, height: windowHeight } = useWindowDimensions();
  /**
   * 输入法高度:WebView 内的隐藏 input 不参与 RN 布局,KeyboardAvoidingView
   * 抬不动它,键盘弹出时会盖住终端底部(也就是光标所在行)。用实测键盘高度
   * 给终端区留出等高的底部内边距,把光标行顶到键盘上方。
   */
  const keyboardInset = useKeyboardInset();
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** 入站帧回调注册后不再重建,靠 ref 读取最新视图模式。 */
  const viewModeRef = useRef<"phone" | "desktop">("phone");
  const desktopSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const wrapHeightRef = useRef(0);
  const refitFrameRef = useRef<number | null>(null);
  const pendingWritesRef = useRef("");
  const writeFrameRef = useRef<number | null>(null);
  const lastFlushAtRef = useRef(0);
  const snapshotInProgressRef = useRef(false);
  const trackedWriteSequenceRef = useRef(0);
  const acknowledgedWriteSequenceRef = useRef(0);
  const snapshotLayoutBarrierRef = useRef<number | null>(null);
  const snapshotLayoutPendingRef = useRef(false);
  const repeatTimersRef = useRef<{
    timeout: ReturnType<typeof setTimeout> | null;
    interval: ReturnType<typeof setInterval> | null;
  }>({ timeout: null, interval: null });

  const injectTerm = useCallback((msg: Record<string, unknown>) => {
    webviewRef.current?.injectJavaScript(
      `window.__aeroricTerm && window.__aeroricTerm.handle(${JSON.stringify(msg)}); true;`,
    );
  }, []);

  const completeSnapshotLayout = useCallback(() => {
    snapshotLayoutPendingRef.current = false;
    snapshotLayoutBarrierRef.current = null;
    const desktop = desktopSizeRef.current;
    if (viewModeRef.current === "desktop" && desktop) {
      injectTerm({ type: "snapshotEnd", mode: "desktop", ...desktop });
    } else {
      injectTerm({ type: "snapshotEnd", mode: "phone" });
    }
  }, [injectTerm]);

  const requestSnapshotFinalLayout = useCallback(() => {
    const barrier = trackedWriteSequenceRef.current;
    snapshotLayoutBarrierRef.current = barrier;
    if (acknowledgedWriteSequenceRef.current >= barrier) {
      completeSnapshotLayout();
    } else {
      snapshotLayoutPendingRef.current = true;
    }
  }, [completeSnapshotLayout]);

  const handleWriteComplete = useCallback(
    (writeId: number) => {
      acknowledgedWriteSequenceRef.current = Math.max(
        acknowledgedWriteSequenceRef.current,
        writeId,
      );
      const barrier = snapshotLayoutBarrierRef.current;
      if (
        snapshotLayoutPendingRef.current &&
        barrier !== null &&
        acknowledgedWriteSequenceRef.current >= barrier
      ) {
        completeSnapshotLayout();
      }
    },
    [completeSnapshotLayout],
  );

  const flushTermWrites = useCallback(
    (scrollToBottom = false, trackCompletion = snapshotInProgressRef.current) => {
      if (writeFrameRef.current !== null) {
        cancelAnimationFrame(writeFrameRef.current);
        writeFrameRef.current = null;
      }
      const data = pendingWritesRef.current;
      if (!data) return false;
      pendingWritesRef.current = "";
      lastFlushAtRef.current = Date.now();
      const message: Record<string, unknown> = { type: "write", data, scrollToBottom };
      if (trackCompletion) {
        message.writeId = ++trackedWriteSequenceRef.current;
      }
      injectTerm(message);
      return true;
    },
    [injectTerm],
  );

  /**
   * 写入节流策略:
   * - 高速输出(连续帧)按 rAF 合并,避免每帧多次 injectJavaScript 把渲染打满。
   * - 空闲后的第一个小片段立即写入。远程链路(WireGuard 等)RTT 本就有几十毫秒,
   *   再等一个 rAF(最多 ~16ms)会让回显明显发钝;而这种小片段正是打字回显。
   */
  const queueTermWrite = useCallback(
    (data: string) => {
      pendingWritesRef.current += data;
      if (writeFrameRef.current !== null) return;
      const idleGap = Date.now() - lastFlushAtRef.current;
      if (pendingWritesRef.current.length <= ECHO_FLUSH_MAX_BYTES && idleGap >= ECHO_IDLE_GAP_MS) {
        flushTermWrites();
        return;
      }
      writeFrameRef.current = requestAnimationFrame(() => {
        writeFrameRef.current = null;
        flushTermWrites();
      });
    },
    [flushTermWrites],
  );

  const scheduleRelayout = useCallback(() => {
    if (!active || !webReady) return;
    if (refitFrameRef.current !== null) return;
    refitFrameRef.current = requestAnimationFrame(() => {
      refitFrameRef.current = null;
      const desktop = desktopSizeRef.current;
      if (viewModeRef.current === "desktop" && desktop) {
        injectTerm({ type: "viewMode", mode: "desktop", ...desktop });
      } else {
        injectTerm({ type: "fit" });
      }
    });
  }, [active, injectTerm, webReady]);

  const sendInput = useCallback(
    (data: string) => {
      if (!streamIdRef.current) return;
      sendBinary(
        encodeTerminalFrame({
          opcode: TerminalOpcode.Input,
          streamId: streamIdRef.current,
          seq: 0,
          payload: textPayload(data),
        }),
      );
    },
    [sendBinary],
  );

  // ── 订阅生命周期:tab 激活 + WebView 就绪 + 连接在线(含重连)→ (重新)订阅 ──
  useEffect(() => {
    if (!active || !webReady || status !== "online" || !taskId) return;
    const streamId = nextStreamId++;
    streamIdRef.current = streamId;
    setStreamError(null);
    sendBinary(
      encodeTerminalFrame({
        opcode: TerminalOpcode.Subscribe,
        streamId,
        seq: 0,
        payload: jsonPayload({ taskId }),
      }),
    );
    return () => {
      sendBinary(
        encodeTerminalFrame({
          opcode: TerminalOpcode.Unsubscribe,
          streamId,
          seq: 0,
          payload: new Uint8Array(0),
        }),
      );
      if (streamIdRef.current === streamId) streamIdRef.current = 0;
    };
  }, [active, sendBinary, status, taskId, webReady]);

  // ── 入站终端帧 ──
  useEffect(() => {
    return onBinary((data) => {
      const frame = decodeTerminalFrame(new Uint8Array(data));
      if (!frame || frame.streamId !== streamIdRef.current) return;
      switch (frame.opcode) {
        case TerminalOpcode.SnapshotStart: {
          const meta = parseJsonPayload<SnapshotMeta>(frame.payload);
          if (writeFrameRef.current !== null) cancelAnimationFrame(writeFrameRef.current);
          writeFrameRef.current = null;
          pendingWritesRef.current = "";
          snapshotInProgressRef.current = true;
          snapshotLayoutPendingRef.current = false;
          snapshotLayoutBarrierRef.current = null;
          injectTerm({ type: "snapshotStart" });
          if (meta?.cols && meta?.rows) {
            // 记下桌面端尺寸,切「电脑视图」时用它还原
            desktopSizeRef.current = { cols: meta.cols, rows: meta.rows };
            // 先按桌面端尺寸铺快照,避免 TUI 布局错乱
            injectTerm({ type: "resize", cols: meta.cols, rows: meta.rows });
          }
          break;
        }
        case TerminalOpcode.SnapshotChunk:
        case TerminalOpcode.Output:
          queueTermWrite(payloadText(frame.payload));
          break;
        case TerminalOpcode.SnapshotEnd:
          snapshotInProgressRef.current = false;
          flushTermWrites(true, true);
          // 等快照写入 xterm 队列完成后再适配最终视图尺寸,否则全屏 TUI 的光标定位
          // 会按旧列数与新列数交错解析,在两侧形成竖排残影。
          requestSnapshotFinalLayout();
          break;
        case TerminalOpcode.Resized: {
          const size = parseJsonPayload<{ cols: number; rows: number }>(frame.payload);
          if (size) injectTerm({ type: "resize", cols: size.cols, rows: size.rows });
          break;
        }
        case TerminalOpcode.Error:
          setStreamError(payloadText(frame.payload));
          break;
        default:
          break;
      }
    });
  }, [flushTermWrites, injectTerm, onBinary, queueTermWrite, requestSnapshotFinalLayout]);

  const handleWebMessage = useCallback(
    (event: WebViewMessageEvent) => {
      let msg: {
        type?: string;
        data?: string;
        cols?: number;
        rows?: number;
        writeId?: number;
        focused?: boolean;
        hasSelection?: boolean;
        text?: string;
      };
      try {
        msg = JSON.parse(event.nativeEvent.data);
      } catch {
        return;
      }
      switch (msg.type) {
        case "ready":
          setWebReady(true);
          break;
        case "input":
          if (typeof msg.data === "string") sendInput(msg.data);
          break;
        case "ime-focus":
          // WebView 内隐藏 input 的 focus 状态,RN 的 Keyboard 事件覆盖不到
          setImeFocused(!!msg.focused);
          break;
        case "write-complete":
          if (typeof msg.writeId === "number" && Number.isInteger(msg.writeId) && msg.writeId > 0) {
            handleWriteComplete(msg.writeId);
          }
          break;
        case "selection":
          setHasSelection(!!msg.hasSelection);
          if (!msg.hasSelection) setCopied(false);
          break;
        case "selection-text":
          if (msg.text) {
            void Clipboard.setStringAsync(msg.text);
            setCopied(true);
            if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
            copiedTimerRef.current = setTimeout(() => setCopied(false), 1500);
          }
          break;
        case "haptic":
          // 长按进入选择模式的触感反馈;iOS 上 Vibration 为短促一下
          Vibration.vibrate(10);
          break;
        case "fit-result":
        case "resize-result":
          if (msg.cols && msg.rows && streamIdRef.current) {
            sendBinary(
              encodeTerminalFrame({
                opcode: TerminalOpcode.Resize,
                streamId: streamIdRef.current,
                seq: 0,
                payload: jsonPayload({ cols: msg.cols, rows: msg.rows }),
              }),
            );
          }
          break;
        default:
          break;
      }
    },
    [handleWriteComplete, sendBinary, sendInput],
  );

  const adjustFont = useCallback(
    (delta: number) => {
      setFontSize((prev) => {
        const next = Math.min(22, Math.max(9, prev + delta));
        const desktop = desktopSizeRef.current;
        if (viewModeRef.current === "desktop" && desktop) {
          injectTerm({ type: "fontSize", size: next, mode: "desktop", ...desktop });
        } else {
          injectTerm({ type: "fontSize", size: next, mode: "phone" });
        }
        return next;
      });
    },
    [injectTerm],
  );

  const pasteFromClipboard = useCallback(async () => {
    const text = await Clipboard.getStringAsync();
    if (text) sendInput(text);
  }, [sendInput]);

  // 键盘弹出/收起等导致终端区域高度变化时,运行中的任务自动重新适配
  const handleWrapLayout = useCallback(
    (event: { nativeEvent: { layout: { height: number } } }) => {
      const height = event.nativeEvent.layout.height;
      const prev = wrapHeightRef.current;
      wrapHeightRef.current = height;
      if (prev !== 0 && Math.abs(prev - height) < 2) return;
      scheduleRelayout();
    },
    [scheduleRelayout],
  );

  const stopRepeat = useCallback(() => {
    const timers = repeatTimersRef.current;
    if (timers.timeout) clearTimeout(timers.timeout);
    if (timers.interval) clearInterval(timers.interval);
    timers.timeout = null;
    timers.interval = null;
  }, []);

  // 长按连发:首字符立即发出,REPEAT_DELAY_MS 后进入 REPEAT_INTERVAL_MS 的连发
  const startRepeat = useCallback(
    (key: TerminalAccessoryKey) => {
      sendInput(key.bytes);
      if (!key.repeatable) return;
      stopRepeat();
      repeatTimersRef.current.timeout = setTimeout(() => {
        repeatTimersRef.current.interval = setInterval(
          () => sendInput(key.bytes),
          REPEAT_INTERVAL_MS,
        );
      }, REPEAT_DELAY_MS);
    },
    [sendInput, stopRepeat],
  );

  const toggleViewMode = useCallback(() => {
    setViewMode((prev) => {
      const next = prev === "phone" ? "desktop" : "phone";
      viewModeRef.current = next;
      const desktop = desktopSizeRef.current;
      if (next === "desktop" && desktop) {
        injectTerm({ type: "viewMode", mode: "desktop", ...desktop });
      } else {
        injectTerm({ type: "fit" });
      }
      return next;
    });
  }, [injectTerm]);

  useEffect(() => {
    scheduleRelayout();
  }, [scheduleRelayout, viewMode, windowHeight, windowWidth]);

  useEffect(() => {
    if (active || refitFrameRef.current === null) return;
    cancelAnimationFrame(refitFrameRef.current);
    refitFrameRef.current = null;
  }, [active]);

  useEffect(() => {
    return () => {
      if (refitFrameRef.current !== null) cancelAnimationFrame(refitFrameRef.current);
      refitFrameRef.current = null;
      if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
      if (writeFrameRef.current !== null) cancelAnimationFrame(writeFrameRef.current);
      writeFrameRef.current = null;
      pendingWritesRef.current = "";
      snapshotInProgressRef.current = false;
      snapshotLayoutPendingRef.current = false;
      snapshotLayoutBarrierRef.current = null;
      stopRepeat();
    };
  }, [stopRepeat]);

  return (
    /**
     * 不用 KeyboardAvoidingView:真正需要避让的输入框在 WebView 内部(隐藏
     * textarea),不参与 RN 布局,KAV 无从测量;且 KAV 在带导航头的屏幕上
     * 常把 header 高度算漏。这里直接用实测键盘高度做底部内边距,单一来源,
     * 顺带让 onLayout 触发 refit,把光标行顶到键盘上方。
     */
    <View style={[styles.screen, { paddingBottom: keyboardInset }]}>
      {status !== "online" ? <Text style={styles.notice}>{t("term.disconnected")}</Text> : null}
      {streamError ? <Text style={styles.noticeError}>{streamError}</Text> : null}
      <View style={styles.terminalWrap} onLayout={handleWrapLayout}>
        <WebView
          ref={webviewRef}
          source={{ html: TERMINAL_HTML }}
          onMessage={handleWebMessage}
          // The terminal is bundled as inline HTML; no remote navigation is
          // required or permitted.
          originWhitelist={["about:blank"]}
          javaScriptEnabled
          domStorageEnabled={false}
          allowFileAccess={false}
          setSupportMultipleWindows={false}
          hideKeyboardAccessoryView
          keyboardDisplayRequiresUserAction={false}
          style={styles.webview}
          containerStyle={styles.webview}
        />
      </View>
      {/* 长按选中后浮出:复制 / 取消。放在工具条上方,避免被输入法挤走 */}
      {hasSelection ? (
        <View style={styles.selectionBar}>
          <Text style={styles.selectionHint}>
            {copied ? t("term.copied") : t("term.selectionHint")}
          </Text>
          <View style={styles.selectionActions}>
            <AnimatedPressable
              onPress={() => injectTerm({ type: "copySelection" })}
              accessibilityLabel={t("term.copy")}
              style={[styles.key, styles.keyAccent]}
            >
              <Copy size={15} color={theme.onAccent} />
              <Text style={[styles.keyText, styles.keyTextAccent]}>{t("term.copy")}</Text>
            </AnimatedPressable>
            <AnimatedPressable
              onPress={() => injectTerm({ type: "clearSelection" })}
              accessibilityLabel={t("term.cancelSelection")}
              style={styles.key}
            >
              <X size={15} color={theme.text} />
            </AnimatedPressable>
          </View>
        </View>
      ) : null}
      <View style={styles.accessoryBar}>
        <View style={styles.accessoryRow}>
          {/* 固定在 ScrollView 外:开了输入法就是收起按钮,否则是手机/电脑视图切换 */}
          {imeFocused ? (
            <AnimatedPressable
              onPress={() => injectTerm({ type: "blur" })}
              accessibilityLabel={t("term.hideKeyboard")}
              style={[styles.key, styles.keyPinned]}
            >
              <ChevronsDownUp size={17} color={theme.text} />
            </AnimatedPressable>
          ) : (
            <AnimatedPressable
              onPress={toggleViewMode}
              accessibilityLabel={
                viewMode === "phone" ? t("term.desktopView") : t("term.phoneView")
              }
              style={[styles.key, styles.keyPinned, viewMode === "desktop" && styles.keyActive]}
            >
              {viewMode === "phone" ? (
                <Smartphone size={17} color={theme.text} />
              ) : (
                <Monitor size={17} color={theme.onAccent} />
              )}
            </AnimatedPressable>
          )}
          <ScrollView
            horizontal
            keyboardShouldPersistTaps="always"
            showsHorizontalScrollIndicator={false}
            style={styles.keyScroll}
            contentContainerStyle={styles.keyScrollContent}
          >
            <AnimatedPressable
              onPress={() => void pasteFromClipboard()}
              accessibilityLabel={t("term.paste")}
              style={styles.key}
            >
              <ClipboardPaste size={16} color={theme.text} />
              <Text style={styles.keyText}>{t("term.paste")}</Text>
            </AnimatedPressable>
            {TERMINAL_ACCESSORY_KEYS.map((key) => (
              <AnimatedPressable
                key={key.id}
                onPressIn={() => startRepeat(key)}
                onPressOut={stopRepeat}
                style={styles.key}
              >
                <Text style={styles.keyText}>{key.label}</Text>
              </AnimatedPressable>
            ))}
            <AnimatedPressable onPress={() => adjustFont(-1)} style={styles.key}>
              <Text style={styles.keyText}>A-</Text>
            </AnimatedPressable>
            <AnimatedPressable onPress={() => adjustFont(1)} style={styles.key}>
              <Text style={styles.keyText}>A+</Text>
            </AnimatedPressable>
          </ScrollView>
        </View>
        {/* 输入法弹出后这一行让位给键盘,只留上面的粘贴/ESC 行 */}
        {imeFocused ? null : (
          <AnimatedPressable
            style={styles.liveInputBar}
            onPress={() => injectTerm({ type: "focus" })}
          >
            <Text style={styles.liveInputTitle}>{t("term.liveInput")}</Text>
            <Text style={styles.liveInputDetail}>{t("term.tapToShowKeyboard")}</Text>
          </AnimatedPressable>
        )}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  notice: {
    color: theme.warning,
    fontSize: typography.metaSize,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.xs + 2,
    backgroundColor: theme.bgCard,
  },
  noticeError: {
    color: theme.danger,
    fontSize: typography.metaSize,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.xs + 2,
    backgroundColor: theme.bgCard,
  },
  terminalWrap: { flex: 1, backgroundColor: theme.bg },
  webview: { flex: 1, backgroundColor: theme.bg },
  accessoryBar: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    backgroundColor: theme.bgCard,
  },
  accessoryRow: {
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: spacing.sm,
    paddingVertical: spacing.xs,
    gap: spacing.xs + 2,
  },
  keyScroll: { flexGrow: 0, flexShrink: 1 },
  keyScrollContent: { gap: spacing.xs + 2, alignItems: "center" },
  key: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.xs + 2,
    minHeight: 32,
    paddingHorizontal: spacing.sm + 2,
    paddingVertical: 5,
    borderRadius: radii.button,
    backgroundColor: theme.bgElevated,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  keyPinned: { paddingHorizontal: spacing.sm + 4 },
  keyActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  keyAccent: { backgroundColor: theme.accent, borderColor: theme.accent },
  keyText: { color: theme.text, fontSize: 12.5, fontWeight: "600" },
  keyTextAccent: { color: theme.onAccent },
  selectionBar: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: spacing.sm,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.xs + 2,
    backgroundColor: theme.bgElevated,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
  },
  selectionHint: {
    flexShrink: 1,
    color: theme.textSecondary,
    fontSize: typography.metaSize,
  },
  selectionActions: { flexDirection: "row", alignItems: "center", gap: spacing.xs + 2 },
  liveInputBar: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    alignItems: "center",
    paddingHorizontal: spacing.lg,
    paddingVertical: spacing.sm,
    gap: 1,
  },
  liveInputTitle: {
    color: theme.text,
    fontSize: typography.metaSize,
    fontWeight: "600",
  },
  liveInputDetail: { color: theme.textSecondary, fontSize: typography.metaSize },
});
