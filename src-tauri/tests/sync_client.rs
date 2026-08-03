use std::time::Duration;

use openmeter_lib::sync_client::{PendingEnvelope, RetryState};
use openmeter_sync_protocol::{EncryptedEnvelope, EnvelopeMeta};

#[test]
fn newest_pending_snapshot_replaces_older_and_backoff_is_capped() {
    let mut queue = PendingEnvelope::default();
    queue.replace(envelope(4));
    queue.replace(envelope(5));

    assert_eq!(queue.current().unwrap().meta.revision, 5);
    assert!(RetryState::after_failures(20).maximum_delay() <= Duration::from_secs(30 * 60));
}

#[test]
fn stale_pending_snapshot_cannot_replace_a_newer_revision() {
    let mut queue = PendingEnvelope::default();
    queue.replace(envelope(5));
    queue.replace(envelope(4));

    assert_eq!(queue.current().unwrap().meta.revision, 5);
}

fn envelope(revision: u64) -> EncryptedEnvelope {
    EncryptedEnvelope {
        meta: EnvelopeMeta::new("device-test", revision, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    }
}
