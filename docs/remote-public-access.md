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
