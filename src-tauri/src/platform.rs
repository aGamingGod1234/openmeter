use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WDA_NONE_VALUE: u32 = 0;
pub const WDA_EXCLUDEFROMCAPTURE_VALUE: u32 = 0x11;

pub fn capture_affinity(enabled: bool) -> u32 {
    if enabled {
        WDA_EXCLUDEFROMCAPTURE_VALUE
    } else {
        WDA_NONE_VALUE
    }
}

pub fn privacy_tray_values(enabled: bool, values: &[u32]) -> Vec<u32> {
    if enabled {
        Vec::new()
    } else {
        values.to_vec()
    }
}

#[cfg(windows)]
pub fn set_capture_exclusion(
    hwnd: windows::Win32::Foundation::HWND,
    enabled: bool,
) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
    };
    unsafe {
        SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(capture_affinity(enabled)))
            .map_err(|error| format!("set capture exclusion: {error}"))
    }
}

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

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temporary_path(path);
    let result = (|| -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        atomic_replace(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("openmeter.json");
    path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), nonce))
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(io::Error::other)
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)
}
