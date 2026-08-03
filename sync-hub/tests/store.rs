use std::path::PathBuf;

use openmeter_sync_hub::{PutError, Store};
use openmeter_sync_protocol::{EncryptedEnvelope, EnvelopeMeta};

fn envelope(device_id: &str, revision: u64) -> EncryptedEnvelope {
    EncryptedEnvelope {
        meta: EnvelopeMeta::new(device_id, revision, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    }
}

#[test]
fn revision_must_increase_exactly_and_devices_cannot_overwrite_peers() {
    let path = temp_db();
    let store = Store::open(&path).unwrap();
    store.enroll_device("a", [1; 32], 900).unwrap();
    store.enroll_device("b", [2; 32], 900).unwrap();
    store.put("a", &envelope("a", 1), 1_000).unwrap();

    assert_eq!(
        store.put("a", &envelope("a", 1), 2_000),
        Err(PutError::Replay)
    );
    assert_eq!(
        store.put("b", &envelope("a", 2), 3_000),
        Err(PutError::DeviceMismatch)
    );
    assert_eq!(
        store.put("a", &envelope("a", 3), 4_000),
        Err(PutError::RevisionGap)
    );
    store.put("a", &envelope("a", 2), 5_000).unwrap();

    drop(store);
    let reopened = Store::open(&path).unwrap();
    let active = reopened.list_active("b", 5_001).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].meta.revision, 2);
    cleanup(path);
}

#[test]
fn revoked_record_is_hidden_immediately_and_purged_after_thirty_days() {
    let path = temp_db();
    let store = Store::open(&path).unwrap();
    store.enroll_device("a", [1; 32], 900).unwrap();
    store.put("a", &envelope("a", 1), 1_000).unwrap();
    store.revoke("a", 2_000).unwrap();

    assert!(store.list_active("b", 2_001).unwrap().is_empty());
    store
        .purge(2_000 + 30 * 24 * 60 * 60 * 1_000 + 1)
        .unwrap();
    assert_eq!(store.envelope_count().unwrap(), 0);
    cleanup(path);
}

fn temp_db() -> PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-sync-store-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn cleanup(path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
}
