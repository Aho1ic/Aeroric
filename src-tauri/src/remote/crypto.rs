//! 远程连接 E2EE(协议 v2):X25519 握手 + ChaCha20-Poly1305 帧加密。
//!
//! 信任模型:桌面持有长期静态 X25519 密钥对(`~/.aeroric/remote-keypair.json`,
//! 0600),公钥通过配对 QR 交给手机(首连 pinning)。握手为 Noise-NK 式:
//!
//! 1. 客户端明文 `hello` 携带临时公钥 c_eph;
//! 2. 服务端生成临时密钥 s_eph,派生
//!    `HKDF(dh(s_static, c_eph) || dh(s_eph, c_eph))` → 双向独立会话密钥,
//!    回 `hello_ack`(s_eph 公钥 + 用 s2c 密钥封的 confirm 标签);
//! 3. 客户端用 pin 的静态公钥做同样派生并验 confirm——只有真正持有静态私钥
//!    的主机才能算出会话密钥,中间人(包括盲转发 relay)无法伪造。
//!
//! 握手后所有 WS 消息均为二进制帧 `[kind:u8][seq:u64 BE][ciphertext]`:
//! kind=1 控制面 JSON,kind=2 终端流帧;kind 作 AAD,nonce = 4 字节 0 + seq,
//! 双向密钥独立、seq 严格递增,乱序/重放/篡改一律断连。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::storage::{aeroric_dir, atomic_write_private};

/// 加密帧内层类型:控制面(JSON 文本)。
pub const KIND_CTRL: u8 = 1;
/// 加密帧内层类型:终端流(terminal_frames 二进制帧)。
pub const KIND_TERMINAL: u8 = 2;

const HKDF_SALT: &[u8] = b"aeroric-remote-e2ee-v2";
const CONFIRM_PLAINTEXT: &[u8] = b"aeroric-e2ee-confirm-v2";
const CONFIRM_AAD: &[u8] = b"confirm";
/// seq(u64)+ 帧头 kind(u8)。
const FRAME_HEADER_BYTES: usize = 9;

fn random_secret() -> Result<StaticSecret, String> {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).map_err(|e| format!("CSPRNG unavailable: {e}"))?;
    Ok(StaticSecret::from(buf))
}

fn parse_public_key(b64: &str) -> Result<PublicKey, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(b64)
        .map_err(|_| "Malformed public key".to_string())?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Public key must be 32 bytes".to_string())?;
    Ok(PublicKey::from(arr))
}

// ── 静态密钥对 ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct KeypairFile {
    secret: String,
    public: String,
}

pub struct StaticKeys {
    secret: StaticSecret,
    public: PublicKey,
}

impl StaticKeys {
    /// 读取或首次生成静态密钥对;私钥文件 0600。
    pub fn load_or_create() -> Result<Self, String> {
        let path = aeroric_dir()?.join("remote-keypair.json");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(file) = serde_json::from_str::<KeypairFile>(&raw) {
                if let Ok(bytes) = URL_SAFE_NO_PAD.decode(&file.secret) {
                    if let Ok(arr) = <[u8; 32]>::try_from(bytes) {
                        let secret = StaticSecret::from(arr);
                        let public = PublicKey::from(&secret);
                        return Ok(Self { secret, public });
                    }
                }
            }
            return Err(format!(
                "Corrupted remote keypair file: {} (delete it to re-pair all devices)",
                path.display()
            ));
        }
        let keys = Self::ephemeral()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = KeypairFile {
            secret: URL_SAFE_NO_PAD.encode(keys.secret.as_bytes()),
            public: keys.public_b64(),
        };
        let raw = serde_json::to_string(&file).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok(keys)
    }

    /// 纯内存密钥对(测试 / 持久化失败时的降级)。
    pub fn ephemeral() -> Result<Self, String> {
        let secret = random_secret()?;
        let public = PublicKey::from(&secret);
        Ok(Self { secret, public })
    }

    pub fn public_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.public.as_bytes())
    }

    /// Raw static key material for a negotiated compatibility protocol. The
    /// caller remains inside the remote module and never serializes the
    /// private key; this is only used to derive an in-memory session key.
    pub(crate) fn secret_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }

    /// hostId:静态公钥 SHA-256 前 16 字节的 base64url。用于 relay 撮合,
    /// 公开无害(公钥本身就在配对 QR 里)。
    pub fn host_id(&self) -> String {
        let digest = Sha256::digest(self.public.as_bytes());
        URL_SAFE_NO_PAD.encode(&digest[..16])
    }
}

