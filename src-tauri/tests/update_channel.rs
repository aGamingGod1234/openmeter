use openmeter_lib::updates::{update_endpoint, UpdateChannel};

#[test]
fn stable_and_beta_select_distinct_signed_manifests() {
    assert_eq!(
        update_endpoint(UpdateChannel::Stable).as_str(),
        "https://github.com/aGamingGod1234/openmeter/releases/latest/download/latest.json"
    );
    assert_eq!(
        update_endpoint(UpdateChannel::Beta).as_str(),
        "https://github.com/aGamingGod1234/openmeter/releases/download/beta/latest.json"
    );
}

#[test]
fn unknown_saved_channels_migrate_to_stable() {
    assert_eq!(UpdateChannel::from_config(Some("nightly")), UpdateChannel::Stable);
    assert_eq!(UpdateChannel::from_config(None), UpdateChannel::Stable);
}
