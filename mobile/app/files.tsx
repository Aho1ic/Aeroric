/**
 * 独立文件浏览页:薄壳,实际列表逻辑在 FilesPane(任务详情页的「文件」tab 共用同一实现)。
 */

import { Stack, useLocalSearchParams } from "expo-router";
import { StyleSheet, View } from "react-native";
import { FilesPane } from "../src/files/FilesPane";
import { t } from "../src/i18n";
import { theme } from "../src/ui/theme";

export default function FilesScreen() {
  const params = useLocalSearchParams<{ projectId?: string; path?: string; name?: string }>();
  const projectId = typeof params.projectId === "string" ? params.projectId : "";
  const path = typeof params.path === "string" ? params.path : "";
  const title = typeof params.name === "string" && params.name ? params.name : t("files.title");

  return (
    <View style={styles.screen}>
      <Stack.Screen options={{ title }} />
      <FilesPane projectId={projectId} active initialPath={path} />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
});
