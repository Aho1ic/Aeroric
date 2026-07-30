/** parseMarkdown 结果的 RN 渲染(纯展示,解析逻辑在 markdown.ts)。 */

import { useMemo } from "react";
import { Platform, StyleSheet, Text, View } from "react-native";
import { theme } from "../ui/theme";
import { parseMarkdown, type MdInline } from "./markdown";

const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

function InlineSpans({ spans }: { spans: MdInline[] }) {
  return (
    <>
      {spans.map((span, i) => {
        switch (span.kind) {
          case "code":
            return (
              <Text key={i} style={styles.inlineCode}>
                {span.text}
              </Text>
            );
          case "bold":
            return (
              <Text key={i} style={styles.bold}>
                {span.text}
              </Text>
            );
          case "italic":
            return (
              <Text key={i} style={styles.italic}>
                {span.text}
              </Text>
            );
          default:
            return <Text key={i}>{span.text}</Text>;
        }
      })}
    </>
  );
}

const HEADING_SIZES: Record<number, number> = { 1: 19, 2: 17.5, 3: 16, 4: 15, 5: 14.5, 6: 14 };

export function MarkdownText({ text }: { text: string }) {
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  return (
    <View style={styles.wrap}>
      {blocks.map((block, i) => {
        switch (block.kind) {
          case "codeBlock":
            return (
              <View key={i} style={styles.codeBlock}>
                <Text style={styles.codeText} selectable>
                  {block.text}
                </Text>
              </View>
            );
          case "heading":
            return (
              <Text
                key={i}
                style={[styles.heading, { fontSize: HEADING_SIZES[block.level] ?? 14 }]}
              >
                <InlineSpans spans={block.spans} />
              </Text>
            );
          case "bullet":
            return (
              <View key={i} style={styles.listRow}>
                <Text style={styles.listMarker}>•</Text>
                <Text style={styles.body}>
                  <InlineSpans spans={block.spans} />
                </Text>
              </View>
            );
          case "ordered":
            return (
              <View key={i} style={styles.listRow}>
                <Text style={styles.listMarker}>{block.index}.</Text>
                <Text style={styles.body}>
                  <InlineSpans spans={block.spans} />
                </Text>
              </View>
            );
          case "quote":
            return (
              <View key={i} style={styles.quote}>
                <Text style={styles.quoteText}>
                  <InlineSpans spans={block.spans} />
                </Text>
              </View>
            );
          default:
            return (
              <Text key={i} style={styles.body} selectable>
                <InlineSpans spans={block.spans} />
              </Text>
            );
        }
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: { gap: 7 },
  body: { color: theme.text, fontSize: 13.5, lineHeight: 21, flex: 1 },
  heading: { color: theme.text, fontWeight: "700", lineHeight: 24 },
  bold: { fontWeight: "700" },
  italic: { fontStyle: "italic" },
  inlineCode: {
    fontFamily: MONO,
    fontSize: 12.5,
    color: theme.accent,
    backgroundColor: theme.bgElevated,
  },
  codeBlock: {
    backgroundColor: theme.bgElevated,
    borderRadius: 8,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    paddingHorizontal: 10,
    paddingVertical: 8,
  },
  codeText: { fontFamily: MONO, fontSize: 12, lineHeight: 17.5, color: theme.text },
  listRow: { flexDirection: "row", gap: 7, paddingRight: 4 },
  listMarker: { color: theme.textSecondary, fontSize: 13.5, lineHeight: 21 },
  quote: {
    borderLeftWidth: 2,
    borderLeftColor: theme.border,
    paddingLeft: 10,
  },
  quoteText: { color: theme.textSecondary, fontSize: 13, lineHeight: 20, fontStyle: "italic" },
});
