use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use openmeter_lib::environment::EnvironmentSnapshot;

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
