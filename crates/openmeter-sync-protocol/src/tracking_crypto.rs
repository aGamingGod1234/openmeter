use std::io::Read;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::model::{
    validate_id, EncryptedEnvelope, EnvelopeMeta, ProtocolError, MAX_ENVELOPE_BYTES,
    MAX_PAYLOAD_BYTES,
};
use crate::tracking::TrackingPayloadV2;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct TrackingKey {
    encryption: [u8; 32],
    event_id: [u8; 32],
}

impl std::fmt::Debug for TrackingKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[secret]")
    }
}

pub fn derive_tracking_key(recovery: &[u8; 32]) -> Result<TrackingKey, ProtocolError> {
    let hkdf = Hkdf::<Sha256>::new(None, recovery);
    let mut encryption = [0_u8; 32];
    let mut event_id = [0_u8; 32];
    hkdf.expand(b"openmeter/tracking/v2/encryption", &mut encryption)
        .map_err(|_| ProtocolError::Authentication)?;
    hkdf.expand(b"openmeter/tracking/v2/event-id", &mut event_id)
        .map_err(|_| ProtocolError::Authentication)?;
    Ok(TrackingKey {
        encryption,
        event_id,
    })
}

pub fn tracking_event_id(
    key: &TrackingKey,
    provider_id: &str,
    account_id: &str,
    source_record: &[u8],
) -> Result<String, ProtocolError> {
    validate_id(provider_id, 64)?;
    validate_id(account_id, 128)?;
    if source_record.is_empty() {
        return Err(ProtocolError::Invalid("empty event source"));
    }
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key.event_id)
        .map_err(|_| ProtocolError::Authentication)?;
    update_len_prefixed(&mut mac, provider_id.as_bytes());
    update_len_prefixed(&mut mac, account_id.as_bytes());
    update_len_prefixed(&mut mac, source_record);
    let digest = mac.finalize().into_bytes();
    Ok(format!("evt_{}", encode_hex(&digest)))
}

pub fn seal_tracking(
    key: &TrackingKey,
    meta: EnvelopeMeta,
    payload: &TrackingPayloadV2,
) -> Result<EncryptedEnvelope, ProtocolError> {
    meta.validate_tracking()?;
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
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key.encryption));
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

pub fn open_tracking(
    key: &TrackingKey,
    envelope: &EncryptedEnvelope,
) -> Result<TrackingPayloadV2, ProtocolError> {
    envelope.meta.validate_tracking()?;
    if envelope.nonce.len() != 24 {
        return Err(ProtocolError::Invalid("invalid nonce"));
    }
    if envelope.ciphertext.len() > MAX_ENVELOPE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let aad = serde_json::to_vec(&envelope.meta).map_err(|_| ProtocolError::Serialization)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key.encryption));
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
    let payload: TrackingPayloadV2 =
        serde_json::from_slice(&serialized).map_err(|_| ProtocolError::Serialization)?;
    payload.validate()?;
    Ok(payload)
}

fn update_len_prefixed(mac: &mut Hmac<Sha256>, value: &[u8]) {
    mac.update(&(value.len() as u64).to_le_bytes());
    mac.update(value);
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
