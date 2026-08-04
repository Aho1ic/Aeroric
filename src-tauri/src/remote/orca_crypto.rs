//! Orca mobile E2EE v2 compatibility.
//!
//! This module mirrors Orca's `mobile-e2ee-v2-contract.ts`,
//! `mobile-e2ee-v2-key-schedule.ts`, and `mobile-e2ee-v2-framing.ts`.
//! It deliberately lives beside Aeroric's existing protocol instead of
//! changing that protocol in place: the server can negotiate either wire
//! contract while clients are migrated.

use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hkdf::Hkdf;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use xsalsa20poly1305::aead::{AeadInPlace, KeyInit};
use xsalsa20poly1305::{Key, Nonce, XSalsa20Poly1305};

use super::crypto::StaticKeys;

pub const VERSION: u64 = 2;
pub const PROTOCOL: &str = "orca-mobile-e2ee";
pub const TRANSCRIPT_DOMAIN: &[u8] = b"orca-mobile-e2ee/v2/transcript";
pub const TEXT_KIND: u8 = 0;
pub const BINARY_KIND: u8 = 1;

const NONCE_BYTES: usize = 24;
const SESSION_ID_BYTES: usize = 32;
const HEADER_BYTES: usize = SESSION_ID_BYTES + 1 + 1 + 8;
const TAG_BYTES: usize = 16;
const MAX_COUNTER: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transport {
    Direct,
    Relay,
}

impl Transport {
    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Relay => "relay",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub transport: Transport,
    pub relay_host_id: Option<String>,
}

pub(crate) struct HandshakeAccept {
    pub ready_json: String,
    pub session: SessionCrypto,
}

/// Orca's deterministic, direction- and payload-kind-bound secretbox session.
pub(crate) struct SessionCrypto {
    send: DirectionCipher,
    recv: DirectionCipher,
    transcript_hash_b64: String,
}

impl fmt::Debug for SessionCrypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrcaSessionCrypto")
            .field("sendCounter", &self.send.counter)
            .field("recvCounter", &self.recv.counter)
            .finish_non_exhaustive()
    }
}

struct DirectionCipher {
    cipher: XSalsa20Poly1305,
    session_id: [u8; SESSION_ID_BYTES],
    direction: u8,
    counter: u64,
}

impl DirectionCipher {
    fn new(key: [u8; 32], session_id: [u8; SESSION_ID_BYTES], direction: u8) -> Self {
        Self {
            cipher: XSalsa20Poly1305::new(Key::from_slice(&key)),
            session_id,
            direction,
            counter: 0,
        }
    }

    fn nonce(&self, kind: u8, counter: u64) -> Nonce {
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[..12].copy_from_slice(&self.session_id[..12]);
        nonce[12] = VERSION as u8;
        nonce[13] = self.direction;
        nonce[14] = kind;
        nonce[15] = 0;
        nonce[16..].copy_from_slice(&counter.to_be_bytes());
        Nonce::from(nonce)
    }

    fn header(&self, kind: u8, counter: u64) -> [u8; HEADER_BYTES] {
        let mut header = [0u8; HEADER_BYTES];
        header[..SESSION_ID_BYTES].copy_from_slice(&self.session_id);
        header[SESSION_ID_BYTES] = self.direction;
        header[SESSION_ID_BYTES + 1] = kind;
        header[SESSION_ID_BYTES + 2..].copy_from_slice(&counter.to_be_bytes());
        header
    }

    fn seal(&mut self, kind: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        if self.counter == MAX_COUNTER {
            return Err("Orca E2EE counter exhausted".to_string());
        }
        let counter = self.counter;
        let nonce = self.nonce(kind, counter);
        let mut plaintext = Vec::with_capacity(HEADER_BYTES + payload.len());
        plaintext.extend_from_slice(&self.header(kind, counter));
        plaintext.extend_from_slice(payload);
        self.cipher
            .encrypt_in_place(&nonce, b"", &mut plaintext)
            .map_err(|_| "Orca E2EE encryption failed".to_string())?;
        self.counter = counter + 1;
        let mut frame = Vec::with_capacity(NONCE_BYTES + plaintext.len());
        frame.extend_from_slice(&nonce);
        frame.extend_from_slice(&plaintext);
        Ok(frame)
    }

