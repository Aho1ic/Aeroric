# Credential storage threat model

Aeroric is a local desktop workspace. Connection secrets are optimized for developer convenience and crash-safe reconnect, not multi-user host hardening.

## What is stored

| Secret | Location | Format | File mode (Unix) |
| --- | --- | --- | --- |
| SSH passwords | `~/.aeroric/ssh-connections.json` (via `aeroric_dir`) | JSON plaintext fields | `0600` via `atomic_write_private` |
| Database passwords / transport secrets | DBX connections v2 JSON under aeroric data dir | JSON plaintext on disk; API responses sanitize password fields | `0600` via `atomic_write_private` |
| Agent proxy / optional env credentials | App settings store | Config JSON | owner-private where applicable |

Windows inherits the private write path without Unix mode bits; rely on user profile ACLs.

## Runtime exposure

- SSH password auth sets `SSHPASS` for the child `sshpass`/`ssh` process.
- Optional auto-sudo injects the saved password into a remote shell after a ready marker.
- Clipboard "copy password" actions exist in SSH UI for operator convenience.

## Mitigations already in place

- Owner-only atomic writes for credential files (`storage::atomic_write_private`).
- DB connection list API sanitizes password-like fields for frontend display.
- Production database operations require an extra confirmation when marked production.
- Release builds keep DevTools feature-flagged off so casual IPC console access is not shipped.

## Non-goals (current)

- OS keychain / Credential Manager / libsecret integration
- At-rest encryption of the JSON store
- Multi-user shared machine isolation beyond OS user accounts

## Operator guidance

1. Prefer SSH keys / agent forwarding over saved passwords.
2. Leave SSH password empty to use interactive prompts when acceptable.
3. Mark production DB connections and production database lists so destructive SQL requires confirmation.
4. Do not run Aeroric under a shared OS account with untrusted local users.

## Future work

- Optional "session-only password" (memory, not disk)
- OS credential vault backends
- Redact secrets from crash reports / debug exports
