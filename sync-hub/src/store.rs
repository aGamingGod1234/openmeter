use std::path::Path;
use std::sync::Mutex;

use openmeter_sync_protocol::{EncryptedEnvelope, MAX_ENVELOPE_BYTES};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use thiserror::Error;

const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Error)]
pub enum HubError {
    #[error("sync database unavailable")]
    Database,
    #[error("sync record is invalid")]
    InvalidRecord,
    #[error("sync device was not found")]
    UnknownDevice,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PutError {
    #[error("authenticated device does not match envelope")]
    DeviceMismatch,
    #[error("envelope revision was already stored")]
    Replay,
    #[error("envelope revision must increase exactly once")]
    RevisionGap,
    #[error("sync device was not found")]
    UnknownDevice,
    #[error("sync device is revoked")]
    Revoked,
    #[error("sync envelope is invalid")]
    InvalidEnvelope,
    #[error("sync database unavailable")]
    Database,
}

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, HubError> {
        let connection = Connection::open(path).map_err(|_| HubError::Database)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS devices (
                   device_id TEXT PRIMARY KEY,
                   credential_hash BLOB NOT NULL,
                   created_at_ms INTEGER NOT NULL,
                   revoked_at_ms INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS envelopes (
                   device_id TEXT PRIMARY KEY REFERENCES devices(device_id),
                   revision INTEGER NOT NULL,
                   generated_at_ms INTEGER NOT NULL,
                   received_at_ms INTEGER NOT NULL,
                   envelope_json BLOB NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS enrollment_tokens (
                   token_hash BLOB PRIMARY KEY,
                   expires_at_ms INTEGER NOT NULL,
                   consumed_at_ms INTEGER
                 );",
            )
            .map_err(|_| HubError::Database)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn enroll_device(
        &self,
        device_id: &str,
        credential_hash: [u8; 32],
        now_ms: i64,
    ) -> Result<(), HubError> {
        validate_device_id(device_id)?;
        let mut connection = self.connection.lock().map_err(|_| HubError::Database)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| HubError::Database)?;
        transaction
            .execute(
                "INSERT INTO devices(device_id, credential_hash, created_at_ms, revoked_at_ms)
                 VALUES (?1, ?2, ?3, NULL)",
                params![device_id, credential_hash.as_slice(), now_ms],
            )
            .map_err(|_| HubError::Database)?;
        transaction.commit().map_err(|_| HubError::Database)
    }

    pub fn put(
        &self,
        authenticated_device_id: &str,
        envelope: &EncryptedEnvelope,
        now_ms: i64,
    ) -> Result<(), PutError> {
        if authenticated_device_id != envelope.meta.device_id {
            return Err(PutError::DeviceMismatch);
        }
        if validate_device_id(authenticated_device_id).is_err()
            || envelope.meta.revision == 0
            || envelope.nonce.len() != 24
            || envelope.ciphertext.len() > MAX_ENVELOPE_BYTES
        {
            return Err(PutError::InvalidEnvelope);
        }
        let encoded = serde_json::to_vec(envelope).map_err(|_| PutError::InvalidEnvelope)?;
        if encoded.len() > MAX_ENVELOPE_BYTES + 1024 {
            return Err(PutError::InvalidEnvelope);
        }
        let revision =
            i64::try_from(envelope.meta.revision).map_err(|_| PutError::InvalidEnvelope)?;

        let mut connection = self.connection.lock().map_err(|_| PutError::Database)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| PutError::Database)?;
        let revoked_at: Option<Option<i64>> = transaction
            .query_row(
                "SELECT revoked_at_ms FROM devices WHERE device_id = ?1",
                [authenticated_device_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| PutError::Database)?;
        match revoked_at {
            None => return Err(PutError::UnknownDevice),
            Some(Some(_)) => return Err(PutError::Revoked),
            Some(None) => {}
        }
        let current: Option<i64> = transaction
            .query_row(
                "SELECT revision FROM envelopes WHERE device_id = ?1",
                [authenticated_device_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| PutError::Database)?;
        let expected = current.map_or(1, |value| value.saturating_add(1));
        if current.is_some_and(|value| revision <= value) {
            return Err(PutError::Replay);
        }
        if revision != expected {
            return Err(PutError::RevisionGap);
        }
        transaction
            .execute(
                "INSERT INTO envelopes(device_id, revision, generated_at_ms, received_at_ms, envelope_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(device_id) DO UPDATE SET
                   revision = excluded.revision,
                   generated_at_ms = excluded.generated_at_ms,
                   received_at_ms = excluded.received_at_ms,
                   envelope_json = excluded.envelope_json",
                params![
                    authenticated_device_id,
                    revision,
                    envelope.meta.generated_at_ms,
                    now_ms,
                    encoded
                ],
            )
            .map_err(|_| PutError::Database)?;
        transaction.commit().map_err(|_| PutError::Database)
    }

    pub fn list_active(
        &self,
        excluding_device: &str,
        _now_ms: i64,
    ) -> Result<Vec<EncryptedEnvelope>, HubError> {
        let connection = self.connection.lock().map_err(|_| HubError::Database)?;
        let mut statement = connection
            .prepare(
                "SELECT e.envelope_json
                 FROM envelopes e
                 JOIN devices d ON d.device_id = e.device_id
                 WHERE d.revoked_at_ms IS NULL AND d.device_id <> ?1
                 ORDER BY d.device_id",
            )
            .map_err(|_| HubError::Database)?;
        let rows = statement
            .query_map([excluding_device], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|_| HubError::Database)?;
        let mut envelopes = Vec::new();
        for row in rows {
            let bytes = row.map_err(|_| HubError::Database)?;
            let envelope = serde_json::from_slice(&bytes).map_err(|_| HubError::InvalidRecord)?;
            envelopes.push(envelope);
        }
        Ok(envelopes)
    }

    pub fn revoke(&self, device_id: &str, now_ms: i64) -> Result<(), HubError> {
        let mut connection = self.connection.lock().map_err(|_| HubError::Database)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| HubError::Database)?;
        let changed = transaction
            .execute(
                "UPDATE devices SET revoked_at_ms = COALESCE(revoked_at_ms, ?2)
                 WHERE device_id = ?1",
                params![device_id, now_ms],
            )
            .map_err(|_| HubError::Database)?;
        if changed == 0 {
            return Err(HubError::UnknownDevice);
        }
        transaction.commit().map_err(|_| HubError::Database)
    }

    pub fn purge(&self, now_ms: i64) -> Result<(), HubError> {
        let cutoff = now_ms.saturating_sub(RETENTION_MS);
        let mut connection = self.connection.lock().map_err(|_| HubError::Database)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| HubError::Database)?;
        transaction
            .execute(
                "DELETE FROM envelopes WHERE device_id IN (
                   SELECT device_id FROM devices
                   WHERE revoked_at_ms IS NOT NULL AND revoked_at_ms < ?1
                 )",
                [cutoff],
            )
            .map_err(|_| HubError::Database)?;
        transaction.commit().map_err(|_| HubError::Database)
    }

    pub fn envelope_count(&self) -> Result<u64, HubError> {
        let connection = self.connection.lock().map_err(|_| HubError::Database)?;
        connection
            .query_row("SELECT COUNT(*) FROM envelopes", [], |row| row.get(0))
            .map_err(|_| HubError::Database)
    }
}

fn validate_device_id(device_id: &str) -> Result<(), HubError> {
    if device_id.is_empty()
        || device_id.len() > 64
        || !device_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(HubError::InvalidRecord);
    }
    Ok(())
}