    fn open(&mut self, kind: u8, frame: &[u8]) -> Result<Vec<u8>, String> {
        if frame.len() < NONCE_BYTES + HEADER_BYTES + TAG_BYTES {
            return Err("Orca E2EE frame too short".to_string());
        }
        let counter = self.counter;
        let nonce = self.nonce(kind, counter);
        if frame[..NONCE_BYTES] != nonce[..] {
            return Err("Orca E2EE nonce mismatch".to_string());
        }
        let mut ciphertext = frame[NONCE_BYTES..].to_vec();
        self.cipher
            .decrypt_in_place(&nonce, b"", &mut ciphertext)
            .map_err(|_| "Orca E2EE authentication failed".to_string())?;
        if ciphertext.len() < HEADER_BYTES
            || ciphertext[..HEADER_BYTES] != self.header(kind, counter)[..]
        {
            return Err("Orca E2EE header mismatch".to_string());
        }
        self.counter = counter
            .checked_add(1)
            .ok_or_else(|| "Orca E2EE counter exhausted".to_string())?;
        Ok(ciphertext[HEADER_BYTES..].to_vec())
    }
}

impl SessionCrypto {
    pub(crate) fn seal_text_base64(&mut self, plaintext: &str) -> Result<String, String> {
        self.seal_text(plaintext)
            .map(|frame| STANDARD.encode(frame))
    }

    pub(crate) fn transcript_hash_b64(&self) -> &str {
        &self.transcript_hash_b64
    }

    pub(crate) fn open_text_base64(&mut self, frame: &str) -> Result<Vec<u8>, String> {
        let bytes = STANDARD
            .decode(frame)
            .map_err(|_| "Invalid Orca E2EE text frame".to_string())?;
        if STANDARD.encode(&bytes) != frame {
            return Err("Non-canonical Orca E2EE text frame".to_string());
        }
        self.open_text(&bytes)
    }

    pub(crate) fn seal_text(&mut self, plaintext: &str) -> Result<Vec<u8>, String> {
        self.send.seal(TEXT_KIND, plaintext.as_bytes())
    }

    pub(crate) fn seal_binary(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        self.send.seal(BINARY_KIND, plaintext)
    }

    pub(crate) fn open_text(&mut self, frame: &[u8]) -> Result<Vec<u8>, String> {
        self.recv.open(TEXT_KIND, frame)
    }

    pub(crate) fn open_binary(&mut self, frame: &[u8]) -> Result<Vec<u8>, String> {
        self.recv.open(BINARY_KIND, frame)
    }
}

/// Process an Orca `e2ee_hello` and create the matching `e2ee_ready`.
pub(crate) fn respond_handshake(
    static_keys: &StaticKeys,
    raw: &str,
    expected: &Context,
) -> Result<HandshakeAccept, String> {
    respond_handshake_with_random(static_keys, raw, expected, random_bytes)
}

fn respond_handshake_with_random<F>(
    static_keys: &StaticKeys,
    raw: &str,
    expected: &Context,
    mut random: F,
) -> Result<HandshakeAccept, String>
where
    F: FnMut(usize) -> Result<Vec<u8>, String>,
{
    let hello: Value =
        serde_json::from_str(raw).map_err(|_| "Malformed Orca E2EE hello".to_string())?;
    validate_hello(&hello, expected)?;
    let client_public = decode_standard_exact(
        hello
            .get("clientPublicKeyB64")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing client public key".to_string())?,
        32,
    )?;
    let client_nonce = decode_standard_exact(
        hello
            .get("clientNonceB64")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing client nonce".to_string())?,
        32,
    )?;
    let desktop_nonce = random(32)?;
    let server_secret = StaticSecret::from(static_keys.secret_bytes());
    let server_public = PublicKey::from(&server_secret);
    let client_public = PublicKey::from(<[u8; 32]>::try_from(client_public).unwrap());
    let shared = server_secret.diffie_hellman(&client_public);
    if !shared.was_contributory() {
        return Err("Low-order Orca E2EE client key rejected".to_string());
    }

    let ready = json!({
        "type": "e2ee_ready",
        "v": VERSION,
        "desktopPublicKeyB64": STANDARD.encode(server_public.as_bytes()),
        "clientNonceB64": STANDARD.encode(&client_nonce),
        "desktopNonceB64": STANDARD.encode(&desktop_nonce),
        "selection": { "framing": 2, "payloadKinds": ["text", "binary"] },
        "context": hello.get("context").expect("validated context"),
    });
    validate_ready(&hello, &ready, expected)?;
    let transcript = encode_transcript(
        &hello,
        &ready,
        &client_public,
        &server_public,
        &client_nonce,
        &desktop_nonce,
    )?;
    let transcript_hash = Sha256::digest(&transcript);
    let schedule = derive_schedule(
        shared.as_bytes(),
        &transcript,
        &client_nonce,
        &desktop_nonce,
    );
    let client_to_desktop = schedule[..32].try_into().expect("key length");
    let desktop_to_client = schedule[32..64].try_into().expect("key length");
    let session_id = schedule[64..].try_into().expect("session length");
    let session = SessionCrypto {
        send: DirectionCipher::new(desktop_to_client, session_id, 1),
        recv: DirectionCipher::new(client_to_desktop, session_id, 0),
        transcript_hash_b64: STANDARD.encode(transcript_hash),
    };
    Ok(HandshakeAccept {
        ready_json: ready.to_string(),
        session,
    })
}

