import * as Device from "expo-device";
import { router, useLocalSearchParams } from "expo-router";
import { CameraView, useCameraPermissions } from "expo-camera";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  Platform,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { t } from "../src/i18n";
import { useHosts } from "../src/state/hosts-context";
import { parsePairingOffer } from "../src/transport/pairing-offer";
import { pairWithInvite, preferredPairingError } from "../src/transport/remote-connection";
import type { PairedHost } from "../src/types";
import { AnimatedPressable } from "../src/ui/AnimatedPressable";
import { radii, theme } from "../src/ui/theme";

function deviceDisplayName(): string {
  return Device.deviceName ?? Device.modelName ?? (Platform.OS === "ios" ? "iPhone" : "Android");
}

function newHostId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export default function PairScreen() {
  const { code: deepLinkCode } = useLocalSearchParams<{ code?: string }>();
  const {
    addHost,
    loadError,
    retryLoad,
    waitUntilReady,
    stagePendingPairingHost,
    promotePendingPairingHost,
    discardPendingPairingHost,
  } = useHosts();
  const [permission, requestPermission] = useCameraPermissions();
  const [manualCode, setManualCode] = useState("");
  const [pairing, setPairing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);

  const handleCode = useCallback(
    async (input: string) => {
      if (busyRef.current) return;
      busyRef.current = true;
      setPairing(true);
      setError(null);
      try {
        await waitUntilReady();
        const offer = parsePairingOffer(input);
        // 依次尝试 offer 中的地址(LAN → 自定义公网 → relay)
        const endpointErrors: Error[] = [];
        let pairedResult: Awaited<ReturnType<typeof pairWithInvite>> | null = null;
        const hostId = offer.hostId ?? offer.publicKey;
        const makeHost = (credentials: Awaited<ReturnType<typeof pairWithInvite>>): PairedHost => ({
          id: hostId ?? newHostId(),
          hostId,
          name: offer.hostName,
          endpoints: offer.endpoints,
          publicKey: offer.publicKey,
          deviceId: credentials.deviceId,
          deviceToken: credentials.deviceToken,
          pairedAt: Date.now(),
        });
        for (const endpoint of offer.endpoints) {
          try {
            pairedResult = await pairWithInvite({
              endpoint,
              invite: offer.invite,
              deviceName: deviceDisplayName(),
              serverPublicKey: offer.publicKey,
              pairingConfirmationVersion: offer.pairingConfirmationVersion,
              persistProvisionalCredentials:
                offer.pairingConfirmationVersion === 1
                  ? async (credentials) => stagePendingPairingHost(makeHost(credentials))
                  : undefined,
              discardProvisionalCredentials:
                offer.pairingConfirmationVersion === 1 ? discardPendingPairingHost : undefined,
            });
            break;
          } catch (err) {
            endpointErrors.push(err instanceof Error ? err : new Error(String(err)));
          }
        }
        if (!pairedResult) {
          throw endpointErrors.length
            ? preferredPairingError(endpointErrors)
            : new Error(t("pair.cannotReach"));
        }
        const host = makeHost(pairedResult);
        if (offer.pairingConfirmationVersion === 1) {
          await promotePendingPairingHost(host);
        } else {
          await addHost(host);
        }
        router.replace("/");
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        busyRef.current = false;
        setPairing(false);
      }
    },
    [
      addHost,
      discardPendingPairingHost,
      promotePendingPairingHost,
      stagePendingPairingHost,
      waitUntilReady,
    ],
  );

  // 深链进入(aeroric://pair?code=…):自动开始配对
  useEffect(() => {
    if (typeof deepLinkCode === "string" && deepLinkCode) {
      void handleCode(`aeroric://pair?code=${deepLinkCode}`);
    }
  }, [deepLinkCode, handleCode]);

  const cameraGranted = permission?.granted ?? false;

  return (
    <ScrollView
      style={styles.screen}
      contentContainerStyle={styles.content}
      keyboardShouldPersistTaps="handled"
    >
      <Text style={styles.stepText}>{t("pair.steps")}</Text>
      <View style={styles.trustWarning}>
        <Text style={styles.trustWarningText}>{t("pair.trustWarning")}</Text>
      </View>

      <View style={styles.cameraCard}>
        {cameraGranted ? (
          <CameraView
            style={styles.camera}
            barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
            onBarcodeScanned={
              pairing || loadError ? undefined : ({ data }) => void handleCode(data)
            }
          />
        ) : (
          <View style={styles.cameraPlaceholder}>
            <Text style={styles.cameraHint}>
              {permission?.canAskAgain === false ? t("pair.cameraDenied") : t("pair.cameraNeeded")}
            </Text>
            {permission?.canAskAgain !== false ? (
              <AnimatedPressable
                style={styles.primaryButton}
                onPress={() => void requestPermission()}
              >
                <Text style={styles.primaryButtonText}>{t("pair.grantCamera")}</Text>
              </AnimatedPressable>
            ) : null}
          </View>
        )}
        {pairing ? (
          <View style={styles.pairingOverlay}>
            <ActivityIndicator color={theme.accent} size="large" />
            <Text style={styles.pairingText}>{t("pair.pairing")}</Text>
          </View>
        ) : null}
      </View>

      <Text style={styles.dividerText}>{t("pair.manualHint")}</Text>
      <TextInput
        style={styles.input}
        value={manualCode}
        onChangeText={setManualCode}
        placeholder="aeroric://pair?code=…"
        placeholderTextColor={theme.textHint}
        autoCapitalize="none"
        autoCorrect={false}
        multiline
      />
      <AnimatedPressable
        style={[
          styles.primaryButton,
          (!manualCode.trim() || pairing || loadError) && styles.buttonDisabled,
        ]}
        disabled={!manualCode.trim() || pairing || Boolean(loadError)}
        onPress={() => void handleCode(manualCode)}
      >
        <Text style={styles.primaryButtonText}>{t("pair.connect")}</Text>
      </AnimatedPressable>

      {loadError ? (
        <View style={styles.loadErrorCard}>
          <Text style={styles.errorText}>{t("hosts.loadFailed")}</Text>
          <AnimatedPressable
            style={styles.retryButton}
            onPress={() => {
              setError(null);
              void retryLoad().catch(() => {
                // The provider keeps the latest failure visible.
              });
            }}
          >
            <Text style={styles.retryButtonText}>{t("common.retry")}</Text>
          </AnimatedPressable>
        </View>
      ) : null}
      {error ? <Text style={styles.errorText}>{error}</Text> : null}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: theme.bg },
  content: { padding: 20, gap: 14 },
  stepText: { color: theme.textSecondary, fontSize: 13, lineHeight: 20 },
  trustWarning: {
    borderRadius: radii.input,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.warning,
    backgroundColor: theme.bgCard,
    padding: 12,
  },
  trustWarningText: { color: theme.textSecondary, fontSize: 12.5, lineHeight: 19 },
  cameraCard: {
    height: 300,
    borderRadius: radii.card,
    overflow: "hidden",
    backgroundColor: theme.bgCard,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
  },
  camera: { flex: 1 },
  cameraPlaceholder: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    gap: 14,
    padding: 24,
  },
  cameraHint: { color: theme.textSecondary, fontSize: 13, textAlign: "center", lineHeight: 20 },
  pairingOverlay: {
    ...StyleSheet.absoluteFillObject,
    backgroundColor: "rgba(13,17,23,0.82)",
    alignItems: "center",
    justifyContent: "center",
    gap: 10,
  },
  pairingText: { color: theme.text, fontSize: 14 },
  dividerText: { color: theme.textHint, fontSize: 12, textAlign: "center", marginTop: 6 },
  input: {
    minHeight: 72,
    borderRadius: radii.input,
    borderWidth: 1,
    borderColor: theme.border,
    backgroundColor: theme.bgCard,
    color: theme.text,
    padding: 12,
    fontSize: 13,
    textAlignVertical: "top",
  },
  primaryButton: {
    backgroundColor: theme.accent,
    borderRadius: radii.button,
    paddingVertical: 12,
    alignItems: "center",
  },
  buttonDisabled: { opacity: 0.45 },
  primaryButtonText: { color: "#fff", fontSize: 15, fontWeight: "600" },
  loadErrorCard: { gap: 10, alignItems: "flex-start" },
  retryButton: {
    borderRadius: radii.button,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  retryButtonText: { color: theme.text, fontSize: 13, fontWeight: "600" },
  errorText: { color: theme.danger, fontSize: 13, lineHeight: 19 },
});