// ── 单方向流加密 ─────────────────────────────────────────────────────────────

struct DirectionCipher {
    cipher: ChaCha20Poly1305,
    seq: u64,
}

impl DirectionCipher {
    fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
            seq: 0,
        }
    }

    fn nonce(seq: u64) -> Nonce {
        let mut bytes = [0u8; 12];
        bytes[4..].copy_from_slice(&seq.to_be_bytes());
        Nonce::from(bytes)
    }

    fn seal(&mut self, aad: &[u8], plain: &[u8]) -> Result<(u64, Vec<u8>), String> {
        let seq = self.seq;
        self.seq = seq.checked_add(1).ok_or("nonce space exhausted")?;
        let ct = self
            .cipher
            .encrypt(&Self::nonce(seq), Payload { msg: plain, aad })
            .map_err(|_| "encrypt failed".to_string())?;
        Ok((seq, ct))
    }

    fn open(&mut self, seq: u64, aad: &[u8], ct: &[u8]) -> Result<Vec<u8>, String> {
        if seq != self.seq {
            return Err(format!("out-of-order frame: got {seq}, want {}", self.seq));
        }
        let plain = self
            .cipher
            .decrypt(&Self::nonce(seq), Payload { msg: ct, aad })
            .map_err(|_| "frame authentication failed".to_string())?;
        self.seq = seq.checked_add(1).ok_or("nonce space exhausted")?;
        Ok(plain)
    }
}

// ── 会话 ─────────────────────────────────────────────────────────────────────

pub struct SessionCrypto {
    send: DirectionCipher,
    recv: DirectionCipher,
}

// 密钥材料不入 Debug 输出
impl std::fmt::Debug for SessionCrypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionCrypto").finish_non_exhaustive()
    }
}

impl SessionCrypto {
    pub fn encrypt_frame(&mut self, kind: u8, plain: &[u8]) -> Result<Vec<u8>, String> {
        let (seq, ct) = self.send.seal(&[kind], plain)?;
        let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + ct.len());
        frame.push(kind);
        frame.extend_from_slice(&seq.to_be_bytes());
        frame.extend_from_slice(&ct);
        Ok(frame)
    }

    pub fn decrypt_frame(&mut self, frame: &[u8]) -> Result<(u8, Vec<u8>), String> {
        if frame.len() < FRAME_HEADER_BYTES {
            return Err("frame too short".to_string());
        }
        let kind = frame[0];
        let seq = u64::from_be_bytes(frame[1..FRAME_HEADER_BYTES].try_into().expect("8 bytes"));
        let plain = self.recv.open(seq, &[kind], &frame[FRAME_HEADER_BYTES..])?;
        Ok((kind, plain))
    }
}

