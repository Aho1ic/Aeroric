/**
 * 终端面板:WebView 内嵌 xterm + 触屏工具条。
 * 从 M2 的 app/terminal/[taskId] 页面抽出,供任务详情页「终端」tab 复用;
 * `active=false`(切到会话 tab)时退订终端流,切回时重订阅走快照恢复。
 * 键盘避让由宿主页面统一处理。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { WebView, type WebViewMessageEvent } from "react-native-webview";
import * as Clipboard from "expo-clipboard";
import { ChevronsDownUp, ClipboardPaste, Monitor, Smartphone } from "lucide-react-native";
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
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

interface SnapshotMeta {
  cols?: number | null;
  rows?: number | null;
  live?: boolean;
}

export function TerminalPane({ taskId, active }: { taskId: string; active: boolean }) {
  const { status, sendBinary, onBinary } = useConnection();
  const webviewRef = useRef<WebView>(null);
  const streamIdRef = useRef(0);
  const [webReady, setWebReady] = useState(false);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [, setFontSize] = useState(13);
  const [imeFocused, setImeFocused] = useState(false);
  const [viewMode, setViewMode] = useState<"phone" | "desktop">("phone");
  /** 入站帧回调注册后不再重建,靠 ref 读取最新视图模式。 */
  const viewModeRef = useRef<"phone" | "desktop">("phone");
  const autoFitDoneRef = useRef(false);
  const liveRef = useRef(false);
  const desktopSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const wrapHeightRef = useRef(0);
  const refitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const repeatTimersRef = useRef<{
    timeout: ReturnType<typeof setTimeout> | null;
    interval: ReturnType<typeof setInterval> | null;
  }>({ timeout: null, interval: null });

  const injectTerm = useCallback((msg: Record<string, unknown>) => {
    webviewRef.current?.injectJavaScript(
      `window.__aeroricTerm && window.__aeroricTerm.handle(${JSON.stringify(msg)}); true;`,
    );
  }, []);

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
    autoFitDoneRef.current = false;
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
          liveRef.current = meta?.live ?? false;
          injectTerm({ type: "reset" });
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
          injectTerm({ type: "write", data: payloadText(frame.payload) });
          break;
        case TerminalOpcode.SnapshotEnd:
          injectTerm({ type: "scrollToBottom" });
          // 运行中的任务:自动适配手机屏(SIGWINCH 触发 TUI 重绘);「电脑视图」下保持桌面尺寸
          if (liveRef.current && !autoFitDoneRef.current && viewModeRef.current === "phone") {
            autoFitDoneRef.current = true;
            injectTerm({ type: "fit" });
          }
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
  }, [injectTerm, onBinary]);

  const handleWebMessage = useCallback(
    (event: WebViewMessageEvent) => {
      let msg: {
        type?: string;
        data?: string;
        cols?: number;
        rows?: number;
        focused?: boolean;
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
        case "fit-result":
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
    [sendBinary, sendInput],
  );

  const adjustFont = useCallback(
    (delta: number) => {
      setFontSize((prev) => {
        const next = Math.min(22, Math.max(9, prev + delta));
        injectTerm({ type: "fontSize", size: next });
        injectTerm({ type: "fit" });
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
      if (prev === 0 || Math.abs(prev - height) < 2 || !liveRef.current) return;
      if (refitTimerRef.current) clearTimeout(refitTimerRef.current);
      refitTimerRef.current = setTimeout(() => injectTerm({ type: "fit" }), 150);
    },
    [injectTerm],
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
        // 还原桌面端 cols/rows,靠 WebView 横向滚动看完整 TUI
        injectTerm({ type: "resize", cols: desktop.cols, rows: desktop.rows });
      } else {
        injectTerm({ type: "fit" });
      }
      return next;
    });
  }, [injectTerm]);

  useEffect(() => {
    return () => {
      if (refitTimerRef.current) clearTimeout(refitTimerRef.current);
      stopRepeat();
    };
  }, [stopRepeat]);

  return (
    <View style={styles.screen}>
      {status !== "online" ? <Text style={styles.notice}>{t("term.disconnected")}</Text> : null}
      {streamError ? <Text style={styles.noticeError}>{streamError}</Text> : null}
      <View style={styles.terminalWrap} onLayout={handleWrapLayout}>
        <WebView
          ref={webviewRef}
          source={{ html: TERMINAL_HTML }}
          onMessage={handleWebMessage}
          originWhitelist={["*"]}
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
      <View style={styles.accessoryBar}>
        <View style={styles.accessoryRow}>
          {/* 固定在 ScrollView 外:开了输入法就是收起按钮,否则是手机/电脑视图切换 */}
          {imeFocused ? (
            <Pressable
              onPress={() => injectTerm({ type: "blur" })}
              accessibilityLabel={t("term.hideKeyboard")}
              style={[styles.key, styles.keyPinned]}
            >
              <ChevronsDownUp size={17} color={theme.text} />
            </Pressable>
          ) : (
            <Pressable
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
            </Pressable>
          )}
          <ScrollView
            horizontal
            keyboardShouldPersistTaps="always"
            showsHorizontalScrollIndicator={false}
            style={styles.keyScroll}
            contentContainerStyle={styles.keyScrollContent}
          >
            <Pressable
              onPress={() => void pasteFromClipboard()}
              accessibilityLabel={t("term.paste")}
              style={styles.key}
            >
              <ClipboardPaste size={16} color={theme.text} />
              <Text style={styles.keyText}>{t("term.paste")}</Text>
            </Pressable>
            {TERMINAL_ACCESSORY_KEYS.map((key) => (
              <Pressable
                key={key.id}
                onPressIn={() => startRepeat(key)}
                onPressOut={stopRepeat}
                style={styles.key}
              >
                <Text style={styles.keyText}>{key.label}</Text>
              </Pressable>
            ))}
            <Pressable onPress={() => adjustFont(-1)} style={styles.key}>
              <Text style={styles.keyText}>A-</Text>
            </Pressable>
            <Pressable onPress={() => adjustFont(1)} style={styles.key}>
              <Text style={styles.keyText}>A+</Text>
            </Pressable>
          </ScrollView>
        </View>
        {/* 输入法弹出后这一行让位给键盘,只留上面的粘贴/ESC 行 */}
        {imeFocused ? null : (
          <Pressable style={styles.liveInputBar} onPress={() => injectTerm({ type: "focus" })}>
            <Text style={styles.liveInputTitle}>{t("term.liveInput")}</Text>
            <Text style={styles.liveInputDetail}>{t("term.tapToShowKeyboard")}</Text>
          </Pressable>
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
  keyText: { color: theme.text, fontSize: 12.5, fontWeight: "600" },
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
