#[test]
fn config_directory_uses_openmeter_identity() {
    let path = openmeter_lib::platform::config_dir_from(std::path::Path::new(
        r"C:\Users\test\AppData\Roaming",
    ));

    assert_eq!(
        path,
        std::path::PathBuf::from(r"C:\Users\test\AppData\Roaming\OpenMeter")
    );
}
