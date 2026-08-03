use std::sync::{Arc, Mutex};

use openmeter_lib::accounts::{AccountContext, AccountSource};
use openmeter_lib::providers::{
    account_credential_stamp, provider_catalog, CredentialMaterial, Metric, ProviderRuntime,
    ProviderSnapshot,
};

#[test]
fn provider_catalog_covers_openusage_parity_and_existing_pane_providers() {
    let ids: Vec<&str> = provider_catalog()
        .iter()
        .map(|provider| provider.id)
        .collect();
    assert_eq!(
        &ids[..10],
        &[
            "claude",
            "codex",
            "cursor",
            "antigravity",
            "copilot",
            "devin",
            "grok",
            "opencode",
            "openrouter",
            "zai",
        ]
    );
    for pane_provider in [
        "minimax",
        "deepseek",
        "moonshot",
        "elevenlabs",
        "ollama",
        "codebuff",
        "kilo",
        "aihubmix",
    ] {
        assert!(ids.contains(&pane_provider));
    }
}

#[test]
fn directory_accounts_stamp_their_own_credential_bytes() {
    let root =
        std::env::temp_dir().join(format!("openmeter-provider-account-{}", std::process::id()));
    let credential_dir = root.join(".codex");
    std::fs::create_dir_all(&credential_dir).unwrap();
    let credential_file = credential_dir.join("auth.json");
    std::fs::write(&credential_file, br#"{"token":"first"}"#).unwrap();

    let mut account = AccountContext::named("codex", "work", "Work").unwrap();
    account.sources = vec![AccountSource::directory("codex-work", root.clone()).unwrap()];
    let first = account_credential_stamp(&account);
    std::fs::write(&credential_file, br#"{"token":"second"}"#).unwrap();
    let second = account_credential_stamp(&account);
    assert_ne!(first, second);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn probe_and_refresh_share_account_resolution_and_never_surface_credentials() {
    tauri::async_runtime::block_on(async {
        let account = AccountContext::named("codex", "work", "Work").unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_by_fetch = Arc::clone(&observed);
        let runtime = ProviderRuntime::new(
            "codex",
            |_account| {
                Ok(CredentialMaterial::new(
                    "directory:work",
                    b"super-secret-access-token".to_vec(),
                ))
            },
            move |_account, credential| {
                observed_by_fetch
                    .lock()
                    .unwrap()
                    .push(credential.source_id().to_string());
                let secret = credential.expose().to_vec();
                Box::pin(async move {
                    let mut snapshot = ProviderSnapshot::ok(
                        "codex",
                        "Codex",
                        Some("Plus".to_string()),
                        vec![Metric::progress("Weekly", 42.0, None)],
                    );
                    snapshot.warning =
                        Some(format!("debug token {}", String::from_utf8_lossy(&secret)));
                    snapshot
                })
            },
        );

        let probe = runtime.probe(&account).unwrap();
        assert_eq!(probe.source_id, "directory:work");
        assert!(!probe.credential_stamp.contains("secret"));

        let snapshot = runtime.refresh(&account).await;
        assert_eq!(observed.lock().unwrap().as_slice(), &["directory:work"]);
        assert_eq!(snapshot.provider_id, "codex");
        assert_eq!(snapshot.account_id, "work");
        assert_eq!(snapshot.card_id, "codex--work");
        let raw = serde_json::to_string(&snapshot).unwrap();
        assert!(!raw.contains("super-secret-access-token"));
        assert!(raw.contains("[secret]"));
    });
}
