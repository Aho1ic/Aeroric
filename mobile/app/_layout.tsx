// E2EE 随机源 polyfill(expo-crypto),必须先于任何握手代码加载
import "../src/transport/install-crypto";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
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
        <StatusBar style="light" />
        <Stack
          screenOptions={{
            headerStyle: { backgroundColor: theme.bgCard },
            headerTintColor: theme.text,
            headerTitleStyle: { fontWeight: "600" },
            contentStyle: { backgroundColor: theme.bg },
          }}
        >
          <Stack.Screen name="index" options={{ title: "Aeroric" }} />
          <Stack.Screen name="pair" options={{ title: t("nav.pair") }} />
          <Stack.Screen name="hosts" options={{ title: t("nav.hosts") }} />
          <Stack.Screen name="new-task" options={{ title: t("nav.newTask") }} />
          <Stack.Screen name="task/[taskId]" options={{ title: t("common.task") }} />
          <Stack.Screen name="files" options={{ title: t("files.title") }} />
          <Stack.Screen name="file-view" options={{ title: t("files.title") }} />
        </Stack>
      </ConnectionProvider>
    </HostsProvider>
  );
}