fn derive_keys(
    dh_static: &[u8],
    dh_ephemeral: &[u8],
    client_eph: &PublicKey,
    server_eph: &PublicKey,
    server_static: &PublicKey,
) -> ([u8; 32], [u8; 32]) {
    let mut ikm = Vec::with_capacity(64);
    ikm.extend_from_slice(dh_static);
    ikm.extend_from_slice(dh_ephemeral);
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &ikm);
    let mut info = Vec::with_capacity(96);
    info.extend_from_slice(client_eph.as_bytes());
    info.extend_from_slice(server_eph.as_bytes());
    info.extend_from_slice(server_static.as_bytes());
    let mut okm = [0u8; 64];
    hk.expand(&info, &mut okm)
        .expect("64 bytes is a valid HKDF-SHA256 length");
    let client_to_server: [u8; 32] = okm[..32].try_into().expect("32 bytes");
    let server_to_client: [u8; 32] = okm[32..].try_into().expect("32 bytes");
    (client_to_server, server_to_client)
}

pub struct HandshakeAccept {
    pub session: SessionCrypto,
    pub server_eph_pub_b64: String,
    pub confirm_b64: String,
}

/// 服务端处理 hello:生成临时密钥、派生会话密钥、产出 confirm 标签
/// (消耗 s2c 方向 seq 0,客户端验签后双方 seq 对齐)。
pub fn respond_handshake(
    static_keys: &StaticKeys,
    client_eph_pub_b64: &str,
) -> Result<HandshakeAccept, String> {
    let client_eph = parse_public_key(client_eph_pub_b64)?;
    let server_eph_secret = random_secret()?;
    let server_eph_pub = PublicKey::from(&server_eph_secret);
    let dh_static = static_keys.secret.diffie_hellman(&client_eph);
    let dh_ephemeral = server_eph_secret.diffie_hellman(&client_eph);
    if !dh_static.was_contributory() || !dh_ephemeral.was_contributory() {
        return Err("low-order client key rejected".to_string());
    }
    let (key_c2s, key_s2c) = derive_keys(
        dh_static.as_bytes(),
        dh_ephemeral.as_bytes(),
        &client_eph,
        &server_eph_pub,
        &static_keys.public,
    );
    let mut send = DirectionCipher::new(&key_s2c);
    let recv = DirectionCipher::new(&key_c2s);
    let (_, confirm) = send.seal(CONFIRM_AAD, CONFIRM_PLAINTEXT)?;
    Ok(HandshakeAccept {
        session: SessionCrypto { send, recv },
        server_eph_pub_b64: URL_SAFE_NO_PAD.encode(server_eph_pub.as_bytes()),
        confirm_b64: URL_SAFE_NO_PAD.encode(confirm),
    })
}

// ── 客户端侧(集成测试模拟手机;真实客户端为 RN 端 e2ee.ts) ─────────────────

#[cfg(test)]
pub struct ClientHandshake {
    eph_secret: StaticSecret,
    pub eph_pub_b64: String,
    server_static: PublicKey,
}

#[cfg(test)]
pub fn initiate_handshake(server_static_pub_b64: &str) -> Result<ClientHandshake, String> {
    let server_static = parse_public_key(server_static_pub_b64)?;
    let eph_secret = random_secret()?;
    let eph_pub_b64 = URL_SAFE_NO_PAD.encode(PublicKey::from(&eph_secret).as_bytes());
    Ok(ClientHandshake {
        eph_secret,
        eph_pub_b64,
        server_static,
    })
}

