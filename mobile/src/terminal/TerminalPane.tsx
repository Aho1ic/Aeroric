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
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
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
import { theme } from "../ui/theme";

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
  const [ctrlArmed, setCtrlArmed] = useState(false);
  const ctrlArmedRef = useRef(false);
  const autoFitDoneRef = useRef(false);
  const liveRef = useRef(false);

  const injectTerm = useCallback((msg: Record<string, unknown>) => {
    webviewRef.current?.injectJavaScript(
      `window.__aeroricTerm && window.__aeroricTerm.handle(${JSON.stringify(msg)}); true;`,
    );
  }, []);

  const sendInput = useCallback(
    (data: string) => {
      if (!streamIdRef.current) return;
      let payload = data;
      if (ctrlArmedRef.current && data.length === 1) {
        const code = data.toUpperCase().charCodeAt(0);
        if (code >= 64 && code <= 95) {
          payload = String.fromCharCode(code & 0x1f);
        }
        ctrlArmedRef.current = false;
        setCtrlArmed(false);
      }
      sendBinary(
        encodeTerminalFrame({
          opcode: TerminalOpcode.Input,
          streamId: streamIdRef.current,
          seq: 0,
          payload: textPayload(payload),
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
          // 运行中的任务:自动适配手机屏(SIGWINCH 触发 TUI 重绘)
          if (liveRef.current && !autoFitDoneRef.current) {
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
      let msg: { type?: string; data?: string; cols?: number; rows?: number };
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

  const toolbarKeys: Array<{ label: string; onPress: () => void; active?: boolean }> = [
    { label: "Esc", onPress: () => sendInput("\x1b") },
    { label: "Tab", onPress: () => sendInput("\t") },
    {
      label: "Ctrl",
      active: ctrlArmed,
      onPress: () => {
        ctrlArmedRef.current = !ctrlArmedRef.current;
        setCtrlArmed(ctrlArmedRef.current);
      },
    },
    { label: "^C", onPress: () => sendInput("\x03") },
    { label: "↑", onPress: () => sendInput("\x1b[A") },
    { label: "↓", onPress: () => sendInput("\x1b[B") },
    { label: "←", onPress: () => sendInput("\x1b[D") },
    { label: "→", onPress: () => sendInput("\x1b[C") },
    { label: "⏎", onPress: () => sendInput("\r") },
    { label: t("term.paste"), onPress: () => void pasteFromClipboard() },
    { label: "A-", onPress: () => adjustFont(-1) },
    { label: "A+", onPress: () => adjustFont(1) },
    { label: t("term.fit"), onPress: () => injectTerm({ type: "fit" }) },
    { label: t("term.keyboard"), onPress: () => injectTerm({ type: "focus" }) },
  ];

  return (
    <View style={styles.screen}>
      {status !== "online" ? (
        <Text style={styles.notice}>{t("term.disconnected")}</Text>
      ) : null}
      {streamError ? <Text style={styles.noticeError}>{streamError}</Text> : null}
      <View style={styles.terminalWrap}>
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
      <ScrollView
        horizontal
        keyboardShouldPersistTaps="always"
        showsHorizontalScrollIndicator={false}
        style={styles.toolbar}
        contentContainerStyle={styles.toolbarContent}
      >
        {toolbarKeys.map((key) => (
          <Pressable
            key={key.label}
            onPress={key.onPress}
            style={[styles.key, key.active && styles.keyActive]}
          >
            <Text style={[styles.keyText, key.active && styles.keyTextActive]}>{key.label}</Text>
          </Pressable>
        ))}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  notice: {
    color: theme.warning,
    fontSize: 12,
    paddingHorizontal: 12,
    paddingVertical: 6,
    backgroundColor: theme.bgCard,
  },
  noticeError: {
    color: theme.danger,
    fontSize: 12,
    paddingHorizontal: 12,
    paddingVertical: 6,
    backgroundColor: theme.bgCard,
  },
  terminalWrap: { flex: 1, backgroundColor: theme.bg },
  webview: { flex: 1, backgroundColor: theme.bg },
  toolbar: {
    flexGrow: 0,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    backgroundColor: theme.bgCard,
  },
  toolbarContent: { paddingHorizontal: 8, paddingVertical: 8, gap: 6 },
  key: {
    paddingHorizontal: 13,
    paddingVertical: 8,
    borderRadius: 8,
    backgroundColor: theme.bgElevated,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  keyActive: { backgroundColor: theme.accent, borderColor: theme.accent },
  keyText: { color: theme.text, fontSize: 13.5, fontWeight: "600" },
  keyTextActive: { color: "#fff" },
});
