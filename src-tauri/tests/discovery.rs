use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use openmeter_lib::accounts::AccountSourceKind;
use openmeter_lib::discovery::{discover_claude_sources, discover_codex_sources};
use openmeter_lib::environment::EnvironmentSnapshot;

#[test]
fn claude_discovers_bounded_homes_and_keeps_every_source_for_an_identity() {
    let home = temp_home("claude");
    write_claude(&home.join(".claude"), "org_same", "Personal");
    write_claude(&home.join(".claude-work"), "org_same", "Company");
    write_claude(
        &home.join(".config").join("claude-client"),
        "org_other",
        "Side",
    );
    let environment = snapshot(home.clone(), BTreeMap::new());

    let found = discover_claude_sources(&environment);

    assert_eq!(found.len(), 3);
    assert_eq!(
        found
            .iter()
            .map(|source| source.identity_key.as_str())
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert!(found.iter().any(|source| {
        source.identity_key == "org_same"
            && source.source.kind == AccountSourceKind::DefaultHome
            && source.source.holds_default_source
    }));
    assert!(found.iter().all(|source| source.provider_id == "claude"));

    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn codex_requires_a_non_secret_account_claim_and_honors_captured_codex_home() {
    let home = temp_home("codex");
    write(
        &home.join(".codex").join("auth.json"),
        r#"{"tokens":{"access_token":"secret"}}"#,
    );
    let alternate = home.join("codex-work");
    write(
        &alternate.join("auth.json"),
        r#"{"tokens":{"access_token":"secret","account_id":"acct-work"}}"#,
    );
    let mut variables = BTreeMap::new();
    variables.insert(
        "CODEX_HOME".to_string(),
        alternate.as_os_str().to_os_string(),
    );
    let environment = snapshot(home.clone(), variables);

    let found = discover_codex_sources(&environment);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].provider_id, "codex");
    assert_eq!(found[0].identity_key, "acct-work");
    assert_eq!(found[0].source.kind, AccountSourceKind::Directory);
    assert!(!found[0].source.holds_default_source);

    let _ = std::fs::remove_dir_all(home);
}

fn write_claude(root: &Path, identity: &str, label: &str) {
    write(
        &root.join(".claude.json"),
        &format!(
            r#"{{"oauthAccount":{{"accountUuid":"{identity}","organizationName":"{label}"}}}}"#
        ),
    );
    write(
        &root.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"secret"}}"#,
    );
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn snapshot(home: PathBuf, variables: BTreeMap<String, OsString>) -> EnvironmentSnapshot {
    EnvironmentSnapshot::from_values(home, variables)
}

fn temp_home(scope: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-discovery-{scope}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
