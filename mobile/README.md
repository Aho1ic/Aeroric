# Aeroric Mobile

Aeroric Mobile is the scanner and remote client for the desktop app. The iPhone
camera app is only used to open an Expo development session; pairing QR codes
must be scanned from **Aeroric Mobile → Scan to pair**.

## Test on an iPhone with Expo Go

1. Install **Expo Go** from the iOS App Store.
2. From this directory, run `pnpm start --tunnel`.
3. Scan the Expo development QR code with the iPhone camera and open it in Expo Go.
4. In Aeroric Mobile, tap **Scan to pair**.
5. On the desktop, open **Settings → Remote Access**, choose the IP reachable
   from the iPhone, start the service, and generate the pairing QR code.

For same-Wi-Fi testing, choose the Wi-Fi/LAN address (for example
`192.168.1.10`). Choose a Tailscale or WireGuard address only when the iPhone is
connected to the same virtual network.

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
