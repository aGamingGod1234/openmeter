use std::fs;
use std::process::Command;

#[test]
fn serve_rejects_wildcard_bind_before_opening_storage() {
    let output = Command::new(env!("CARGO_BIN_EXE_openmeter-sync-hub"))
        .args([
            "serve",
            "--bind",
            "0.0.0.0:6740",
            "--database",
            "missing-parent/hub.db",
            "--pepper-file",
            "missing-pepper.bin",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("private LAN or Tailscale"));
}

#[test]
fn enrollment_cli_creates_a_single_use_token_for_the_exact_tailnet_database() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let database = root.join("hub.db");
    let pepper = root.join("pepper.bin");
    fs::write(&pepper, [7_u8; 32]).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_openmeter-sync-hub"))
        .args([
            "enrollment",
            "create",
            "--database",
            database.to_str().unwrap(),
            "--pepper-file",
            pepper.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let token = String::from_utf8(output.stdout).unwrap();
    assert!(token.trim().len() >= 40);
    assert!(database.exists());

    let _ = fs::remove_dir_all(root);
}

fn temp_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-sync-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
