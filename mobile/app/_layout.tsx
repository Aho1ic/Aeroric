// E2EE 随机源 polyfill(expo-crypto),必须先于任何握手代码加载
import "../src/transport/install-crypto";
// 全局中文字体注入,必须先于任何渲染 Text 的模块加载
import "../src/ui/install-default-font";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { StyleSheet, View } from "react-native";
import { t } from "../src/i18n";
import { NotificationsBridge } from "../src/notifications/NotificationsBridge";
import { ConnectionProvider } from "../src/state/connection-context";
import { HostsProvider } from "../src/state/hosts-context";
import { theme } from "../src/ui/theme";

export default function RootLayout() {
  return (
    <HostsProvider>
      <ConnectionProvider>
        <NotificationsBridge />
        <View style={styles.root}>
          <View pointerEvents="none" style={StyleSheet.absoluteFill}>
            <View style={styles.blueOrb} />
            <View style={styles.purpleOrb} />
          </View>
          <StatusBar style="light" />
          <Stack
            screenOptions={{
              headerStyle: { backgroundColor: theme.bgCard },
              headerTintColor: theme.text,
              headerTitleStyle: { fontWeight: "600" },
              headerShadowVisible: false,
              contentStyle: { backgroundColor: theme.bg },
            }}
          >
            <Stack.Screen name="index" options={{ title: "Aeroric" }} />
            <Stack.Screen name="pair" options={{ title: t("nav.pair") }} />
            <Stack.Screen name="hosts" options={{ title: t("nav.hosts") }} />
            <Stack.Screen name="new-task" options={{ title: t("nav.newTask") }} />
            <Stack.Screen name="project/[projectId]" options={{ title: t("home.projects") }} />
            <Stack.Screen name="task/[taskId]" options={{ title: t("common.task") }} />
            <Stack.Screen name="agent-config" options={{ title: t("agentConfig.title") }} />
            <Stack.Screen name="files" options={{ title: t("files.title") }} />
            <Stack.Screen name="file-view" options={{ title: t("files.title") }} />
          </Stack>
        </View>
      </ConnectionProvider>
    </HostsProvider>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, overflow: "hidden", backgroundColor: theme.canvas },
  blueOrb: {
    position: "absolute",
    width: 340,
    height: 340,
    borderRadius: 170,
    top: -170,
    right: -120,
    backgroundColor: theme.orbBlue,
  },
  purpleOrb: {
    position: "absolute",
    width: 300,
    height: 300,
    borderRadius: 150,
    bottom: -150,
    left: -130,
    backgroundColor: theme.orbPurple,
  },
});
