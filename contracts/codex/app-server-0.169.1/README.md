# Codex app-server protocol snapshot

Generated with `codex app-server generate-json-schema --out <dir>` from the version in `VERSION`
(E9, 2026-11-27), the same four files as `app-server-0.162.1`. The newest Codex the native
driver has been checked against; `0.162.1` stays as the oldest it supports (`version_supported`).
Every snapshot here checks each message the driver sends (`codex::contract`); the history upgrade
test (`real_codex_history_survives_an_upgrade`) passed 0.162.1 → 0.165.0 → 0.169.1.
