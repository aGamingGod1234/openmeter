use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn config_dir_from(roaming_app_data: &Path) -> PathBuf {
    roaming_app_data.join("OpenMeter")
}

pub fn config_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = dirs::config_dir().unwrap_or_default();
        let destination = config_dir_from(&base);

        if !destination.exists() {
            for legacy_name in ["Pane", "OpenUsage"] {
                let legacy = base.join(legacy_name);
                if legacy.exists() {
                    if let Err(error) = std::fs::rename(&legacy, &destination) {
                        eprintln!(
                            "[openmeter] could not migrate {} to OpenMeter; continuing to use it: {error}",
                            legacy.display()
                        );
                        return legacy;
                    }
                    break;
                }
            }
        }

        destination
    })
    .clone()
}
