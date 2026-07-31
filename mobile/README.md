# Aeroric Mobile

Aeroric Mobile is the scanner and remote client for the desktop app. The iPhone
camera app is only used to open an Expo development session; pairing QR codes
must be scanned from **Aeroric Mobile → Scan to pair**.

## Test on an iPhone with Expo Go

1. Install **Expo Go** from the iOS App Store.
2. From this directory, run `pnpm expo start --lan` (or `pnpm start`).
3. Pick the computer address the phone can reach — see below.
4. Scan the Expo development QR code with the iPhone camera and open it in Expo Go.
5. In Aeroric Mobile, tap **Scan to pair**.
6. On the desktop, open **Settings → Remote Access**, choose the IP reachable
   from the iPhone, start the service, and generate the pairing QR code.

`pnpm expo start --lan` goes through `scripts/expo-cli.mjs`, which prompts for the
LAN address before handing it to Expo via `REACT_NATIVE_PACKAGER_HOSTNAME`:

```
请选择 Expo Go 扫码使用的电脑 LAN 地址（手机需与该地址同一网络）:
  1. 192.168.0.121 (en0) [推荐，局域网]
  2. 198.18.0.1 (utun1024) [代理 fake-IP，手机无法访问]
  3. 手动输入 IPv4 地址
  4. 交给 Expo 自动检测
```

Pick the Wi-Fi/LAN address (for example `192.168.0.121`). Without this prompt,
Expo may auto-select a Clash/Surge TUN address such as `198.18.0.1`, and Expo Go
fails with `The request timed out`. Choose a Tailscale or WireGuard address only
when the iPhone is on the same virtual network.

The choice is remembered in `.expo/aeroric-lan.json` and offered as the default
next time. To skip the prompt entirely (CI, scripts), set the address up front:

```bash
AERORIC_EXPO_LAN_IP=192.168.0.121 pnpm expo start --lan
```

`--tunnel` and `--localhost` do not need a LAN address, so no prompt appears.

## Build an installable iOS package

An installable iPhone build must be signed with an Apple Developer account and
include the test device in the provisioning profile.

```bash
pnpm dlx eas-cli login
pnpm dlx eas-cli device:create
pnpm dlx eas-cli build --platform ios --profile preview
```

The `preview` profile creates an internal-distribution build that can be
installed on registered devices from the EAS build page.
