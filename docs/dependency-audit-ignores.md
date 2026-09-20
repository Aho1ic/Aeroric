# Dependency audit ignores

`package.json` → `pnpm.auditConfig.ignoreGhsas` lists advisory IDs that
`pnpm audit` must not fail CI on. JSON cannot carry comments, so rationale
lives here.

## GHSA-5p2g-fcmc-qvqq and GHSA-w3rx-r6r6-pgpr

| Field | Value |
| --- | --- |
| Package | `image-size` (mobile workspace dependency, pinned `1.2.1`) |
| Why ignored | pnpm's advisory database marks **every** `image-size <= 2.0.2` release as affected and does **not** list a fixed npm release. There is no upgrade path. |
| Mitigation | Workspace patch `mobile/patches/image-size@1.2.1.patch` adds bounds checks so malformed ICNS / JXL container lengths cannot leave the parser in a non-advancing loop. The installed 1.2.1 tarball already contains the equivalent zero-length guard in its shared HEIF/JXL box scanner. |
| Regression coverage | `mobile/src/security/image-size-security.test.ts` executes malformed ICNS, JXL, and HEIF inputs in child processes with a hard timeout so a missing fix fails safely instead of hanging the test runner. |
| Added | `c27b708d` (unified full-stack architecture; ignore moved from `mobile/package.json` to the root `package.json` workspace audit config) |
| Revisit when | upstream publishes a fixed `image-size` release that pnpm's DB accepts, **or** the mobile app drops the `image-size` dependency. Until then the ignore + patch pair is intentional. |
| Owner context | Desktop/mobile dependency hygiene; cargo-side equivalents are documented as comments in `src-tauri/.cargo/audit.toml`. |

Full patch narrative: [`mobile/patches/README.md`](../mobile/patches/README.md).

## Tauri opener scope (`src-tauri/capabilities/default.json`)

Frontend `openUrl` call sites (as of this note):

| Call site | URLs |
| --- | --- |
| `AboutPanel` | fixed `https://github.com/Aho1ic/Aeroric.git` |
| `NotificationBell` | release `html_url` (GitHub releases) |
| `WebPreviewPanel` | loopback preview tunnels (`http://127.0.0.1:*`, `http://localhost:*`) |
| `dshWebUi` | DSH Web UI on loopback |
| notebook WYSIWYG `mousedown.ts` | arbitrary user markdown links that pass `isSafeExternalUrl` (http **or** https) |

Capability allow list therefore keeps:

- concrete GitHub / githubusercontent / openssl-library.org hosts (fixed product/docs URLs)
- explicit loopback `http://localhost` / `127.0.0.1` / `[::1]` (preview + DSH)
- `https://*` — notebook notes and some notification/OAuth-adjacent flows open **user- or API-provided https URLs** that cannot be host-pinned in advance

`http://*` was removed. Plain-http open is limited to loopback. A notebook
link to a non-loopback `http://…` site will fail at the opener scope even
though `isSafeExternalUrl` still accepts the `http:` scheme in the UI filter.
Re-adding `http://*` should be a deliberate product decision, not a default.

Rust-side `OpenerExt::open_url` (storage OAuth authorize pages) uses https
provider hosts and is unaffected by dropping `http://*`.
