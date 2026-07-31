use openmeter_lib::refresh::{CliAction, CliOptions};

#[test]
fn parses_default_force_family_and_exact_account_invocations() {
    assert_eq!(
        CliOptions::parse([]).unwrap(),
        CliAction::Run(CliOptions::default())
    );
    assert_eq!(
        CliOptions::parse(["--force"]).unwrap(),
        CliAction::Run(CliOptions {
            force: true,
            filter: None,
        })
    );
    assert_eq!(
        CliOptions::parse(["codex"]).unwrap(),
        CliAction::Run(CliOptions {
            force: false,
            filter: Some("codex".to_string()),
        })
    );
    assert_eq!(
        CliOptions::parse(["codex--work", "--force"]).unwrap(),
        CliAction::Run(CliOptions {
            force: true,
            filter: Some("codex--work".to_string()),
        })
    );
}

#[test]
fn help_is_explicit_and_ambiguous_or_unknown_arguments_are_rejected() {
    assert_eq!(CliOptions::parse(["--help"]).unwrap(), CliAction::Help);
    assert!(CliOptions::parse(["codex", "claude"]).is_err());
    assert!(CliOptions::parse(["--force", "--force"]).is_err());
    assert!(CliOptions::parse(["--unknown"]).is_err());
}
