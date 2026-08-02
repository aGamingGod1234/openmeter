use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use openmeter_lib::environment::{
    install_launch_environment, launch_environment, EnvironmentSnapshot,
};
use openmeter_lib::{accounts::AccountContext, providers};

#[test]
fn uses_the_values_captured_at_launch() {
    let mut variables = BTreeMap::new();
    variables.insert(
        "CLAUDE_CONFIG_DIR".to_string(),
        OsString::from(r"C:\Claude\work"),
    );

    let snapshot = EnvironmentSnapshot::from_values(PathBuf::from(r"C:\Users\test"), variables);

    assert_eq!(
        snapshot.var("CLAUDE_CONFIG_DIR"),
        Some(std::ffi::OsStr::new(r"C:\Claude\work"))
    );
    assert_eq!(snapshot.home_dir(), PathBuf::from(r"C:\Users\test"));
    assert_eq!(snapshot.var("CODEX_HOME"), None);
}

#[test]
fn provider_runtime_reads_credentials_from_the_launch_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "openmeter-environment-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let credential = root.join(".claude").join(".credentials.json");
    std::fs::create_dir_all(credential.parent().unwrap()).unwrap();
    std::fs::write(&credential, br#"{"claudeAiOauth":{}}"#).unwrap();

    let snapshot = EnvironmentSnapshot::from_values(root.clone(), BTreeMap::new());
    let runtime = providers::runtime_for_with_environment("claude", snapshot).unwrap();
    let account = AccountContext::default_for("claude").unwrap();

    let probe = runtime.probe(&account).unwrap();

    assert!(probe
        .source_id
        .split(';')
        .any(|source| std::path::Path::new(source) == credential));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_and_codex_refresh_paths_use_the_launch_snapshot() {
    let mut variables = BTreeMap::new();
    variables.insert(
        "CLAUDE_CONFIG_DIR".to_string(),
        OsString::from(r"C:\Accounts\claude-work"),
    );
    variables.insert(
        "CODEX_HOME".to_string(),
        OsString::from(r"C:\Accounts\codex-work"),
    );
    let snapshot = EnvironmentSnapshot::from_values(PathBuf::from(r"C:\Users\test"), variables);

    assert_eq!(
        providers::claude::credentials_path(&snapshot),
        PathBuf::from(r"C:\Accounts\claude-work\.credentials.json")
    );
    assert_eq!(
        providers::codex::auth_path(&snapshot),
        PathBuf::from(r"C:\Accounts\codex-work\auth.json")
    );
}

#[test]
fn installed_launch_environment_is_process_stable() {
    let snapshot =
        EnvironmentSnapshot::from_values(PathBuf::from(r"C:\StableHome"), BTreeMap::new());

    install_launch_environment(snapshot).unwrap();

    assert_eq!(
        launch_environment().home_dir(),
        PathBuf::from(r"C:\StableHome")
    );
    assert!(install_launch_environment(EnvironmentSnapshot::capture()).is_err());
}
