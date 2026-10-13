# Remote public access

Aeroric remote access is intended for trusted personal devices. Pairing uses
an end-to-end encrypted channel, but a paired device is still a **fully trusted
controller**: it can read and modify project files, create and cancel tasks,
control agents and terminals, and change agent configuration. Existing API key
plaintext is not returned to remote clients.

## Security requirements

- Keep the desktop remote server disabled when it is not needed.
- Pair only phones you control. Revoke a lost or retired device immediately.
- Expose relay traffic through TLS (`wss://`) with a valid public certificate.
- Set a long, random `RELAY_TOKEN`. The relay refuses to start without it.
- Never put the relay token in a URL, reverse-proxy log, or client-side page.
- Keep the relay and reverse proxy updated and restrict administrative access.
- Direct `ws://` endpoints are suitable only for private LAN or trusted overlay
  networks because network metadata is not protected by TLS. Application
  payloads remain E2EE.

The desktop accepts `ws://` relay URLs only for `localhost`, `127.0.0.1`, or
`::1` development. Every non-loopback relay must use `wss://`.

## Relay deployment

The relay listens on plain WebSocket port `6791` by default. Bind it behind a
TLS reverse proxy rather than exposing that port directly:

```bash
export RELAY_TOKEN="$(openssl rand -base64 48)"
export RELAY_PORT=6791
cargo run --release --manifest-path remote-relay/Cargo.toml
```

Example Caddy configuration:

```caddyfile
relay.example.com {
  reverse_proxy 127.0.0.1:6791
}
```

Example nginx location inside a TLS-enabled server block:

```nginx
location / {
    proxy_pass http://127.0.0.1:6791;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_read_timeout 90s;
}
```

Configure the desktop with `wss://relay.example.com` and the exact same token.
The token authenticates desktop host registration; phone-to-desktop
application traffic is independently protected by Aeroric's X25519 and
ChaCha20-Poly1305 E2EE handshake.

## Operational checks

1. Confirm the public endpoint negotiates a trusted TLS certificate.
2. Confirm starting the relay without `RELAY_TOKEN` fails.
3. Confirm a wrong token is rejected.
4. Confirm a second registration for the same host ID is rejected rather than
   replacing the active desktop connection.
5. Review reverse-proxy access logs and Aeroric's local remote audit log.
6. Rotate the relay token after suspected disclosure and update every desktop
   that uses that relay.

## Threat model

The relay is a blind transport and cannot decrypt application frames. It can
still observe connection timing, IP addresses, host identifiers, and traffic
volume, and it can drop or delay traffic. TLS protects relay credentials and
transport metadata from network observers; E2EE protects application payloads
from the relay itself. Neither layer makes an already paired phone
untrusted—device revocation is the boundary for removing that authority.

## Push notifications (AP6)

Aeroric ships **no notifications service**. Reminders are delivered by a
sender that you deploy yourself, which is the only place the APNs auth key
(`.p8`) exists. The Mac app and the phone never hold it.

### What leaves your Mac

Only routing information: which kind of event happened (task done, task
failed, or an approval is pending), an event sequence number, and a
collapse key. There is no task title, project name, file path, command,
prompt or credential in the payload — the visible text is a key that the
phone app resolves to a localised string. A notification also never
authorizes anything: tapping it makes the phone re-check against the
authenticated session, and the notification itself cannot approve or run
work.

### Deploying the sender

```bash
export APNS_KEY_PEM="$(cat AuthKey_XXXXXXXXXX.p8)"
export APNS_KEY_ID=XXXXXXXXXX          # Apple Key ID
export APNS_TEAM_ID=YYYYYYYYYY         # Apple Team ID
export APNS_TOPIC=com.aeroric.ios.companion
export APNS_ENVIRONMENT=production     # sandbox for development builds
export PUSH_PORT=6792
export PUSH_CREDENTIALS_FILE=/var/lib/aeroric/push/credentials.json
export PUSH_BINDINGS_FILE=/var/lib/aeroric/push/bindings.json
cargo run --release --manifest-path remote-push/Cargo.toml
```

Mint one credential for the Mac that will drive notifications, then store the
printed secret in that Mac's Keychain:

```bash
cargo run --release --manifest-path remote-push/Cargo.toml -- enroll home-mac
```

The sender refuses to start if the key cannot be parsed, so a mistyped or
wrong-format `.p8` fails loudly instead of silently never delivering. It
requires a per-user HMAC signature on every request (`/v1/bind`, `/v1/push`),
rejects replayed nonces and stale timestamps, rate-limits per credential, and
drops a device binding as soon as APNs reports its token as unregistered.
`--help`, `check` and `enroll` are available without a running server.

Test the chain from the Mac's Remote settings: "Send test notification" goes
through the same minimal payload as a real reminder, and the panel shows the
host's counters (decisions, attempts, delivered, invalidated, failures) so
"notifications are on but nothing arrives" is diagnosable.

### Relay guidance and diagnostics

Relay use is unchanged: self-host it (above) or connect over a trusted direct
network. The Mac's Remote settings has a "Diagnose relay connectivity" action
that reports four separate facts — whether the URL scheme is acceptable,
whether the host is reachable, whether the relay accepted the registration
(that is what proves the shared token is right), and how long the exchange
took. A URL that is reachable but rejected is not "working", and a non-loopback
plaintext `ws://` URL is dropped at configuration load rather than used.

APNs delivery is best effort by Apple's own definition. Aeroric does not promise
that a reminder arrives, and it does not queue or retry reminders on your
behalf; when the sender reports back-pressure, the Mac simply stops sending for
a while.