fn validate_hello(value: &Value, expected: &Context) -> Result<(), String> {
    require_keys(
        value,
        &[
            "type",
            "v",
            "clientPublicKeyB64",
            "clientNonceB64",
            "capabilities",
            "context",
        ],
    )?;
    if value.get("type") != Some(&Value::String("e2ee_hello".to_string()))
        || value.get("v").and_then(Value::as_u64) != Some(VERSION)
    {
        return Err("Invalid Orca E2EE hello version".to_string());
    }
    decode_standard_exact(
        value
            .get("clientPublicKeyB64")
            .and_then(Value::as_str)
            .unwrap_or(""),
        32,
    )?;
    decode_standard_exact(
        value
            .get("clientNonceB64")
            .and_then(Value::as_str)
            .unwrap_or(""),
        32,
    )?;
    let capabilities = value
        .get("capabilities")
        .ok_or_else(|| "Missing capabilities".to_string())?;
    require_keys(capabilities, &["framing", "payloadKinds"])?;
    if capabilities.get("framing") != Some(&json!([2]))
        || capabilities.get("payloadKinds") != Some(&json!(["text", "binary"]))
    {
        return Err("Unsupported Orca E2EE capabilities".to_string());
    }
    validate_context(value.get("context").unwrap(), expected)
}

fn validate_ready(hello: &Value, ready: &Value, expected: &Context) -> Result<(), String> {
    require_keys(
        ready,
        &[
            "type",
            "v",
            "desktopPublicKeyB64",
            "clientNonceB64",
            "desktopNonceB64",
            "selection",
            "context",
        ],
    )?;
    if ready.get("type") != Some(&Value::String("e2ee_ready".to_string()))
        || ready.get("v").and_then(Value::as_u64) != Some(VERSION)
    {
        return Err("Invalid Orca E2EE ready version".to_string());
    }
    decode_standard_exact(
        ready
            .get("desktopPublicKeyB64")
            .and_then(Value::as_str)
            .unwrap_or(""),
        32,
    )?;
    let client_nonce = ready
        .get("clientNonceB64")
        .and_then(Value::as_str)
        .unwrap_or("");
    if client_nonce
        != hello
            .get("clientNonceB64")
            .and_then(Value::as_str)
            .unwrap_or("")
    {
        return Err("Orca E2EE client nonce mismatch".to_string());
    }
    decode_standard_exact(
        ready
            .get("desktopNonceB64")
            .and_then(Value::as_str)
            .unwrap_or(""),
        32,
    )?;
    let selection = ready
        .get("selection")
        .ok_or_else(|| "Missing selection".to_string())?;
    require_keys(selection, &["framing", "payloadKinds"])?;
    if selection.get("framing").and_then(Value::as_u64) != Some(2)
        || selection.get("payloadKinds") != Some(&json!(["text", "binary"]))
    {
        return Err("Unsupported Orca E2EE selection".to_string());
    }
    validate_context(ready.get("context").unwrap(), expected)
}

fn validate_context(value: &Value, expected: &Context) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "Invalid Orca E2EE context".to_string())?;
    let required = match expected.transport {
        Transport::Direct => ["protocol", "initiator", "responder", "transport"].as_slice(),
        Transport::Relay => [
            "protocol",
            "initiator",
            "responder",
            "transport",
            "relayHostId",
        ]
        .as_slice(),
    };
    if object.len() != required.len() || required.iter().any(|key| !object.contains_key(*key)) {
        return Err("Invalid Orca E2EE context fields".to_string());
    }
    if object.get("protocol").and_then(Value::as_str) != Some(PROTOCOL)
        || object.get("initiator").and_then(Value::as_str) != Some("mobile")
        || object.get("responder").and_then(Value::as_str) != Some("desktop")
        || object.get("transport").and_then(Value::as_str) != Some(expected.transport.as_str())
    {
        return Err("Invalid Orca E2EE context identity".to_string());
    }
    match expected.transport {
        Transport::Direct => Ok(()),
        Transport::Relay => {
            let relay = object
                .get("relayHostId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !is_base64url_16(relay) || Some(relay) != expected.relay_host_id.as_deref() {
                return Err("Invalid Orca E2EE relay host".to_string());
            }
            Ok(())
        }
    }
}

