use openmeter_lib::credential_store::{CredentialStore, SecretVec};

#[test]
fn credential_debug_never_exposes_secret() {
    let secret = SecretVec::new(vec![1, 2, 3, 4]);
    assert_eq!(format!("{secret:?}"), "[secret]");
}

#[test]
fn disposable_windows_credential_round_trips_and_is_deleted() {
    let target = format!(
        "OpenMeter/test/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let secret = SecretVec::new(b"disposable-sync-secret".to_vec());

    CredentialStore::write(&target, &secret).unwrap();
    assert_eq!(
        CredentialStore::read(&target).unwrap().unwrap().expose(),
        secret.expose()
    );
    CredentialStore::delete(&target).unwrap();
    assert!(CredentialStore::read(&target).unwrap().is_none());
}
