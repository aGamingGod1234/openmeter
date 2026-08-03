use std::io::Read;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::model::{
    EncryptedEnvelope, EnvelopeMeta, HistoryPayloadV1, ProtocolError, MAX_ENVELOPE_BYTES,
    MAX_PAYLOAD_BYTES,
};

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct HistoryKey([u8; 32]);

impl HistoryKey {
    pub fn random_bytes() -> [u8; 32] {
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        bytes
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for HistoryKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[secret]")
    }
}

pub fn seal(
    key: &HistoryKey,
    meta: EnvelopeMeta,
    payload: &HistoryPayloadV1,
) -> Result<EncryptedEnvelope, ProtocolError> {
    meta.validate()?;
    payload.validate()?;
    let serialized = serde_json::to_vec(payload).map_err(|_| ProtocolError::Serialization)?;
    if serialized.len() > MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let compressed = zstd::stream::encode_all(serialized.as_slice(), 3)
        .map_err(|_| ProtocolError::Compression)?;
    let aad = serde_json::to_vec(&meta).map_err(|_| ProtocolError::Serialization)?;
    let mut nonce = vec![0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.expose()));
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &compressed,
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolError::Authentication)?;
    if ciphertext.len() > MAX_ENVELOPE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    Ok(EncryptedEnvelope {
        meta,
        nonce,
        ciphertext,
    })
}

pub fn open(
    key: &HistoryKey,
    envelope: &EncryptedEnvelope,
) -> Result<HistoryPayloadV1, ProtocolError> {
    envelope.meta.validate()?;
    if envelope.nonce.len() != 24 {
        return Err(ProtocolError::Invalid("invalid nonce"));
    }
    if envelope.ciphertext.len() > MAX_ENVELOPE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let aad = serde_json::to_vec(&envelope.meta).map_err(|_| ProtocolError::Serialization)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.expose()));
    let compressed = cipher
        .decrypt(
            XNonce::from_slice(&envelope.nonce),
            Payload {
                msg: &envelope.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolError::Authentication)?;
    let decoder = zstd::stream::read::Decoder::new(compressed.as_slice())
        .map_err(|_| ProtocolError::Compression)?;
    let mut serialized = Vec::new();
    decoder
        .take((MAX_PAYLOAD_BYTES + 1) as u64)
        .read_to_end(&mut serialized)
        .map_err(|_| ProtocolError::Compression)?;
    if serialized.len() > MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let payload: HistoryPayloadV1 =
        serde_json::from_slice(&serialized).map_err(|_| ProtocolError::Serialization)?;
    payload.validate()?;
    Ok(payload)
}