fn encode_transcript(
    hello: &Value,
    ready: &Value,
    client_public: &PublicKey,
    desktop_public: &PublicKey,
    client_nonce: &[u8],
    desktop_nonce: &[u8],
) -> Result<Vec<u8>, String> {
    let context = hello
        .get("context")
        .ok_or_else(|| "Missing context".to_string())?;
    let fields: Vec<(&str, Vec<u8>)> = vec![
        ("domain", TRANSCRIPT_DOMAIN.to_vec()),
        ("mobile-to-desktop.type", bytes("e2ee_hello")),
        ("mobile-to-desktop.version", u32be(VERSION as u32).to_vec()),
        (
            "mobile-to-desktop.client-public-key",
            client_public.as_bytes().to_vec(),
        ),
        ("mobile-to-desktop.client-nonce", client_nonce.to_vec()),
        ("mobile-to-desktop.capabilities.framing", number_list(&[2])),
        (
            "mobile-to-desktop.capabilities.payload-kinds",
            string_list(&["text", "binary"]),
        ),
        (
            "mobile-to-desktop.context.protocol",
            json_string(context, "protocol")?,
        ),
        (
            "mobile-to-desktop.context.initiator",
            json_string(context, "initiator")?,
        ),
        (
            "mobile-to-desktop.context.responder",
            json_string(context, "responder")?,
        ),
        (
            "mobile-to-desktop.context.transport",
            json_string(context, "transport")?,
        ),
        (
            "mobile-to-desktop.context.relay-host-id",
            json_string_optional(context, "relayHostId"),
        ),
        ("desktop-to-mobile.type", bytes("e2ee_ready")),
        ("desktop-to-mobile.version", u32be(VERSION as u32).to_vec()),
        (
            "desktop-to-mobile.desktop-public-key",
            desktop_public.as_bytes().to_vec(),
        ),
        ("desktop-to-mobile.client-nonce-echo", client_nonce.to_vec()),
        ("desktop-to-mobile.desktop-nonce", desktop_nonce.to_vec()),
        ("desktop-to-mobile.selection.framing", u32be(2).to_vec()),
        (
            "desktop-to-mobile.selection.payload-kinds",
            string_list(&["text", "binary"]),
        ),
        (
            "desktop-to-mobile.context.protocol",
            json_string(ready.get("context").unwrap(), "protocol")?,
        ),
        (
            "desktop-to-mobile.context.initiator",
            json_string(ready.get("context").unwrap(), "initiator")?,
        ),
        (
            "desktop-to-mobile.context.responder",
            json_string(ready.get("context").unwrap(), "responder")?,
        ),
        (
            "desktop-to-mobile.context.transport",
            json_string(ready.get("context").unwrap(), "transport")?,
        ),
        (
            "desktop-to-mobile.context.relay-host-id",
            json_string_optional(ready.get("context").unwrap(), "relayHostId"),
        ),
    ];
    let mut result = Vec::new();
    for (name, value) in fields {
        let name = bytes(name);
        result.extend_from_slice(&u32be(name.len() as u32));
        result.extend_from_slice(&name);
        result.extend_from_slice(&u32be(value.len() as u32));
        result.extend_from_slice(&value);
    }
    Ok(result)
}

fn derive_schedule(
    shared: &[u8],
    transcript: &[u8],
    client_nonce: &[u8],
    desktop_nonce: &[u8],
) -> [u8; 96] {
    let transcript_hash = Sha256::digest(transcript);
    let mut salt_input = b"orca-mobile-e2ee/v2/salt\0".to_vec();
    salt_input.extend_from_slice(client_nonce);
    salt_input.extend_from_slice(desktop_nonce);
    let salt = Sha256::digest(salt_input);
    let mut info = b"orca-mobile-e2ee/v2/session\0".to_vec();
    info.extend_from_slice(&transcript_hash);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut expanded = [0u8; 96];
    hk.expand(&info, &mut expanded)
        .expect("HKDF output length is valid");
    expanded
}

fn json_string(value: &Value, key: &str) -> Result<Vec<u8>, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(bytes)
        .ok_or_else(|| format!("Missing context field {key}"))
}

fn json_string_optional(value: &Value, key: &str) -> Vec<u8> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(bytes)
        .unwrap_or_default()
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "Expected JSON object".to_string())?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err("Unexpected JSON fields".to_string());
    }
    Ok(())
}

