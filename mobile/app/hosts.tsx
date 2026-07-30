import { Link } from "expo-router";
import { useState } from "react";
import {
  Alert,
  FlatList,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { t } from "../src/i18n";
import { useConnection } from "../src/state/connection-context";
import { useHosts } from "../src/state/hosts-context";
import type { PairedHost } from "../src/types";
import { theme } from "../src/ui/theme";

export default function HostsScreen() {
  const { hosts, activeHost, setActiveHost, removeHost, updateHostEndpoints } = useHosts();
  const { status } = useConnection();
  // 正在编辑地址的主机 id 与草稿(每行一个地址)
  const [editingHostId, setEditingHostId] = useState<string | null>(null);
  const [endpointsDraft, setEndpointsDraft] = useState("");
  const [savingEndpoints, setSavingEndpoints] = useState(false);

  const confirmRemove = (host: PairedHost) => {
    Alert.alert(t("hosts.removeTitle"), t("hosts.removeMessage", { name: host.name }), [
      { text: t("hosts.cancel"), style: "cancel" },
      {
        text: t("hosts.remove"),
        style: "destructive",
        onPress: () => {
          void removeHost(host.id).catch((err) => {
            Alert.alert(t("hosts.removeFailed"), err instanceof Error ? err.message : String(err));
          });
        },
      },
    ]);
  };

  const startEditEndpoints = (host: PairedHost) => {
    setEditingHostId(host.id);
    setEndpointsDraft(host.endpoints.join("\n"));
  };

  const saveEndpoints = (host: PairedHost) => {
    if (savingEndpoints) return;
    setSavingEndpoints(true);
    const endpoints = endpointsDraft
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    void updateHostEndpoints(host.id, endpoints)
      .then(() => setEditingHostId(null))
      .catch((err) => {
        Alert.alert(t("hosts.saveFailed"), err instanceof Error ? err.message : String(err));
      })
      .finally(() => setSavingEndpoints(false));
  };

  return (
    <View style={styles.screen}>
      <FlatList
        data={hosts}
        keyExtractor={(item) => item.id}
        contentContainerStyle={styles.list}
        renderItem={({ item }) => {
          const isActive = item.id === activeHost?.id;
          const isEditing = item.id === editingHostId;
          return (
            <View style={[styles.hostCard, isActive && styles.hostCardActive]}>
              <Pressable
                style={styles.hostRow}
                onPress={() => {
                  void setActiveHost(item.id).catch((err) => {
                    Alert.alert(t("hosts.switchFailed"), err instanceof Error ? err.message : String(err));
                  });
                }}
                onLongPress={() => confirmRemove(item)}
              >
                <View style={styles.hostBody}>
                  <Text style={styles.hostName}>
                    {item.name}
                    {isActive && status === "online" ? (
                      <Text style={{ color: theme.success }}>  ●</Text>
                    ) : null}
                  </Text>
                  <Text style={styles.hostEndpoint} numberOfLines={1}>
                    {item.endpoints.length > 1
                      ? t("hosts.endpointCount", { first: item.endpoints[0], count: item.endpoints.length })
                      : item.endpoints[0]}
                  </Text>
                </View>
                {isActive ? <Text style={styles.activeBadge}>{t("hosts.active")}</Text> : null}
                <Pressable
                  hitSlop={10}
                  onPress={() => (isEditing ? setEditingHostId(null) : startEditEndpoints(item))}
                >
                  <Text style={styles.editText}>{isEditing ? t("hosts.collapse") : t("hosts.edit")}</Text>
                </Pressable>
                <Pressable hitSlop={10} onPress={() => confirmRemove(item)}>
                  <Text style={styles.removeText}>{t("hosts.remove")}</Text>
                </Pressable>
              </Pressable>
              {isEditing ? (
                <View style={styles.editArea}>
                  <Text style={styles.editHint}>{t("hosts.endpointsHint")}</Text>
                  <TextInput
                    style={styles.editInput}
                    value={endpointsDraft}
                    onChangeText={setEndpointsDraft}
                    autoCapitalize="none"
                    autoCorrect={false}
                    multiline
                    placeholder={"ws://192.168.1.10:6790\nwss://relay.example.com/connect/…"}
                    placeholderTextColor={theme.textHint}
                  />
                  <Pressable
                    style={[styles.saveButton, savingEndpoints && styles.buttonDisabled]}
                    disabled={savingEndpoints}
                    onPress={() => saveEndpoints(item)}
                  >
                    <Text style={styles.saveButtonText}>{t("hosts.saveEndpoints")}</Text>
                  </Pressable>
                </View>
              ) : null}
            </View>
          );
        }}
        ListEmptyComponent={<Text style={styles.emptyText}>{t("hosts.empty")}</Text>}
        ListFooterComponent={
          <Link href="/pair" asChild>
            <Pressable style={styles.addButton}>
              <Text style={styles.addButtonText}>{t("hosts.addNew")}</Text>
            </Pressable>
          </Link>
        }
      />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  list: { padding: 14, gap: 8 },
  hostCard: {
    backgroundColor: theme.bgCard,
    borderRadius: 12,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  hostCardActive: { borderColor: theme.accent, borderWidth: 1 },
  hostRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    padding: 14,
  },
  hostBody: { flex: 1, gap: 3 },
  hostName: { color: theme.text, fontSize: 15, fontWeight: "600" },
  hostEndpoint: { color: theme.textHint, fontSize: 12 },
  activeBadge: { color: theme.accent, fontSize: 12, fontWeight: "600" },
  editText: { color: theme.accent, fontSize: 12.5 },
  removeText: { color: theme.danger, fontSize: 12.5 },
  editArea: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    padding: 12,
    gap: 10,
  },
  editHint: { color: theme.textSecondary, fontSize: 12, lineHeight: 18 },
  editInput: {
    minHeight: 84,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: theme.border,
    backgroundColor: theme.bg,
    color: theme.text,
    padding: 10,
    fontSize: 12.5,
    textAlignVertical: "top",
  },
  saveButton: {
    backgroundColor: theme.accent,
    borderRadius: 10,
    paddingVertical: 10,
    alignItems: "center",
  },
  buttonDisabled: { opacity: 0.45 },
  saveButtonText: { color: "#fff", fontSize: 13.5, fontWeight: "600" },
  emptyText: { color: theme.textSecondary, fontSize: 13.5, textAlign: "center", marginTop: 40 },
  addButton: {
    marginTop: 10,
    borderRadius: 12,
    borderWidth: 1,
    borderColor: theme.border,
    paddingVertical: 13,
    alignItems: "center",
  },
  addButtonText: { color: theme.accent, fontSize: 14.5, fontWeight: "600" },
});