#[cfg(test)]
impl ClientHandshake {
    /// 用服务端 hello_ack 完成握手;confirm 验证失败即视为主机身份不符。
    pub fn finish(
        self,
        server_eph_pub_b64: &str,
        confirm_b64: &str,
    ) -> Result<SessionCrypto, String> {
        let server_eph = parse_public_key(server_eph_pub_b64)?;
        let client_eph_pub = PublicKey::from(&self.eph_secret);
        let dh_static = self.eph_secret.diffie_hellman(&self.server_static);
        let dh_ephemeral = self.eph_secret.diffie_hellman(&server_eph);
        if !dh_static.was_contributory() || !dh_ephemeral.was_contributory() {
            return Err("low-order server key rejected".to_string());
        }
        let (key_c2s, key_s2c) = derive_keys(
            dh_static.as_bytes(),
            dh_ephemeral.as_bytes(),
            &client_eph_pub,
            &server_eph,
            &self.server_static,
        );
        let send = DirectionCipher::new(&key_c2s);
        let mut recv = DirectionCipher::new(&key_s2c);
        let confirm = URL_SAFE_NO_PAD
            .decode(confirm_b64)
            .map_err(|_| "Malformed confirm tag".to_string())?;
        let plain = recv
            .open(0, CONFIRM_AAD, &confirm)
            .map_err(|_| "Host identity verification failed".to_string())?;
        if plain != CONFIRM_PLAINTEXT {
            return Err("Host identity verification failed".to_string());
        }
        Ok(SessionCrypto { send, recv })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake_pair() -> (SessionCrypto, SessionCrypto) {
        let server = StaticKeys::ephemeral().unwrap();
        let client = initiate_handshake(&server.public_b64()).unwrap();
        let accept = respond_handshake(&server, &client.eph_pub_b64).unwrap();
        let client_session = client
            .finish(&accept.server_eph_pub_b64, &accept.confirm_b64)
            .unwrap();
        (client_session, accept.session)
    }

    #[test]
    fn handshake_and_frame_roundtrip_both_directions() {
        let (mut client, mut server) = handshake_pair();
        let frame = client.encrypt_frame(KIND_CTRL, b"{\"v\":2}").unwrap();
        let (kind, plain) = server.decrypt_frame(&frame).unwrap();
        assert_eq!(
            (kind, plain.as_slice()),
            (KIND_CTRL, b"{\"v\":2}".as_slice())
        );

        let frame = server.encrypt_frame(KIND_TERMINAL, b"tty-bytes").unwrap();
        let (kind, plain) = client.decrypt_frame(&frame).unwrap();
        assert_eq!(
            (kind, plain.as_slice()),
            (KIND_TERMINAL, b"tty-bytes".as_slice())
        );
    }

    #[test]
    fn wrong_static_key_fails_confirm() {
        let real = StaticKeys::ephemeral().unwrap();
        let imposter = StaticKeys::ephemeral().unwrap();
        // 客户端 pin 的是 real 的公钥,但应答方是 imposter
        let client = initiate_handshake(&real.public_b64()).unwrap();
        let accept = respond_handshake(&imposter, &client.eph_pub_b64).unwrap();
        let err = client
            .finish(&accept.server_eph_pub_b64, &accept.confirm_b64)
            .unwrap_err();
        assert!(err.contains("identity verification failed"));
    }

    #[test]
    fn replayed_frame_is_rejected() {
        let (mut client, mut server) = handshake_pair();
        let frame = client.encrypt_frame(KIND_CTRL, b"once").unwrap();
        server.decrypt_frame(&frame).unwrap();
        assert!(server.decrypt_frame(&frame).is_err(), "replay must fail");
    }

    #[test]
    fn tampered_frame_is_rejected() {
        let (mut client, mut server) = handshake_pair();
        let mut frame = client.encrypt_frame(KIND_CTRL, b"payload").unwrap();
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert!(server.decrypt_frame(&frame).is_err());
        // kind(AAD)被改同样失败
        let mut frame2 = client.encrypt_frame(KIND_CTRL, b"payload").unwrap();
        frame2[0] = KIND_TERMINAL;
        assert!(server.decrypt_frame(&frame2).is_err());
    }

    #[test]
    fn low_order_client_key_is_rejected() {
        let server = StaticKeys::ephemeral().unwrap();
        let zero = URL_SAFE_NO_PAD.encode([0u8; 32]);
        assert!(respond_handshake(&server, &zero).is_err());
    }

    #[test]
    fn host_id_is_stable_and_short() {
        let keys = StaticKeys::ephemeral().unwrap();
        assert_eq!(keys.host_id(), keys.host_id());
        assert_eq!(keys.host_id().len(), 22); // 16 bytes → 22 chars base64url
    }
}