fn decode_standard_exact(value: &str, expected_len: usize) -> Result<Vec<u8>, String> {
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| "Invalid canonical base64".to_string())?;
    if decoded.len() != expected_len || STANDARD.encode(&decoded) != value {
        return Err("Invalid canonical base64 length".to_string());
    }
    Ok(decoded)
}

fn is_base64url_16(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn bytes(value: &str) -> Vec<u8> {
    value.as_bytes().to_vec()
}

fn u32be(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

fn number_list(values: &[u32]) -> Vec<u8> {
    let mut result = Vec::with_capacity(4 + values.len() * 4);
    result.extend_from_slice(&u32be(values.len() as u32));
    for value in values {
        result.extend_from_slice(&u32be(*value));
    }
    result
}

fn string_list(values: &[&str]) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(&u32be(values.len() as u32));
    for value in values {
        let value = bytes(value);
        result.extend_from_slice(&u32be(value.len() as u32));
        result.extend_from_slice(&value);
    }
    result
}

fn random_bytes(length: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; length];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("CSPRNG unavailable: {error}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            transport: Transport::Direct,
            relay_host_id: None,
        }
    }

    fn hello(client_secret: &StaticSecret) -> String {
        let client_public = PublicKey::from(client_secret);
        json!({
            "type": "e2ee_hello",
            "v": 2,
            "clientPublicKeyB64": STANDARD.encode(client_public.as_bytes()),
            "clientNonceB64": STANDARD.encode([9u8; 32]),
            "capabilities": { "framing": [2], "payloadKinds": ["text", "binary"] },
            "context": { "protocol": PROTOCOL, "initiator": "mobile", "responder": "desktop", "transport": "direct" }
        }).to_string()
    }

    #[test]
    fn orca_handshake_and_bidirectional_frames_round_trip() {
        let server = StaticKeys::ephemeral().unwrap();
        let client_secret = StaticSecret::from([7u8; 32]);
        let accept =
            respond_handshake_with_random(&server, &hello(&client_secret), &context(), |_| {
                Ok(vec![3u8; 32])
            })
            .unwrap();
        let ready: Value = serde_json::from_str(&accept.ready_json).unwrap();
        assert_eq!(ready["type"], "e2ee_ready");
        assert_eq!(accept.session.transcript_hash_b64().len(), 44);

        let client_public = PublicKey::from(&client_secret);
        let server_public = PublicKey::from(&StaticSecret::from(server.secret_bytes()));
        let hello_value: Value = serde_json::from_str(&hello(&client_secret)).unwrap();
        let transcript = encode_transcript(
            &hello_value,
            &ready,
            &client_public,
            &server_public,
            &[9u8; 32],
            &[3u8; 32],
        )
        .unwrap();
        let shared = client_secret.diffie_hellman(&server_public);
        let schedule = derive_schedule(shared.as_bytes(), &transcript, &[9u8; 32], &[3u8; 32]);
        let transcript_hash_b64 = accept.session.transcript_hash_b64().to_owned();
        let mut client = SessionCrypto {
            send: DirectionCipher::new(
                schedule[..32].try_into().unwrap(),
                schedule[64..].try_into().unwrap(),
                0,
            ),
            recv: DirectionCipher::new(
                schedule[32..64].try_into().unwrap(),
                schedule[64..].try_into().unwrap(),
                1,
            ),
            transcript_hash_b64,
        };
        let request = client.seal_text("{\"id\":\"1\"}").unwrap();
        assert_eq!(accept.session.recv.counter, 0);
        let mut server_session = accept.session;
        assert_eq!(
            server_session.open_text(&request).unwrap(),
            br#"{"id":"1"}"#
        );
        assert!(
            server_session.open_text(&request).is_err(),
            "replay must fail"
        );
        let response = server_session.seal_binary(b"tty").unwrap();
        assert_eq!(client.open_binary(&response).unwrap(), b"tty");
    }

    #[test]
    fn context_and_extra_fields_are_rejected() {
        let server = StaticKeys::ephemeral().unwrap();
        let client_secret = StaticSecret::from([8u8; 32]);
        let mut value: Value = serde_json::from_str(&hello(&client_secret)).unwrap();
        value["unexpected"] = json!(true);
        assert!(respond_handshake(&server, &value.to_string(), &context()).is_err());

        let mut value: Value = serde_json::from_str(&hello(&client_secret)).unwrap();
        value["context"]["transport"] = json!("relay");
        assert!(respond_handshake(&server, &value.to_string(), &context()).is_err());
    }
}
