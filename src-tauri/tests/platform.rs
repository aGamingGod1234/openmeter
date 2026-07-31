use openmeter_lib::platform::{
    capture_affinity, privacy_tray_values, WDA_EXCLUDEFROMCAPTURE_VALUE, WDA_NONE_VALUE,
};

#[test]
fn capture_exclusion_uses_the_windows_11_exclude_from_capture_affinity() {
    assert_eq!(capture_affinity(true), WDA_EXCLUDEFROMCAPTURE_VALUE);
    assert_eq!(WDA_EXCLUDEFROMCAPTURE_VALUE, 0x11);
    assert_eq!(capture_affinity(false), WDA_NONE_VALUE);
}

#[test]
fn privacy_mode_hides_tray_values_without_changing_provider_state() {
    let live = vec![42, 75];
    assert!(privacy_tray_values(true, &live).is_empty());
    assert_eq!(privacy_tray_values(false, &live), live);
}
