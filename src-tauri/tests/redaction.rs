use openmeter_lib::providers::{Metric, ProviderSnapshot};
use openmeter_lib::redaction::{credential_stamp, redact_snapshot, redact_text};

#[test]
fn redacts_tokens_emails_and_windows_user_paths_without_destroying_useful_context() {
    let secret = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
    let input = format!(
        "HTTP 401 for alice@example.com token={secret} at C:\\Users\\Alice\\.codex\\auth.json"
    );
    let redacted = redact_text(&input);
    assert!(redacted.contains("HTTP 401"));
    assert!(redacted.contains("[email]"));
    assert!(redacted.contains("[secret]"));
    assert!(redacted.contains("%USERPROFILE%"));
    assert!(!redacted.contains(secret));
    assert!(!redacted.contains("Alice"));
}

#[test]
fn snapshot_redaction_covers_every_user_visible_string_and_stamp_is_one_way() {
    let secret = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGljZSJ9.signature012345";
    let mut snapshot =
        ProviderSnapshot::error("codex", "Codex", format!("Bearer {secret} was rejected"));
    snapshot.warning = Some(format!("refresh_token={secret}"));
    snapshot.metrics = vec![Metric::progress(
        "Session",
        25.0,
        Some(format!("debug credential {secret}")),
    )];

    redact_snapshot(&mut snapshot);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains(secret));
    assert!(serialized.contains("[secret]"));

    let stamp = credential_stamp(secret.as_bytes());
    assert_ne!(stamp, secret);
    assert!(!stamp.contains("eyJ"));
    assert_eq!(stamp, credential_stamp(secret.as_bytes()));
    assert_ne!(stamp, credential_stamp(b"different credential"));
}
